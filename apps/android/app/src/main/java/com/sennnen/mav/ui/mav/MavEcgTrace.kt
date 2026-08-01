package com.sennnen.mav.ui.mav

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.roundToInt

/**
 * The recorded trace on ECG graph paper (ADR-034).
 *
 * The wearer is being told something about their heart; the waveform is the only part of that
 * they can judge for themselves, so it leads rather than being withheld. Drawn at a true time
 * base against a 1 mm / 5 mm grid, the same geometry as the report, so the screen and the PDF
 * cannot disagree about what was recorded.
 */
private const val MM_PER_SECOND = 25f
private const val MM_PER_MILLIVOLT = 10f

@Composable
fun MavEcgTrace(
    waveform: List<Float>,
    sampleRateHz: Int,
    modifier: Modifier = Modifier,
    height: Dp = 168.dp,
    /** Seconds shown per screen width when scrollable; the whole recording otherwise. */
    secondsPerScreen: Float = 4f,
    scrollable: Boolean = true,
) {
    if (waveform.isEmpty() || sampleRateHz <= 0) return
    val palette = MavTheme.palette
    val seconds = waveform.size.toFloat() / sampleRateHz
    val density = LocalDensity.current
    // One millimetre of ECG paper, in dp. Chosen so a screen width holds `secondsPerScreen`.
    val widthPx = with(density) { 360.dp.toPx() }
    val millimetre = widthPx / (secondsPerScreen * MM_PER_SECOND)
    val totalWidth = with(density) { (seconds * MM_PER_SECOND * millimetre).toDp() }

    val content: @Composable (Modifier) -> Unit = { inner ->
        Canvas(
            inner
                .height(height)
                .semantics {
                    contentDescription =
                        "Electrocardiogram trace, ${seconds.roundToInt()} seconds, " +
                            "${sampleRateHz} hertz, on standard ECG paper."
                },
        ) {
            // One grid token, two weights: the app's paper stays in the theme's register while
            // the report uses conventional ECG red. Same geometry, different surface.
            drawEcgPaper(
                millimetre,
                palette.grid.copy(alpha = palette.grid.alpha * 0.55f),
                palette.grid,
            )
            drawEcgTrace(waveform, sampleRateHz, millimetre, palette.accent)
        }
    }

    if (scrollable) {
        Box(modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
            content(Modifier.width(totalWidth))
        }
    } else {
        content(modifier.fillMaxWidth())
    }
}

private fun DrawScope.drawEcgPaper(millimetre: Float, fine: Color, bold: Color) {
    if (millimetre <= 0f) return
    var index = 0
    while (index * millimetre <= size.width) {
        val x = index * millimetre
        val heavy = index % 5 == 0
        drawLine(
            if (heavy) bold else fine,
            Offset(x, 0f),
            Offset(x, size.height),
            strokeWidth = if (heavy) 1.1f else 0.6f,
        )
        index++
    }
    index = 0
    while (index * millimetre <= size.height) {
        val y = index * millimetre
        val heavy = index % 5 == 0
        drawLine(
            if (heavy) bold else fine,
            Offset(0f, y),
            Offset(size.width, y),
            strokeWidth = if (heavy) 1.1f else 0.6f,
        )
        index++
    }
}

private fun DrawScope.drawEcgTrace(
    waveform: List<Float>,
    sampleRateHz: Int,
    millimetre: Float,
    ink: Color,
) {
    val finite = waveform.filter { it.isFinite() }
    if (finite.isEmpty()) return
    val centre = finite.sorted()[finite.size / 2]
    // Millivolt samples get the clinical 10 mm/mV. Anything else has no established scale, so the
    // trace is fitted to the panel and the caller says so rather than implying a calibration.
    val perMillivolt = millimetre * MM_PER_MILLIVOLT
    val span = finite.maxOf { abs(it - centre) }
    val gain = if (span * perMillivolt > size.height / 2f) {
        (size.height / 2f) / max(span, 0.000_001f)
    } else {
        perMillivolt
    }
    val midpoint = size.height / 2f
    val path = Path()
    var started = false
    waveform.forEachIndexed { index, sample ->
        if (!sample.isFinite()) return@forEachIndexed
        val x = index.toFloat() / sampleRateHz * MM_PER_SECOND * millimetre
        val y = (midpoint - (sample - centre) * gain).coerceIn(0f, size.height)
        if (started) path.lineTo(x, y) else { path.moveTo(x, y); started = true }
    }
    drawPath(path, ink, style = Stroke(width = 2.2f))
}
