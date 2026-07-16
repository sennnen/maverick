package com.sennnen.mav.ui.aura

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.background
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ShowChart
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.sennnen.mav.analytics.StageSegment
import kotlinx.coroutines.delay
import java.text.DateFormat
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.util.Date
import java.util.Locale
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.roundToInt

// The Aura data-viz layer (Android port of Strand/UI/AuraCharts.swift).
// Everything here is interactive: graphs scrub with a finger (value + date
// readout, haptic ticks), the hypnogram scrubs to the stage under your finger,
// zone bars expand on tap. All of it lives on dark cards and uses the adaptive
// contrast tokens.

data class AuraPoint(val day: String, val value: Double)

private val inFmt: DateTimeFormatter = DateTimeFormatter.ofPattern("yyyy-MM-dd", Locale.US)
private val shortFmt: DateTimeFormatter = DateTimeFormatter.ofPattern("d MMM")
private val longFmt: DateTimeFormatter = DateTimeFormatter.ofPattern("EEE d MMM")

fun auraShortDate(day: String?): String =
    runCatching { LocalDate.parse(day ?: return "", inFmt).format(shortFmt) }.getOrDefault("")

fun auraLongDate(day: String?): String =
    runCatching { LocalDate.parse(day ?: return "", inFmt).format(longFmt) }.getOrDefault("")

enum class AuraGraphStyle { LINE, BARS }

private val axisStyle = TextStyle(fontSize = 9.sp, fontWeight = FontWeight.Medium)

// MARK: - AuraGraph — the interactive trend chart

/**
 * A labelled, scrubbable series chart: y-gridlines with values, a dashed
 * average line, date axis, the latest point emphasised, and a drag-to-scrub
 * readout (value + full date) with haptic ticks. LINE draws a smoothed line +
 * soft area; BARS draws rounded columns.
 *
 * [points]: (dayKey yyyy-MM-dd, value), oldest → newest, already range-clipped.
 */
@Composable
fun AuraGraph(
    points: List<AuraPoint>,
    tint: Color,
    unit: String = "",
    style: AuraGraphStyle = AuraGraphStyle.LINE,
    decimals: Int = 0,
    height: Dp = 150.dp,
    /** Extra context per point (e.g. "8h 12m") shown in the scrub readout. */
    detail: ((Int) -> String)? = null,
) {
    val p = Aura.palette
    val haptics = LocalHapticFeedback.current
    var scrub by remember(points) { mutableStateOf<Int?>(null) }
    var dragging by remember { mutableStateOf(false) }

    // Scrub cursor decays 1.6s after the finger lifts.
    LaunchedEffect(dragging, scrub) {
        if (!dragging && scrub != null) {
            delay(1600)
            scrub = null
        }
    }

    fun fmt(v: Double): String =
        if (decimals == 0) v.roundToInt().toString()
        else String.format(Locale.US, "%.${decimals}f", v)

    if (points.size <= 1) {
        Column(
            Modifier.fillMaxWidth().heightIn(min = height),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Icon(
                Icons.AutoMirrored.Filled.ShowChart, contentDescription = null,
                tint = p.ink.copy(alpha = 0.3f), modifier = Modifier.size(24.dp),
            )
            Spacer(Modifier.height(6.dp))
            Text("Not enough history yet", style = AuraType.caption, color = p.ink.copy(alpha = 0.55f))
        }
        return
    }

    val values = points.map { it.value }
    val avg = values.sum() / max(values.size, 1)
    val lo = values.min()
    val hi = values.max()
    val textMeasurer = rememberTextMeasurer()

    fun indexAt(x: Float, plotW: Float): Int {
        val f = x / max(plotW, 1f) * (points.size - 1)
        return f.roundToInt().coerceIn(0, points.size - 1)
    }

    Column(
        Modifier
            .fillMaxWidth()
            .semantics { contentDescription = "Trend chart, latest ${fmt(points.last().value)} $unit" },
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        // MARK: Readout header
        val i = (scrub ?: points.size - 1).coerceIn(0, points.size - 1)
        val pt = points[i]
        Row(
            Modifier.fillMaxWidth().heightIn(min = 34.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            Text(fmt(pt.value), style = AuraType.number(30.sp), color = p.ink)
            if (unit.isNotEmpty()) {
                Text(
                    unit, style = AuraType.caption, color = p.ink.copy(alpha = 0.6f),
                    modifier = Modifier.padding(bottom = 4.dp),
                )
            }
            if (detail != null) {
                Text(
                    detail(i), style = AuraType.caption, color = p.ink.copy(alpha = 0.5f),
                    modifier = Modifier.padding(bottom = 4.dp),
                )
            }
            Spacer(Modifier.weight(1f))
            Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    if (scrub == null) "Latest" else auraLongDate(pt.day),
                    style = AuraType.caption, color = tint,
                )
                Text(
                    "avg ${fmt(avg)} · ${fmt(lo)}–${fmt(hi)}",
                    style = AuraType.caption, color = p.ink.copy(alpha = 0.5f),
                )
            }
        }

        // MARK: Chart body
        Canvas(
            Modifier
                .fillMaxWidth()
                .height(height)
                .pointerInput(points) {
                    detectDragGestures(
                        onDragStart = { pos ->
                            dragging = true
                            val idx = indexAt(pos.x, size.width - 34.dp.toPx())
                            if (idx != scrub) {
                                scrub = idx
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            }
                        },
                        onDrag = { change, _ ->
                            val idx = indexAt(change.position.x, size.width - 34.dp.toPx())
                            if (idx != scrub) {
                                scrub = idx
                                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            }
                            change.consume()
                        },
                        onDragEnd = { dragging = false },
                        onDragCancel = { dragging = false },
                    )
                }
                .pointerInput(points) {
                    detectTapGestures(onPress = { pos ->
                        dragging = true
                        val idx = indexAt(pos.x, size.width - 34.dp.toPx())
                        if (idx != scrub) {
                            scrub = idx
                            haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        }
                        tryAwaitRelease()
                        dragging = false
                    })
                }
        ) {
            val plotW = size.width - 34.dp.toPx()   // reserve right gutter for y labels
            val h = size.height
            val range = max(hi - lo, 0.001)
            val pad = range * 0.08                  // headroom so the line never kisses the edges
            val yLo = lo - pad
            val yRange = range + pad * 2
            fun y(v: Double): Float = h - ((v - yLo) / yRange).toFloat() * h
            fun x(idx: Int): Float =
                if (points.size == 1) plotW / 2
                else idx.toFloat() / (points.size - 1) * plotW
            fun barCenter(idx: Int): Float {
                val bw = plotW / points.size
                return idx * bw + bw / 2
            }

            // Y grid: min / mid / max lines + right-side value labels.
            for (v in listOf(lo, (lo + hi) / 2, hi)) {
                val yy = y(v)
                drawLine(p.grid, Offset(0f, yy), Offset(plotW, yy), strokeWidth = 1.dp.toPx())
                val layout = textMeasurer.measure(fmt(v), axisStyle)
                drawText(
                    layout,
                    color = p.ink.copy(alpha = 0.45f),
                    topLeft = Offset(
                        plotW + 18.dp.toPx() - layout.size.width / 2f,
                        (yy - layout.size.height / 2f).coerceIn(0f, h - layout.size.height),
                    ),
                )
            }

            // Average, dashed in the tint.
            val ay = y(avg)
            drawLine(
                tint.copy(alpha = 0.55f), Offset(0f, ay), Offset(plotW, ay),
                strokeWidth = 1.dp.toPx(),
                pathEffect = PathEffect.dashPathEffect(floatArrayOf(3.dp.toPx(), 3.dp.toPx())),
            )

            when (style) {
                AuraGraphStyle.LINE -> {
                    val pts = points.indices.map { Offset(x(it), y(points[it].value)) }
                    // Soft area under the line.
                    drawPath(
                        smoothPath(pts, closedTo = h),
                        Brush.verticalGradient(
                            0f to tint.copy(alpha = 0.30f), 1f to tint.copy(alpha = 0.02f),
                            startY = 0f, endY = h,
                        ),
                    )
                    // The line itself.
                    drawPath(
                        smoothPath(pts, closedTo = null),
                        tint,
                        style = Stroke(width = 2.dp.toPx(), cap = StrokeCap.Round),
                    )
                    // Latest point, emphasised.
                    pts.lastOrNull()?.let { last ->
                        drawCircle(p.bg, radius = 6.5f.dp.toPx(), center = last)
                        drawCircle(tint, radius = 4.5f.dp.toPx(), center = last)
                    }
                }
                AuraGraphStyle.BARS -> {
                    val n = points.size
                    val bw = plotW / n
                    for (idx in 0 until n) {
                        val top = y(points[idx].value)
                        val active = scrub == idx || (scrub == null && idx == n - 1)
                        val w = max(bw * 0.55f, 1.5f.dp.toPx())
                        drawRoundRect(
                            tint.copy(alpha = if (active) 1f else 0.45f),
                            topLeft = Offset(barCenter(idx) - w / 2, top),
                            size = Size(w, max(h - top, 3.dp.toPx())),
                            cornerRadius = CornerRadius(2.5f.dp.toPx()),
                        )
                    }
                }
            }

            // Scrub cursor.
            scrub?.let { s ->
                val sx = if (style == AuraGraphStyle.BARS) barCenter(s) else x(s)
                drawLine(p.ink.copy(alpha = 0.35f), Offset(sx, 0f), Offset(sx, h), strokeWidth = 1.dp.toPx())
                val c = Offset(sx, y(points[s].value))
                drawCircle(tint.copy(alpha = 0.45f), radius = 9.dp.toPx(), center = c)
                drawCircle(p.bg, radius = 7.5f.dp.toPx(), center = c)
                drawCircle(tint, radius = 5.5f.dp.toPx(), center = c)
            }
        }

        // MARK: Axis
        Row(Modifier.fillMaxWidth().padding(end = 34.dp)) {
            Text(auraShortDate(points.first().day), style = axisStyle, color = p.ink.copy(alpha = 0.45f))
            Spacer(Modifier.weight(1f))
            if (points.size > 4) {
                Text(auraShortDate(points[points.size / 2].day), style = axisStyle, color = p.ink.copy(alpha = 0.45f))
                Spacer(Modifier.weight(1f))
            }
            Text(auraShortDate(points.last().day), style = axisStyle, color = p.ink.copy(alpha = 0.45f))
        }
    }
}

/** Quad-smoothed path through the points (midpoint control), optionally closed
 *  down to [closedTo] (the baseline) for the area fill. */
private fun smoothPath(pts: List<Offset>, closedTo: Float?): Path = Path().apply {
    if (pts.isEmpty()) return@apply
    moveTo(pts.first().x, pts.first().y)
    if (pts.size == 2) {
        lineTo(pts[1].x, pts[1].y)
    } else {
        for (i in 1 until pts.size) {
            val prev = pts[i - 1]
            val cur = pts[i]
            quadraticBezierTo(prev.x, prev.y, (prev.x + cur.x) / 2, (prev.y + cur.y) / 2)
        }
        lineTo(pts.last().x, pts.last().y)
    }
    if (closedTo != null) {
        lineTo(pts.last().x, closedTo)
        lineTo(pts.first().x, closedTo)
        close()
    }
}

// MARK: - Hypnogram — scrubbable sleep-stage timeline

data class AuraStageRow(val stage: String, val label: String, val tintDark: Color, val tintLight: Color) {
    fun tint(dark: Boolean): Color = if (dark) tintDark else tintLight
}

val auraStageRows = listOf(
    AuraStageRow("wake", "Awake", Color(0xFFF5476A), Color(0xFFD83A44)),
    AuraStageRow("rem", "REM", Color(0xFF2BC8D9), Color(0xFF0F93A1)),
    AuraStageRow("light", "Light", Color(0xFF7FA5FF), Color(0xFF5B82D8)),
    AuraStageRow("deep", "Deep", Color(0xFF3E7BFF), Color(0xFF2F5FD0)),
)

private fun clockText(ts: Long): String =
    DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(ts * 1000))

private fun minsText(secs: Long): String {
    val m = secs / 60
    return if (m >= 60) "${m / 60}h ${m % 60}m" else "${m}m"
}

/** 4-row stage timeline with per-segment scrub readout and a totals row.
 *  Times in [segments] are wall-clock unix seconds. */
@Composable
fun AuraHypnogram(segments: List<StageSegment>, height: Dp = 132.dp) {
    val p = Aura.palette
    val haptics = LocalHapticFeedback.current
    var scrub by remember(segments) { mutableStateOf<Int?>(null) }
    var dragging by remember { mutableStateOf(false) }

    LaunchedEffect(dragging, scrub) {
        if (!dragging && scrub != null) {
            delay(1600)
            scrub = null
        }
    }

    val start = segments.minOfOrNull { it.start } ?: 0L
    val end = segments.maxOfOrNull { it.end } ?: 1L

    if (end <= start) {
        Text(
            "No staged sleep recorded", style = AuraType.caption,
            color = p.ink.copy(alpha = 0.55f),
            modifier = Modifier.fillMaxWidth().heightIn(min = 80.dp),
        )
        return
    }

    fun hitIndex(x: Float, width: Float): Int? {
        val ts = start + (x / max(width, 1f) * (end - start)).toLong()
        return segments.indices.firstOrNull { ts >= segments[it].start && ts < segments[it].end }
            ?: segments.indices.minByOrNull { abs(segments[it].start - ts) }
    }

    Column(
        Modifier.fillMaxWidth().semantics { contentDescription = "Sleep stages" },
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        // Readout
        Row(
            Modifier.heightIn(min = 20.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val s = scrub
            if (s != null) {
                val seg = segments[s]
                val row = auraStageRows.firstOrNull { it.stage == seg.stage }
                Box(Modifier.size(8.dp).background(row?.tint(p.dark) ?: p.ink, CircleShape))
                Text(
                    row?.label ?: seg.stage.replaceFirstChar { it.uppercase() },
                    style = AuraType.label, color = p.ink,
                )
                Text(
                    "${clockText(seg.start)}–${clockText(seg.end)} · ${minsText(seg.end - seg.start)}",
                    style = AuraType.caption, color = p.ink.copy(alpha = 0.6f),
                )
            } else {
                Text(
                    "Touch and hold to explore the night",
                    style = AuraType.caption, color = p.ink.copy(alpha = 0.45f),
                )
            }
        }

        // Chart: stage labels column + canvas
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Column(Modifier.width(38.dp).height(height)) {
                auraStageRows.forEach { row ->
                    Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                        Text(
                            row.label, style = TextStyle(fontSize = 10.sp, fontWeight = FontWeight.Medium),
                            color = p.ink.copy(alpha = 0.55f), maxLines = 1,
                        )
                    }
                }
            }
            Canvas(
                Modifier
                    .weight(1f)
                    .height(height)
                    .pointerInput(segments) {
                        detectDragGestures(
                            onDragStart = { pos ->
                                dragging = true
                                hitIndex(pos.x, size.width.toFloat())?.let {
                                    if (it != scrub) {
                                        scrub = it
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    }
                                }
                            },
                            onDrag = { change, _ ->
                                hitIndex(change.position.x, size.width.toFloat())?.let {
                                    if (it != scrub) {
                                        scrub = it
                                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    }
                                }
                                change.consume()
                            },
                            onDragEnd = { dragging = false },
                            onDragCancel = { dragging = false },
                        )
                    }
                    .pointerInput(segments) {
                        detectTapGestures(onPress = { pos ->
                            dragging = true
                            hitIndex(pos.x, size.width.toFloat())?.let {
                                if (it != scrub) {
                                    scrub = it
                                    haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                }
                            }
                            tryAwaitRelease()
                            dragging = false
                        })
                    }
            ) {
                val span = (end - start).toFloat()
                val rowH = size.height / auraStageRows.size
                fun rowY(r: Int): Float = rowH * r + rowH / 2
                fun rowIndex(stage: String): Int? =
                    auraStageRows.indexOfFirst { it.stage == stage }.takeIf { it >= 0 }

                // Row gridlines
                for (r in 1 until auraStageRows.size) {
                    drawLine(p.grid, Offset(0f, rowH * r), Offset(size.width, rowH * r), strokeWidth = 1.dp.toPx())
                }

                // Step connectors between consecutive segments.
                for (idx in 1 until segments.size) {
                    val a = segments[idx - 1]
                    val b = segments[idx]
                    if (a.end != b.start) continue
                    val ra = rowIndex(a.stage) ?: continue
                    val rb = rowIndex(b.stage) ?: continue
                    if (ra == rb) continue
                    val xx = (a.end - start) / span * size.width
                    drawLine(p.ink.copy(alpha = 0.18f), Offset(xx, rowY(ra)), Offset(xx, rowY(rb)), strokeWidth = 1.dp.toPx())
                }

                // Stage blocks, each over a soft same-hue shadow so the stages glow
                // faintly against the card (the Aura colored-shadow treatment).
                segments.forEachIndexed { idx, s ->
                    val r = rowIndex(s.stage) ?: return@forEachIndexed
                    val x0 = (s.start - start) / span * size.width
                    val w = max((s.end - s.start) / span * size.width, 3.dp.toPx())
                    val blockH = rowH * 0.52f
                    val tint = auraStageRows[r].tint(p.dark)
                    val emphasis = if (scrub == null || scrub == idx) 1f else 0.35f
                    val haloPad = 2.5f.dp.toPx()
                    drawRoundRect(
                        tint.copy(alpha = 0.24f * emphasis),
                        topLeft = Offset(x0 - haloPad, rowY(r) - blockH / 2 - haloPad),
                        size = Size(w + haloPad * 2, blockH + haloPad * 2),
                        cornerRadius = CornerRadius((blockH + haloPad * 2) / 2),
                    )
                    drawRoundRect(
                        tint.copy(alpha = emphasis),
                        topLeft = Offset(x0, rowY(r) - blockH / 2),
                        size = Size(w, blockH),
                        cornerRadius = CornerRadius(blockH / 2),
                    )
                }

                // Scrub cursor.
                scrub?.let { s ->
                    val seg = segments[s]
                    val mx = (seg.start + (seg.end - seg.start) / 2 - start) / span * size.width
                    drawLine(p.ink.copy(alpha = 0.25f), Offset(mx, 0f), Offset(mx, size.height), strokeWidth = 1.dp.toPx())
                }
            }
        }

        // Ticks
        Row(Modifier.fillMaxWidth().padding(start = 48.dp)) {
            Text(clockText(start), style = axisStyle, color = p.ink.copy(alpha = 0.45f))
            Spacer(Modifier.weight(1f))
            Text(clockText(start + (end - start) / 2), style = axisStyle, color = p.ink.copy(alpha = 0.45f))
            Spacer(Modifier.weight(1f))
            Text(clockText(end), style = axisStyle, color = p.ink.copy(alpha = 0.45f))
        }

        // Totals
        val total = (end - start).toDouble()
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            auraStageRows.forEach { row ->
                val secs = segments.filter { it.stage == row.stage }.sumOf { it.end - it.start }
                if (secs > 0) {
                    Row(horizontalArrangement = Arrangement.spacedBy(5.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(7.dp).background(row.tint(p.dark), CircleShape))
                        Text(
                            "${row.label} ${minsText(secs)} · ${(secs / total * 100).roundToInt()}%",
                            style = TextStyle(fontSize = 10.sp, fontWeight = FontWeight.Medium),
                            color = p.ink.copy(alpha = 0.7f),
                            maxLines = 1, overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        }
    }
}

// MARK: - HR zone bars — tap to inspect

private val zoneTintsDark = listOf(
    Color(0xFF8E9BA8), Color(0xFF2BC8D9), Color(0xFF14C078), Color(0xFFE0A81E), Color(0xFFF5476A),
)
private val zoneTintsLight = listOf(
    Color(0xFF7B8894), Color(0xFF0F93A1), Color(0xFF1F9E57), Color(0xFFC4841A), Color(0xFFD83A44),
)
private val zoneNames = listOf("Recovery", "Endurance", "Aerobic", "Threshold", "Max")

fun auraZoneTint(index: Int, dark: Boolean): Color =
    (if (dark) zoneTintsDark else zoneTintsLight)[index.coerceIn(0, 4)]

fun auraHm(m: Double): String {
    val t = m.roundToInt()
    return if (t >= 60) "${t / 60}h ${t % 60}m" else "${t}m"
}

/** Minutes per zone, index 0 = Z1 … 4 = Z5. Tap a zone to expand % of session
 *  and (when [hrMax] is provided) its bpm band. When [targets] is provided, a
 *  targeted zone's bar fills AGAINST its target (full bar = target reached, a
 *  checkmark appears) instead of relative-to-max; null entries mean no target
 *  for that zone (Android port of iOS AuraZoneBars' target mode). */
@Composable
fun AuraZoneBars(minutes: List<Double>, hrMax: Int? = null, targets: List<Double?>? = null) {
    val p = Aura.palette
    val haptics = LocalHapticFeedback.current
    var selected by remember { mutableStateOf<Int?>(null) }
    var appeared by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { appeared = true }

    val total = max(minutes.sum(), 0.001)
    val maxMin = max(minutes.maxOrNull() ?: 1.0, 1.0)
    val sweep by animateFloatAsState(
        targetValue = if (appeared) 1f else 0f,
        animationSpec = spring(dampingRatio = 0.85f, stiffness = Spring.StiffnessLow),
        label = "zoneSweep",
    )

    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        minutes.forEachIndexed { i, m ->
            val isSel = selected == i
            val tint = auraZoneTint(i, p.dark)
            val target = targets?.getOrNull(i)
            val fraction = target?.let { (m / max(it, 0.001)).coerceIn(0.0, 1.0) } ?: (m / maxMin)
            val met = target != null && m >= target
            Column(
                Modifier
                    .fillMaxWidth()
                    .clickable {
                        selected = if (isSel) null else i
                        haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                    }
                    .padding(vertical = 5.dp)
                    .semantics { contentDescription = "Zone ${i + 1}, ${auraHm(m)}" },
                verticalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        "Z${i + 1}",
                        style = TextStyle(fontSize = 11.sp, fontWeight = FontWeight.SemiBold),
                        color = if (isSel) tint else p.ink.copy(alpha = 0.6f),
                        modifier = Modifier.width(24.dp),
                    )
                    // Zone bar: rounded track + a gradient fill deepening toward the zone's hue.
                    Box(
                        Modifier
                            .weight(1f)
                            .height(9.dp)
                            .clip(CircleShape)
                            .background(MaterialTheme.colorScheme.surfaceVariant),
                    ) {
                        Box(
                            Modifier
                                .fillMaxHeight()
                                .fillMaxWidth((fraction.toFloat() * sweep).coerceIn(0f, 1f))
                                .clip(CircleShape)
                                .background(
                                    Brush.horizontalGradient(
                                        listOf(tint.copy(alpha = 0.55f), tint),
                                    ),
                                ),
                        )
                    }
                    Row(
                        Modifier.width(if (target != null) 62.dp else 52.dp),
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            auraHm(m),
                            style = TextStyle(fontSize = 11.sp, fontWeight = FontWeight.Medium),
                            color = p.ink.copy(alpha = 0.8f),
                            maxLines = 1,
                        )
                        if (met) {
                            Icon(
                                Icons.Filled.Check, contentDescription = "Target met",
                                tint = p.good, modifier = Modifier.size(11.dp),
                            )
                        }
                    }
                }
                AnimatedVisibility(visible = isSel) {
                    val pct = (m / total * 100).roundToInt()
                    var line = "${zoneNames[i]} · $pct% of session"
                    if (hrMax != null) {
                        val loP = 50 + i * 10
                        val hiP = 60 + i * 10
                        line += " · ${loP * hrMax / 100}–${hiP * hrMax / 100} bpm"
                    }
                    Text(
                        line, style = AuraType.caption, color = p.ink.copy(alpha = 0.6f),
                        modifier = Modifier.padding(start = 36.dp),
                    )
                }
            }
        }
    }
}
