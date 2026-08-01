package com.sennnen.mav.ui.mav

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlin.math.asin
import kotlin.math.max
import kotlin.math.roundToInt

// Everything that draws data. Each takes an accessibilitySummary it cannot be constructed without,
// because a chart with no text description is unusable. The iOS twin is UI/MavCharts.swift.
//
// Every chart is monochrome. Series are distinguished by shape, stroke weight and labels, never by
// unrelated category colours.

/**
 * The geometry of the open-bottom arc, in one place because both platforms draw it and a mock once
 * shipped with an assumed arc length that silently pinned every value above 0.86 to full.
 */
object MavArc {
    /** Half the chord across the opening, as a fraction of the radius. */
    const val OPENING_RATIO: Float = 25f / 30f

    /** How much of the circle the gap takes, in degrees. */
    val gapDegrees: Float get() = (2 * asin(OPENING_RATIO.toDouble()) * 180 / Math.PI).toFloat()

    /** How much of the circle the arc covers: 247.07 degrees. */
    val sweepDegrees: Float get() = 360f - gapDegrees

    /** Where the arc starts. 90 degrees is the bottom, so it begins half a gap past it. */
    val startDegrees: Float get() = 90f + gapDegrees / 2f
}

/**
 * An open-bottom arc. Deliberately not a closed ring, and deliberately not stacked with others: the
 * rail is a scrolling row of separate gauges, which is a different object from a three-ring card and
 * stays a different object.
 */
@Composable
@Suppress("UNUSED_PARAMETER") // Kept in the cross-platform API; every family is monochrome.
fun MavArcGauge(
    text: String,
    label: String,
    fraction: Double?,
    family: MavFamily,
    accessibilitySummary: String,
    onClick: () -> Unit,
) {
    val palette = MavTheme.palette
    val track = palette.ink.copy(alpha = 0.18f)
    val progress = remember { Animatable(0f) }
    LaunchedEffect(fraction) {
        if (fraction == null) {
            progress.snapTo(0f)
        } else {
            progress.animateTo(
                fraction.coerceIn(0.0, 1.0).toFloat(),
                animationSpec = tween(520, easing = FastOutSlowInEasing),
            )
        }
    }
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier
            .width(78.dp)
            .clip(RoundedCornerShape(MavTheme.chipRadius))
            .clickable(onClick = onClick)
            .semantics(mergeDescendants = true) { contentDescription = accessibilitySummary },
    ) {
        Box(Modifier.size(74.dp, 68.dp), contentAlignment = Alignment.Center) {
            Canvas(Modifier.size(68.dp)) {
                val inset = 2.dp.toPx()
                val diameter = size.minDimension - inset * 2
                val topLeft = Offset(
                    (size.width - diameter) / 2f,
                    (size.height - diameter) / 2f,
                )
                val arcSize = Size(diameter, diameter)
                drawArc(
                    color = track,
                    startAngle = MavArc.startDegrees,
                    sweepAngle = MavArc.sweepDegrees,
                    useCenter = false,
                    topLeft = topLeft,
                    size = arcSize,
                    style = Stroke(
                        width = 4.dp.toPx(),
                        cap = StrokeCap.Round,
                    ),
                )
                if (fraction != null) {
                    // The sweep is scaled directly, so nobody has to know how long the arc is.
                    drawArc(
                        color = palette.ink,
                        startAngle = MavArc.startDegrees,
                        sweepAngle = MavArc.sweepDegrees * progress.value,
                        useCenter = false,
                        topLeft = topLeft,
                        size = arcSize,
                        style = Stroke(width = 4.dp.toPx(), cap = StrokeCap.Round),
                    )
                }
            }
            Text(
                text = text,
                style = MavType.numeralSmall,
                color = palette.ink.copy(alpha = if (fraction == null) 0.72f else 1f),
                modifier = Modifier.padding(bottom = 6.dp),
            )
        }
        // The label is boxed to the gauge's width and centred; without the explicit width it drew
        // past its own column and collided with the next gauge.
        Text(
            text = label,
            style = MavType.sub,
            color = palette.inkSecondary,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            modifier = Modifier.width(78.dp),
        )
    }
}

/**
 * The value against the core's own normal range. The band and marker are plain ink: the card
 * underneath already says whether the number is good, and saying it twice is how a screen shouts.
 */
@Composable
fun MavBaselineBar(
    band: MavBand,
    lowText: String,
    highText: String,
    accessibilitySummary: String,
    modifier: Modifier = Modifier,
    /** The metric this belongs to, so the band is drawn in that metric's step of the hue. */
    family: MavFamily = MavFamily.VITALS,
) {
    val palette = MavTheme.palette
    val hue = family.hue
    // Whether today's value sits inside the wearer's own normal range. This is the whole question
    // the row is asking, so it is answered by the shape rather than left to be inferred from a
    // marker's position against a hairline.
    val inRange = band.markerFraction >= band.lowFraction && band.markerFraction <= band.highFraction
    Column(
        modifier.semantics(mergeDescendants = true) {
            contentDescription = accessibilitySummary
        },
    ) {
        Canvas(
            Modifier
                .fillMaxWidth()
                .height(16.dp),
        ) {
            val centreY = size.height / 2f
            val trackHeight = 6.dp.toPx()
            // The full span, recessed. Thin, because it is context rather than content.
            drawRoundRect(
                color = palette.hairline,
                topLeft = Offset(0f, centreY - trackHeight / 2),
                size = Size(size.width, trackHeight),
                cornerRadius = androidx.compose.ui.geometry.CornerRadius(trackHeight),
            )
            val bandStart = (band.lowFraction * size.width).toFloat()
            val bandEnd = (band.highFraction * size.width).toFloat()
            // The normal range, in the metric's own colour and unmistakably a region rather than a
            // line. The previous version drew this at 3dp in ink-at-30%, which read as a slightly
            // darker part of the track and answered nothing.
            drawRoundRect(
                color = hue.copy(alpha = 0.5f),
                topLeft = Offset(bandStart, centreY - trackHeight / 2),
                size = Size(maxOf(bandEnd - bandStart, trackHeight), trackHeight),
                cornerRadius = androidx.compose.ui.geometry.CornerRadius(trackHeight),
            )
            val markerX = (band.markerFraction * size.width).toFloat()
            // A value outside the range gets a halo, so "unusual" is visible at a glance without
            // the reader comparing two x-positions.
            if (!inRange) {
                drawCircle(
                    palette.ink.copy(alpha = 0.35f),
                    radius = 10.dp.toPx(),
                    center = Offset(markerX, centreY),
                    style = Stroke(width = 2.dp.toPx()),
                )
            }
            drawCircle(palette.surface, radius = 8.5.dp.toPx(), center = Offset(markerX, centreY))
            drawCircle(palette.ink, radius = 6.dp.toPx(), center = Offset(markerX, centreY))
        }
        Row(
            Modifier.fillMaxWidth().padding(top = 7.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(lowText, style = MavType.caption, color = palette.inkSecondary)
            // The band is named, not just drawn. It costs one line to say what the coloured region
            // means.
            Text(
                if (inRange) "in range" else "outside range",
                style = MavType.caption,
                color = palette.inkSecondary,
            )
            Text(highText, style = MavType.caption, color = palette.inkSecondary)
        }
    }
}

@Composable
@Suppress("UNUSED_PARAMETER") // Kept in the cross-platform API; every family is monochrome.
fun MavSparkline(
    values: List<Double>,
    family: MavFamily,
    accessibilitySummary: String,
    modifier: Modifier = Modifier,
) {
    val hue = family.hue
    Canvas(
        modifier
            .fillMaxWidth()
            .height(54.dp)
            .semantics { contentDescription = accessibilitySummary },
    ) {
        if (values.size < 2) return@Canvas
        val lowest = values.min()
        val highest = values.max()
        val span = max(highest - lowest, 0.0001)
        val step = size.width / (values.size - 1)
        val positions = values.mapIndexed { index, value ->
            Offset(
                index * step,
                size.height - ((value - lowest) / span).toFloat() * (size.height - 10f) - 5f,
            )
        }
        drawPath(
            smoothChartPath(positions),
            hue,
            style = Stroke(width = 2.25.dp.toPx(), cap = StrokeCap.Round),
        )
        drawCircle(hue, radius = 3.5.dp.toPx(), center = positions.last())
    }
}

@Composable
fun MavCycleHistoryChart(lengths: List<Int>, accessibilitySummary: String) {
    val palette = MavTheme.palette
    // Keep a stable human scale: auto-zooming 28..29 made one day look like a huge swing.
    val low = 20.0
    val high = max(36.0, (lengths.maxOrNull() ?: 34) + 2.0)
    val span = high - low
    Column(Modifier.semantics { contentDescription = accessibilitySummary }) {
        Row(
            Modifier.fillMaxWidth().height(132.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            lengths.forEach { value ->
                Column(
                    Modifier.weight(1f),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Bottom,
                ) {
                    Text("$value", style = MavType.caption, color = palette.ink)
                    Box(
                        Modifier
                            .width(38.dp)
                            .padding(top = 6.dp)
                            .height(max(((value - low) / span * 104).toFloat(), 8f).dp)
                            .clip(RoundedCornerShape(6.dp))
                            .background(palette.ink),
                    )
                }
            }
        }
        Row(Modifier.fillMaxWidth().padding(top = 10.dp)) {
            lengths.indices.forEach { index ->
                Text(
                    "C${index + 1}",
                    style = MavType.caption,
                    color = palette.inkSecondary,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

/**
 * The scrubbable chart on a metric detail. The normal band is a wash behind the line, so "is this
 * normal for me" is answered by position rather than by a colour.
 */
@Composable
fun MavSeriesChart(
    points: List<Pair<String, Double>>,
    band: Pair<Double, Double>?,
    family: MavFamily,
    accessibilitySummary: String,
    selection: Int?,
    onSelect: (Int) -> Unit,
) {
    val palette = MavTheme.palette
    // Read in composition: `hue` is a @Composable getter and the Canvas draw scope is not one.
    val hue = family.hue
    val values = points.map { it.second } + (band?.let { listOf(it.first, it.second) } ?: emptyList())
    val lowest = (values.minOrNull() ?: 0.0)
    val highest = (values.maxOrNull() ?: 1.0)
    val pad = max((highest - lowest) * 0.15, 0.5)
    val low = lowest - pad
    val high = highest + pad
    val span = max(high - low, 0.0001)

    Canvas(
        Modifier
            .fillMaxWidth()
            .height(204.dp)
            .pointerInput(points.size) {
                val step = if (points.size > 1) size.width.toFloat() / (points.size - 1) else size.width.toFloat()
                detectHorizontalDragGestures { change, _ ->
                    onSelect((change.position.x / step).roundToInt().coerceIn(0, points.size - 1))
                }
            }
            .pointerInput(points.size) {
                val step = if (points.size > 1) size.width.toFloat() / (points.size - 1) else size.width.toFloat()
                detectTapGestures { offset ->
                    onSelect((offset.x / step).roundToInt().coerceIn(0, points.size - 1))
                }
            }
            .semantics { contentDescription = accessibilitySummary },
    ) {
        fun y(value: Double): Float = size.height - ((value - low) / span).toFloat() * size.height
        val step = if (points.size > 1) size.width / (points.size - 1) else size.width

        if (band != null) {
            val top = y(band.second)
            drawRect(
                color = palette.ink.copy(alpha = 0.05f),
                topLeft = Offset(0f, top),
                size = Size(size.width, max(y(band.first) - top, 1f)),
            )
        }

        listOf(0.12f, 0.5f, 0.88f).forEach { fraction ->
            drawLine(
                palette.grid,
                Offset(0f, size.height * fraction),
                Offset(size.width, size.height * fraction),
                strokeWidth = 1f,
            )
        }

        if (points.size < 2) return@Canvas

        val positions = points.mapIndexed { index, point ->
            Offset(index * step, y(point.second))
        }
        val line = smoothChartPath(positions)
        drawPath(line, hue, style = Stroke(width = 2.5.dp.toPx(), cap = StrokeCap.Round))
        if (positions.size <= 12) {
            positions.forEach {
                drawCircle(hue.copy(alpha = 0.72f), radius = 2.dp.toPx(), center = it)
            }
        }

        if (selection != null && selection in points.indices) {
            val x = selection * step
            drawLine(palette.ink.copy(alpha = 0.3f), Offset(x, 0f), Offset(x, size.height), 1f)
            drawCircle(palette.canvas, radius = 6.dp.toPx(), center = Offset(x, y(points[selection].second)))
            drawCircle(palette.ink, radius = 4.5.dp.toPx(), center = Offset(x, y(points[selection].second)))
        }
    }
}

private fun smoothChartPath(points: List<Offset>): Path {
    val path = Path()
    if (points.isEmpty()) return path
    path.moveTo(points.first().x, points.first().y)
    if (points.size < 3) {
        points.drop(1).forEach { path.lineTo(it.x, it.y) }
        return path
    }
    for (index in 0 until points.lastIndex) {
        val p0 = points[max(index - 1, 0)]
        val p1 = points[index]
        val p2 = points[index + 1]
        val p3 = points[minOf(index + 2, points.lastIndex)]
        path.cubicTo(
            p1.x + (p2.x - p0.x) / 6f,
            p1.y + (p2.y - p0.y) / 6f,
            p2.x - (p3.x - p1.x) / 6f,
            p2.y - (p3.y - p1.y) / 6f,
            p2.x,
            p2.y,
        )
    }
    return path
}

/**
 * Time in each heart-rate zone. One row per zone, hardest at the top, one quantity, bar length
 * relative to the biggest zone that week.
 *
 * This replaces a stacked bar that borrowed five unrelated family hues and printed a per-zone
 * "target" beside each. Nothing in the core admits a weekly zone target, so that number was
 * invented, and it is gone.
 */
data class MavZone(val number: Int, val name: String, val bounds: String, val minutes: Int)

@Composable
fun MavZoneLadder(zones: List<MavZone>, accessibilitySummary: String) {
    val palette = MavTheme.palette
    val largest = max(zones.maxOfOrNull { it.minutes } ?: 0, 1)
    Column(
        verticalArrangement = Arrangement.spacedBy(13.dp),
        modifier = Modifier.semantics { contentDescription = accessibilitySummary },
    ) {
        zones.sortedByDescending { it.number }.forEach { zone ->
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .fillMaxWidth()
                    .semantics(mergeDescendants = true) {
                        contentDescription =
                            "Zone ${zone.number}, ${zone.name}, ${zone.bounds}, ${zone.minutes} minutes"
                    },
            ) {
                Column(Modifier.width(104.dp)) {
                    Text(
                        "Zone ${zone.number} · ${zone.name}",
                        style = MavType.sub,
                        color = palette.inkSecondary,
                    )
                    Text(
                        zone.bounds,
                        style = MavType.sub,
                        color = palette.inkSecondary.copy(alpha = 0.85f),
                    )
                }
                Box(
                    Modifier
                        .weight(1f)
                        .padding(horizontal = 12.dp)
                        .height(6.dp)
                        .clip(CircleShape)
                        .background(palette.hairline),
                ) {
                    Box(
                        Modifier
                            .fillMaxWidth(zone.minutes.toFloat() / largest)
                            .height(6.dp)
                            .clip(CircleShape)
                            .background(palette.ink.copy(alpha = 0.3f + 0.14f * zone.number)),
                    )
                }
                Text(
                    "${zone.minutes}m",
                    style = MavType.sub,
                    color = palette.ink,
                    modifier = Modifier.width(44.dp),
                )
            }
        }
    }
}

/** Seven days of load. The selected day is the accent — selection is a state, not a judgement. */
data class MavWeekDay(
    val letter: String,
    val key: String,
    val fraction: Float,
    val minutes: Int = 0,
    val summary: String,
)

@Composable
fun MavWeekStrip(days: List<MavWeekDay>, selected: Int, onSelect: (Int) -> Unit) {
    val hue = MavFamily.EFFORT.hue
    val palette = MavTheme.palette
    Row(
        Modifier.fillMaxWidth().height(126.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        days.forEachIndexed { index, day ->
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Bottom,
                modifier = Modifier
                    .weight(1f)
                    .height(126.dp)
                    .clickable { onSelect(index) }
                    .semantics(mergeDescendants = true) { contentDescription = day.summary },
            ) {
                Text(
                    if (day.minutes > 0) "${day.minutes}" else "",
                    style = MavType.caption,
                    color = if (index == selected) palette.ink else palette.inkSecondary,
                    modifier = Modifier.height(18.dp),
                )
                Box(
                    Modifier
                        .fillMaxWidth()
                        .height(max(day.fraction * 76f, 3f).dp)
                        .clip(RoundedCornerShape(5.dp))
                        .background(
                            // A bar is a data mark, so it is the hue rather than ink. Selection
                            // is carried by weight — the chosen day is the hue at full strength,
                            // the rest are a wash of it.
                            hue.copy(alpha = if (index == selected) 1f else 0.28f),
                        ),
                )
                Text(
                    day.letter,
                    style = MavType.sub,
                    color = if (index == selected) palette.ink else palette.inkSecondary,
                    modifier = Modifier.padding(top = 9.dp),
                )
            }
        }
    }
}
