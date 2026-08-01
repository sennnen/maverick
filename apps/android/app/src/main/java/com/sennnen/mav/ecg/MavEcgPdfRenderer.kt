package com.sennnen.mav.ecg

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Typeface
import android.graphics.pdf.PdfDocument
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import java.io.ByteArrayOutputStream
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import uniffi.mav_ffi.EcgReportPayload

data class MavEcgReportContent(
    val captureId: ULong,
    val recordedAt: Instant,
    val rhythm: String,
    val probabilities: FloatArray,
    val confidence: Float,
    val quality: Float,
    val sampleRateHz: Int,
    val sampleCount: Int,
    val sourceUnit: String,
    val waveform: FloatArray,
    val explanation: List<Segment>,
    val modelSha256: String,
    val preprocessingSha256: String,
    val algorithmVersion: String,
    val provisional: Boolean,
) {
    data class Segment(
        val startSecond: Int,
        val endSecond: Int,
        val importance: Float,
    )

    constructor(payload: EcgReportPayload) : this(
        captureId = payload.result.captureId,
        recordedAt = Instant.ofEpochSecond(
            payload.result.startedNs / 1_000_000_000,
            payload.result.startedNs % 1_000_000_000,
        ),
        rhythm = payload.result.rhythm,
        probabilities = floatArrayOf(
            payload.result.sinusProbability,
            payload.result.atrialFibrillationProbability,
            payload.result.otherAbnormalProbability,
        ),
        confidence = payload.result.confidenceMilli.toFloat() / 1_000f,
        quality = payload.result.qualityMilli.toFloat() / 1_000f,
        sampleRateHz = payload.result.sourceRateHz.toInt(),
        sampleCount = payload.result.sampleCount.toInt(),
        sourceUnit = payload.sourceUnit,
        waveform = payload.waveform.toFloatArray(),
        explanation = payload.result.explanation.map {
            Segment(
                startSecond = it.startSecond.toInt(),
                endSecond = it.endSecond.toInt(),
                importance = it.importanceMilli.toFloat() / 1_000f,
            )
        },
        modelSha256 = payload.result.modelSha256,
        preprocessingSha256 = payload.result.preprocessingSha256,
        algorithmVersion = payload.result.algorithmVersion,
        provisional = payload.result.provisional,
    )
}

object MavEcgPdfRenderer {
    private data class TraceScale(
        val center: Float,
        val pointsPerUnit: Float,
        val caption: String,
        /** True when [pointsPerUnit] is a real 10 mm/mV gain rather than a fitted relative one. */
        val calibrated: Boolean,
    )

    private const val PAGE_WIDTH = 595
    private const val PAGE_HEIGHT = 842
    private const val PAPER = 0xFFF6F4EC.toInt()
    private const val INK = 0xFF131B19.toInt()
    private const val SECONDARY = 0xFF475651.toInt()
    private const val TEAL = 0xFF2F5B56.toInt()
    private const val PALE_TEAL = 0xFFD1E0D6.toInt()
    private const val RULE = 0xFFC2C2B0.toInt()
    private const val POINTS_PER_MILLIMETRE = 72f / 25.4f
    private const val TRACE_WIDTH = 125f * POINTS_PER_MILLIMETRE
    private const val TRACE_HEIGHT = 22f * POINTS_PER_MILLIMETRE

    fun render(report: MavEcgReportContent): ByteArray {
        val document = PdfDocument()
        try {
            val page = document.startPage(
                PdfDocument.PageInfo.Builder(PAGE_WIDTH, PAGE_HEIGHT, 1).create(),
            )
            drawReport(report, page.canvas)
            document.finishPage(page)
            return ByteArrayOutputStream().use { output ->
                document.writeTo(output)
                output.toByteArray()
            }
        } finally {
            document.close()
        }
    }

    private fun drawReport(report: MavEcgReportContent, canvas: Canvas) {
        canvas.drawColor(PAPER)
        text(canvas, "MAVERICK", 42f, 48f, 12f, INK, Typeface.BOLD, letterSpacing = 0.18f)
        text(canvas, "ECG REPORT", 42f, 76f, 9f, TEAL, Typeface.BOLD, letterSpacing = 0.12f)
        text(canvas, rhythmTitle(report.rhythm), 42f, 118f, 31f, INK, Typeface.NORMAL)
        text(
            canvas,
            if (report.provisional) {
                "Provisional on-device interpretation"
            } else {
                "On-device interpretation"
            },
            44f,
            143f,
            11.5f,
            SECONDARY,
            Typeface.BOLD,
        )

        val formatter = DateTimeFormatter.ofPattern("d MMM uuuu, HH:mm")
            .withZone(ZoneId.systemDefault())
        metric(canvas, "RECORDED", formatter.format(report.recordedAt), 42f, 174f)
        metric(canvas, "DURATION", "30 seconds", 226f, 174f)
        metric(canvas, "QUALITY", percent(report.quality), 348f, 174f)
        metric(canvas, "CONFIDENCE", percent(report.confidence), 454f, 174f)

        text(canvas, "MODEL VIEW", 42f, 224f, 8.5f, TEAL, Typeface.BOLD, letterSpacing = 0.1f)
        val labels = listOf("Sinus", "Atrial fibrillation", "Other")
        repeat(3) { index ->
            probability(
                canvas,
                labels[index],
                report.probabilities.getOrElse(index) { 0f },
                42f + index * 174f,
                245f,
                158f,
            )
        }

        val scale = traceScale(report)
        text(
            canvas,
            "30-SECOND RHYTHM STRIP",
            42f,
            286f,
            8.5f,
            TEAL,
            Typeface.BOLD,
            letterSpacing = 0.1f,
        )
        text(
            canvas,
            scale.caption,
            552f,
            286f,
            7.5f,
            SECONDARY,
            Typeface.NORMAL,
            align = Paint.Align.RIGHT,
            monospace = true,
        )

        val graphX = 42f
        val graphY = 296f
        val stride = 68f
        repeat(6) { strip ->
            val top = graphY + strip * stride
            drawGrid(canvas, graphX, top, TRACE_WIDTH, TRACE_HEIGHT)
            drawTrace(
                canvas,
                report.waveform,
                report.sampleRateHz,
                strip,
                scale,
                graphX,
                top,
                TRACE_WIDTH,
                TRACE_HEIGHT,
            )
            if (strip == 0) drawCalibrationPulse(canvas, scale, graphX, top, TRACE_HEIGHT)
            traceAnnotation(canvas, report, strip, graphX + TRACE_WIDTH + 18f, top)
        }

        text(canvas, "HOW TO READ", 42f, 716f, 8.5f, TEAL, Typeface.BOLD, letterSpacing = 0.1f)
        paragraph(
            canvas,
            "Each row is five seconds at a true 25 mm/s time base. The same vertical gain is used " +
                "throughout. Model influence shows which masked interval most changed the winning score.",
            42f,
            727f,
            510,
            8.8f,
            SECONDARY,
        )
        text(canvas, "READ WITH CARE", 42f, 766f, 8.5f, TEAL, Typeface.BOLD, letterSpacing = 0.1f)
        paragraph(
            canvas,
            "This research-only software result is not a diagnosis. Seek urgent care for chest pain, " +
                "fainting, severe breathlessness, or other concerning symptoms. A clinician should " +
                "interpret this single-lead recording in context.",
            42f,
            777f,
            510,
            8.8f,
            SECONDARY,
        )
        footer(canvas, report)
    }

    private fun metric(canvas: Canvas, label: String, value: String, x: Float, y: Float) {
        text(canvas, label, x, y, 7f, SECONDARY, Typeface.BOLD, letterSpacing = 0.08f)
        text(canvas, value, x, y + 17f, 10.5f, INK, Typeface.BOLD)
    }

    private fun probability(
        canvas: Canvas,
        label: String,
        value: Float,
        x: Float,
        y: Float,
        width: Float,
    ) {
        text(canvas, label, x, y, 9f, INK, Typeface.BOLD)
        text(
            canvas,
            percent(value),
            x + width,
            y,
            8.5f,
            INK,
            Typeface.BOLD,
            align = Paint.Align.RIGHT,
            monospace = true,
        )
        val paint = Paint(Paint.ANTI_ALIAS_FLAG)
        paint.color = PALE_TEAL
        canvas.drawRoundRect(x, y + 9f, x + width, y + 13f, 2f, 2f, paint)
        paint.color = TEAL
        canvas.drawRoundRect(
            x,
            y + 9f,
            x + width * value.clamped(),
            y + 13f,
            2f,
            2f,
            paint,
        )
    }

    private fun traceAnnotation(
        canvas: Canvas,
        report: MavEcgReportContent,
        strip: Int,
        x: Float,
        top: Float,
    ) {
        val start = strip * 5
        val importance = report.explanation.getOrNull(strip)?.importance?.clamped() ?: 0f
        text(
            canvas,
            "$start-${start + 5} s",
            x,
            top + 10f,
            8f,
            INK,
            Typeface.BOLD,
            monospace = true,
        )
        text(
            canvas,
            "MODEL INFLUENCE",
            x,
            top + 26f,
            6.5f,
            SECONDARY,
            Typeface.BOLD,
            letterSpacing = 0.06f,
        )
        val paint = Paint(Paint.ANTI_ALIAS_FLAG)
        paint.color = PALE_TEAL
        canvas.drawRoundRect(x, top + 34f, x + 119f, top + 39f, 2.5f, 2.5f, paint)
        paint.color = TEAL
        canvas.drawRoundRect(
            x,
            top + 34f,
            x + 119f * importance,
            top + 39f,
            2.5f,
            2.5f,
            paint,
        )
        text(
            canvas,
            percent(importance),
            552f,
            top + 51f,
            7f,
            SECONDARY,
            Typeface.NORMAL,
            align = Paint.Align.RIGHT,
            monospace = true,
        )
    }

    private fun drawGrid(
        canvas: Canvas,
        left: Float,
        top: Float,
        width: Float,
        height: Float,
    ) {
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            style = Paint.Style.STROKE
            strokeWidth = 0.22f
            color = Color.argb(120, 214, 138, 138)
        }
        val horizontal = (width / POINTS_PER_MILLIMETRE).toInt()
        val vertical = (height / POINTS_PER_MILLIMETRE).toInt()
        repeat(horizontal + 1) { index ->
            val x = left + index * POINTS_PER_MILLIMETRE
            canvas.drawLine(x, top, x, top + height, paint)
        }
        repeat(vertical + 1) { index ->
            val y = top + index * POINTS_PER_MILLIMETRE
            canvas.drawLine(left, y, left + width, y, paint)
        }
        paint.strokeWidth = 0.5f
        paint.color = Color.argb(205, 198, 96, 96)
        for (index in 0..horizontal step 5) {
            val x = left + index * POINTS_PER_MILLIMETRE
            canvas.drawLine(x, top, x, top + height, paint)
        }
        for (index in 0..vertical step 5) {
            val y = top + index * POINTS_PER_MILLIMETRE
            canvas.drawLine(left, y, left + width, y, paint)
        }
    }

    private fun drawTrace(
        canvas: Canvas,
        waveform: FloatArray,
        sampleRate: Int,
        strip: Int,
        scale: TraceScale,
        left: Float,
        top: Float,
        width: Float,
        height: Float,
    ) {
        if (sampleRate <= 0 || waveform.isEmpty()) return
        val start = strip * sampleRate * 5
        val end = min(waveform.size, start + sampleRate * 5)
        if (end - start < 2) return
        val path = Path()
        var started = false
        repeat(end - start) { offset ->
            val sample = waveform[start + offset]
            if (!sample.isFinite()) return@repeat
            val seconds = offset.toFloat() / sampleRate
            val x = left + seconds * 25f * POINTS_PER_MILLIMETRE
            val y = top + height / 2f - (sample - scale.center) * scale.pointsPerUnit
            if (started) {
                path.lineTo(x, y)
            } else {
                path.moveTo(x, y)
                started = true
            }
        }
        canvas.save()
        canvas.clipRect(left, top, left + width, top + height)
        canvas.drawPath(
            path,
            Paint(Paint.ANTI_ALIAS_FLAG).apply {
                color = TEAL
                style = Paint.Style.STROKE
                strokeWidth = 0.9f
                strokeJoin = Paint.Join.ROUND
                strokeCap = Paint.Cap.ROUND
            },
        )
        canvas.restore()
    }

    /**
     * The standard 1 mV calibration mark: a 5 mm step drawn at the head of the recording, in the
     * same gain as the trace. It is only honest where the gain means millivolts, so a source with
     * no established scale gets no pulse rather than a decorative one.
     */
    private fun drawCalibrationPulse(
        canvas: Canvas,
        scale: TraceScale,
        left: Float,
        top: Float,
        height: Float,
    ) {
        if (!scale.calibrated) return
        val millivolt = scale.pointsPerUnit
        val baseline = top + height / 2f
        val width = 5f * POINTS_PER_MILLIMETRE
        val path = Path().apply {
            moveTo(left, baseline)
            lineTo(left + width * 0.35f, baseline)
            lineTo(left + width * 0.35f, baseline - millivolt)
            lineTo(left + width, baseline - millivolt)
            lineTo(left + width, baseline)
            lineTo(left + width * 1.35f, baseline)
        }
        canvas.drawPath(
            path,
            Paint(Paint.ANTI_ALIAS_FLAG).apply {
                color = INK
                style = Paint.Style.STROKE
                strokeWidth = 0.9f
                strokeJoin = Paint.Join.MITER
            },
        )
    }

    private fun traceScale(report: MavEcgReportContent): TraceScale {
        val finite = report.waveform.filter(Float::isFinite).sorted()
        val center = if (finite.isEmpty()) 0f else finite[finite.size / 2]
        return when (report.sourceUnit) {
            "millivolts" -> TraceScale(
                center,
                10f * POINTS_PER_MILLIMETRE,
                "25 mm/s / 10 mm/mV / ${report.sampleRateHz} Hz",
                calibrated = true,
            )
            "microvolts" -> TraceScale(
                center,
                0.01f * POINTS_PER_MILLIMETRE,
                "25 mm/s / 10 mm/mV / ${report.sampleRateHz} Hz",
                calibrated = true,
            )
            "volts" -> TraceScale(
                center,
                10_000f * POINTS_PER_MILLIMETRE,
                "25 mm/s / 10 mm/mV / ${report.sampleRateHz} Hz",
                calibrated = true,
            )
            else -> {
                val deviations = finite.map { abs(it - center) }.sorted()
                val percentile = if (deviations.isEmpty()) {
                    1f
                } else {
                    deviations[min(deviations.lastIndex, (deviations.size * 0.98f).toInt())]
                }
                TraceScale(
                    center,
                    7f * POINTS_PER_MILLIMETRE / max(percentile, 0.000_001f),
                    "25 mm/s / shared relative gain / ${report.sampleRateHz} Hz",
                    calibrated = false,
                )
            }
        }
    }

    private fun footer(canvas: Canvas, report: MavEcgReportContent) {
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = RULE
            strokeWidth = 0.5f
        }
        canvas.drawLine(42f, 814f, 552f, 814f, paint)
        text(
            canvas,
            "Capture ${report.captureId} / Model ${report.modelSha256.take(12)} / " +
                "v${report.algorithmVersion} / ${report.sampleCount} samples",
            42f,
            830f,
            7f,
            SECONDARY,
            Typeface.NORMAL,
            monospace = true,
        )
        text(
            canvas,
            "1 / 1",
            552f,
            830f,
            7f,
            SECONDARY,
            Typeface.NORMAL,
            align = Paint.Align.RIGHT,
            monospace = true,
        )
    }

    private fun paragraph(
        canvas: Canvas,
        value: String,
        x: Float,
        y: Float,
        width: Int,
        size: Float,
        color: Int,
    ) {
        val paint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
            textSize = size
            this.color = color
            typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.NORMAL)
        }
        val layout = StaticLayout.Builder.obtain(value, 0, value.length, paint, width)
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .setLineSpacing(1.5f, 1f)
            .setIncludePad(false)
            .build()
        canvas.save()
        canvas.translate(x, y)
        layout.draw(canvas)
        canvas.restore()
    }

    private fun text(
        canvas: Canvas,
        value: String,
        x: Float,
        y: Float,
        size: Float,
        color: Int,
        style: Int,
        align: Paint.Align = Paint.Align.LEFT,
        letterSpacing: Float = 0f,
        monospace: Boolean = false,
    ) {
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            textSize = size
            this.color = color
            textAlign = align
            typeface = Typeface.create(
                if (monospace) Typeface.MONOSPACE else Typeface.SANS_SERIF,
                style,
            )
            this.letterSpacing = letterSpacing
        }
        canvas.drawText(value, x, y, paint)
    }

    private fun rhythmTitle(value: String): String = when (value) {
        "sinus_rhythm" -> "Sinus rhythm"
        "atrial_fibrillation" -> "Atrial fibrillation"
        else -> "Other rhythm"
    }

    private fun percent(value: Float): String = "${(value.clamped() * 100).toInt()}%"

    private fun Float.clamped(): Float = coerceIn(0f, 1f)
}
