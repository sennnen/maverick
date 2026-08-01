package com.sennnen.mav.ui.mav

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.sennnen.mav.data.WorkoutRow
import uniffi.mav_ffi.DailySnapshotReport
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

// Today — the narrative tab. It opens with the score rail, because "give me the numbers" deserves
// half a second, and everything under it is prose. The iOS twin is UI/MavTodayView.swift.
//
// There is deliberately no live heart rate here. It belongs inside the Heart Rate vital; it was on
// the old Today because the old shell had nowhere else to put it, which is not a reason.

@Composable
fun MavTodayScreen(
    rows: List<MavMetricRow>,
    snapshot: DailySnapshotReport?,
    syncNote: String?,
    workouts: List<WorkoutRow>,
    usingFixture: Boolean,
    dayKey: String,
    onOpenMetric: (MavMetric) -> Unit,
    onOpenReports: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val narrative = MavStubNarrativeProvider
    MavTabScroll {
        MavScoreRail(MavMetricMapper.rail(rows), onOpenMetric)

        MavNarrativeHero(narrative.daily(dayKey, rows))

        MavSectionHeader("Your day")
        MavDayTimeline(snapshot, syncNote, workouts, usingFixture)

        MavSectionHeader("Discoveries")
        MavTile {
            MavTrendLine(
                "Resilience",
                "3 months",
                MavFamily.CHARGE,
                narrative.trend("resilience", rows),
            )
            MavDivider()
            MavTrendLine(
                "Training load",
                "8 weeks",
                MavFamily.HEART,
                narrative.trend("cardio_load", rows),
            )
        }

        MavSectionHeader("More")
        MavTile(padded = false) {
            MavNavRow("Weekly report", "Every day the core has scored", onOpenReports)
            MavDivider()
            MavNavRow("Diagnostics", "What the core recorded, and any errors", onOpenDiagnostics)
        }
    }
}

/**
 * A horizontally scrolling row of open-bottom arcs. An unavailable score is a dashed arc and an em
 * dash — never a zero, because a zero is a claim about the day.
 */
@Composable
fun MavScoreRail(items: List<MavRailItem>, onOpen: (MavMetric) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            // More content than the viewport plus this quiet trailing breathing room leaves the
            // next gauge partially visible. That is the scroll affordance; no arrows or labels.
            .padding(top = 2.dp, bottom = 2.dp, end = 42.dp),
        horizontalArrangement = Arrangement.spacedBy(MavTheme.railGap),
    ) {
        items.forEach { item ->
            MavArcGauge(
                text = item.text,
                label = item.metric.shortName,
                fraction = item.fraction,
                family = item.metric.family,
                accessibilitySummary = if (item.fraction == null) {
                    "${item.metric.displayName}, no value today"
                } else {
                    "${item.metric.displayName}, ${item.text}${item.metric.unit?.let { " $it" } ?: ""}"
                },
                onClick = { onOpen(item.metric) },
            )
        }
    }
}

@Composable
fun MavNarrativeHero(state: MavNarrativeState) {
    val shape = RoundedCornerShape(MavTheme.cardRadius)
    Box(
        Modifier
            .fillMaxWidth()
            .defaultMinSize(minHeight = 190.dp)
            .clip(shape),
    ) {
        MavScene(Modifier.matchParentSize(), Alignment.TopCenter)

        Column(Modifier.align(Alignment.BottomStart).padding(18.dp)) {
            Text(
                "Daily insight",
                style = MavType.caption,
                color = androidx.compose.ui.graphics.Color.White.copy(alpha = 0.88f),
            )
            val headline = state.headline ?: "Nothing written yet"
            Text(
                headline,
                style = MavType.title.copy(
                    fontSize = 23.sp,
                    lineHeight = 28.sp,
                    fontWeight = FontWeight.Normal,
                ),
                color = androidx.compose.ui.graphics.Color.White,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(top = 5.dp),
            )
            val body = when (state) {
                is MavNarrativeState.Unavailable -> state.reason
                else -> state.bodyText.orEmpty()
            }
            if (body.isNotEmpty()) {
                Text(
                    body,
                    style = MavType.body,
                    color = androidx.compose.ui.graphics.Color.White.copy(alpha = 0.9f),
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(top = 6.dp),
                )
            }
        }

        if (state.isSample) {
            Box(Modifier.align(Alignment.TopEnd).padding(12.dp)) { MavBadge("Preview") }
        }
    }
}

@Composable
fun MavTrendLine(
    title: String,
    window: String,
    family: MavFamily,
    state: MavNarrativeState,
) {
    val palette = MavTheme.palette
    Column(Modifier.padding(vertical = 2.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(
                MavIcons.familyIcon(family),
                contentDescription = null,
                tint = family.hue,
                modifier = Modifier.size(20.dp),
            )
            Text(title, style = MavType.label, modifier = Modifier.padding(start = 10.dp).weight(1f))
            Text(window, style = MavType.caption, color = palette.inkSecondary)
        }
        Text(
            state.headline ?: "Not enough history yet",
            color = palette.ink,
            style = MavType.body,
            modifier = Modifier.padding(top = 7.dp, bottom = 15.dp),
        )
    }
}

/**
 * What actually happened, in the order it happened. Every row is something the core recorded; an
 * empty day says so rather than showing an invented morning.
 */
@Composable
fun MavDayTimeline(
    snapshot: DailySnapshotReport?,
    syncNote: String?,
    workouts: List<WorkoutRow>,
    usingFixture: Boolean,
) {
    val palette = MavTheme.palette
    val entries = buildList {
        if (usingFixture) {
            add(MavTimelineEntry("Sleep", "7h 42m · 91% efficiency", "06:42", MavFamily.REST))
            add(MavTimelineEntry("Recovery", "82 · overnight signals stayed in range", "07:05", MavFamily.CHARGE))
        } else if (snapshot != null) {
            snapshot.hrv?.let { hrv ->
                add(
                    MavTimelineEntry(
                        "Overnight variability",
                        "${MavMetricMapper.decimal(hrv.rmssdMs, 0)} ms · " +
                            "${hrv.intervalCount} intervals",
                        "Overnight",
                        MavFamily.VITALS,
                    ),
                )
            }
            if (snapshot.hrSampleCount > 0u) {
                val quality = if (snapshot.hrExcludedCount > 0u) {
                    " · ${snapshot.hrExcludedCount} omitted after quality checks"
                } else {
                    ""
                }
                add(
                    MavTimelineEntry(
                        "Heart rate",
                        "${snapshot.hrSampleCount} readings recorded$quality",
                        "All day",
                        MavFamily.HEART,
                    ),
                )
            }
        }
        workouts.sortedBy { it.startTs }.forEach { workout ->
            val time = Instant.ofEpochSecond(workout.startTs)
                .atZone(ZoneId.systemDefault())
                .toLocalTime()
                .format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
            val minutes = ((workout.durationS ?: (workout.endTs - workout.startTs).toDouble()) / 60)
                .toInt()
            add(
                MavTimelineEntry(
                    workout.sport,
                    "$minutes min${workout.avgHr?.let { " · $it avg bpm" } ?: ""}",
                    time,
                    MavFamily.EFFORT,
                ),
            )
        }
        if (usingFixture) {
            add(MavTimelineEntry("Journal", "Late caffeine · marked for comparison", "20:45", MavFamily.ENERGY))
        }
        if (syncNote != null) {
            add(MavTimelineEntry("Sync", syncNote, "Now", MavFamily.VITALS))
        }
    }

    Column {
        if (entries.isEmpty()) {
            Text(
                "Nothing recorded for this day yet.",
                style = MavType.body,
                color = palette.inkSecondary,
            )
        } else {
            entries.forEachIndexed { index, entry ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .defaultMinSize(minHeight = 72.dp)
                        .semantics(mergeDescendants = true) {
                            contentDescription = "${entry.period}. ${entry.title}. ${entry.detail}"
                        },
                    verticalAlignment = Alignment.Top,
                ) {
                    Text(
                        entry.period,
                        style = MavType.caption,
                        color = palette.inkSecondary,
                        modifier = Modifier.width(64.dp).padding(top = 12.dp),
                    )
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        modifier = Modifier.width(24.dp),
                    ) {
                        Box(
                            Modifier.width(1.dp).height(10.dp)
                                .background(if (index == 0) Color.Transparent else palette.hairline),
                        )
                        Box(
                            Modifier.size(10.dp).background(entry.family.hue, CircleShape),
                        )
                        Box(
                            Modifier.width(1.dp).height(52.dp)
                                .background(
                                    if (index == entries.lastIndex) Color.Transparent
                                    else palette.hairline,
                                ),
                        )
                    }
                    Column(Modifier.padding(start = 12.dp, top = 8.dp).weight(1f)) {
                        Text(entry.title, style = MavType.label, color = palette.ink)
                        Text(
                            entry.detail,
                            style = MavType.sub,
                            color = palette.inkSecondary,
                            modifier = Modifier.padding(top = 4.dp),
                        )
                    }
                }
            }
        }
    }
}

private data class MavTimelineEntry(
    val title: String,
    val detail: String,
    val period: String,
    val family: MavFamily,
)
