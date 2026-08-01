package com.sennnen.mav.ui.mav

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MultiChoiceSegmentedButtonRow
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.RadioButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material.icons.Icons
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import uniffi.mav_ffi.DailySnapshotReport
import com.sennnen.mav.data.DailyMetric
import com.sennnen.mav.data.WorkoutRow
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.parseZonePercents
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.delay
import kotlin.math.roundToInt

// Vitals, the metric detail, Workouts and Cycle. The iOS twins are MavVitalsView.swift,
// MavMetricDetailView.swift, MavWorkoutsView.swift and MavCycleView.swift.

// ---------------------------------------------------------------------------------------------
// Vitals
// ---------------------------------------------------------------------------------------------

/**
 * One row per metric, built from the core's availability set rather than a hardcoded list, so a
 * metric no connector can supply is an honest unavailable card and never an empty frame.
 *
 * A row is a button that pushes. It does not expand in place: two disclosure mechanisms on one
 * control is how the old hubs got confusing.
 */
@Composable
fun MavVitalsScreen(
    rows: List<MavMetricRow>,
    usingFixture: Boolean,
    showEcg: Boolean,
    ecgDetail: String,
    onOpenEcg: () -> Unit,
    onOpenMetric: (MavMetric) -> Unit,
) {
    MavTabScroll {
        if (usingFixture) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(9.dp),
                modifier = Modifier.padding(top = 4.dp),
            ) {
                MavBadge("Sample")
                Text(
                    "Fixture data — nothing is connected.",
                    style = MavType.sub,
                    color = MavTheme.palette.inkSecondary,
                )
            }
        }

        if (showEcg) {
            MavSectionHeader("Heart")
            MavTile(
                modifier = Modifier
                    .clickable(onClick = onOpenEcg)
                    .semantics {
                        contentDescription = "ECG. $ecgDetail. Opens ECG capture and history."
                    },
                padded = false,
            ) {
                MavRow(
                    title = "ECG",
                    detail = ecgDetail,
                    trailing = {
                        Icon(MavIcons.chevronRight, contentDescription = null)
                    },
                )
            }
        }

        MavMetricGroup.entries.forEach { group ->
            val groupRows = rows.filter {
                it.metric.group == group &&
                    (it.isAvailable || it.metric.family == MavFamily.CYCLE)
            }
            if (groupRows.isNotEmpty()) {
                if (group.title != "Vitals") MavSectionHeader(group.title)
                groupRows.forEach { row -> MavVitalRow(row, onOpenMetric) }
            }
        }
    }
}

/**
 * The Oura shape, with the one colour rule enforced: the surface carries the verdict, the family
 * tints only the small icon, and the numeral, bar, marker and status word are all plain ink.
 */
@Composable
fun MavVitalRow(row: MavMetricRow, onOpen: (MavMetric) -> Unit) {
    val palette = MavTheme.palette
    when (val state = row.state) {
        is MavMetricState.Unavailable -> {
            if (row.metric.family == MavFamily.CYCLE) {
                MavStatusCard(
                    MavFamily.CYCLE,
                    radius = MavTheme.tileRadius,
                    onClick = { onOpen(row.metric) },
                    scene = Alignment.BottomCenter,
                ) {
                    MavRow(
                        "Cycle",
                        "Period history and cycle day",
                        trailing = {
                            Icon(MavIcons.chevronRight, contentDescription = null)
                        },
                    )
                }
            } else {
                MavUnavailableCard(
                    row.metric.displayName,
                    state.reason,
                    scene = Alignment.BottomCenter,
                )
            }
        }

        is MavMetricState.Value -> {
            val unitSuffix = row.metric.unit?.let { " $it" } ?: ""
            MavStatusCard(
                family = row.metric.family,
                radius = MavTheme.tileRadius,
                onClick = { onOpen(row.metric) },
                // Crop follows from the family rather than the row's position, so a metric keeps
                // the same landscape band across refreshes instead of reshuffling under the reader.
                scene = when (row.metric.family) {
                    MavFamily.CHARGE, MavFamily.HEART -> Alignment.TopCenter
                    MavFamily.REST, MavFamily.ENERGY -> Alignment.Center
                    else -> Alignment.BottomCenter
                },
                modifier = Modifier.semantics(mergeDescendants = true) {
                    contentDescription =
                        "${row.metric.displayName}, ${state.word}, ${state.text}$unitSuffix"
                },
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier
                            .size(24.dp)
                            .clip(RoundedCornerShape(MavTheme.chipRadius))
                            .background(palette.raised),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            MavIcons.familyIcon(row.metric.family),
                            contentDescription = null,
                            tint = row.metric.family.hue,
                            modifier = Modifier.size(13.dp),
                        )
                    }
                    Text(
                        row.metric.displayName,
                        style = MavType.label,
                        color = palette.ink,
                        modifier = Modifier.padding(start = 9.dp).weight(1f),
                    )
                    // The verdict is a word, always. Colour is never the only thing saying it.
                    Text(
                        state.word,
                        style = MavType.caption,
                        color = palette.inkSecondary,
                    )
                    Icon(
                        MavIcons.chevronRight,
                        contentDescription = null,
                        tint = palette.inkSecondary,
                        modifier = Modifier.padding(start = 6.dp).size(20.dp),
                    )
                }

                Row(
                    Modifier.fillMaxWidth().padding(top = 14.dp),
                    verticalAlignment = Alignment.Bottom,
                ) {
                    Row(verticalAlignment = Alignment.Bottom) {
                        Text(state.text, style = MavType.numeralLarge, color = palette.ink)
                        if (row.metric.unit != null) {
                            Text(
                                row.metric.unit,
                                style = MavType.sub,
                                color = palette.inkSecondary,
                                modifier = Modifier.padding(start = 3.dp, bottom = 6.dp),
                            )
                        }
                    }
                    val band = state.band
                    if (band != null) {
                        MavBaselineBar(
                            band = band,
                            lowText = MavMetricMapper.decimal(band.low, 0),
                            highText = MavMetricMapper.decimal(band.high, 0),
                            accessibilitySummary =
                            "${state.text}$unitSuffix. Your normal range is " +
                                "${MavMetricMapper.decimal(band.low, 0)} to " +
                                MavMetricMapper.decimal(band.high, 0),
                            modifier = Modifier.padding(start = 18.dp).weight(1f),
                            family = row.metric.family,
                        )
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Metric detail
// ---------------------------------------------------------------------------------------------

enum class MavRange(val label: String, val days: Int, val spoken: String) {
    WEEK("1W", 7, "One week"),
    MONTH("1M", 30, "One month"),
    QUARTER("3M", 90, "Three months"),
    HALF("6M", 182, "Six months"),
    YEAR("1Y", 365, "One year"),
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MavMetricDetailScreen(
    metric: MavMetric,
    snapshot: DailySnapshotReport?,
    history: List<DailyMetric>,
    liveBpm: Int?,
    connected: Boolean,
    deviceName: String?,
    usingFixture: Boolean,
    onBack: () -> Unit,
) {
    val palette = MavTheme.palette
    var range by remember { mutableStateOf(MavRange.MONTH) }
    var selection by remember { mutableStateOf<Int?>(null) }
    val state = MavMetricMapper.state(metric, snapshot)

    MavDetailScaffold(
        metric.displayName,
        onBack,
        // The same crop the Vitals row used, kept in step with MavVitalRow.
        scene = when (metric.family) {
            MavFamily.CHARGE, MavFamily.HEART -> Alignment.TopCenter
            MavFamily.REST, MavFamily.ENERGY -> Alignment.Center
            else -> Alignment.BottomCenter
        },
    ) {
        when (state) {
            is MavMetricState.Value -> MavStatusCard(metric.family) {
                Text(state.word, style = MavType.caption, color = palette.inkSecondary)
                Text(
                    "${state.text}${metric.unit?.let { " $it" } ?: ""}",
                    style = MavType.numeralXL,
                    color = palette.ink,
                    modifier = Modifier.padding(top = 11.dp),
                )
                val band = state.band
                Text(
                    if (band == null) {
                        "The core has not published a normal range for this metric yet."
                    } else {
                        "Your normal range is ${MavMetricMapper.decimal(band.low, 0)} to " +
                            "${MavMetricMapper.decimal(band.high, 0)} ${metric.unit ?: ""}, " +
                            "measured from your own last seven days."
                    },
                    style = MavType.body,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 11.dp),
                )
            }

            is MavMetricState.Unavailable ->
                MavUnavailableCard(metric.displayName, state.reason)
        }

        if (metric.id == "heart_rate") {
            MavTile {
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column {
                        Text("Live", style = MavType.caption, color = palette.inkSecondary)
                        Row(
                            Modifier.padding(top = 4.dp),
                            verticalAlignment = Alignment.Bottom,
                        ) {
                            Text(liveBpm?.toString() ?: "—", style = MavType.numeralLarge)
                            Text(
                                "bpm",
                                style = MavType.sub,
                                color = palette.inkSecondary,
                                modifier = Modifier.padding(start = 4.dp, bottom = 5.dp),
                            )
                        }
                    }
                    Text(
                        if (connected) "From ${deviceName ?: "your device"}"
                        else "Connect a device for a live reading",
                        style = MavType.sub,
                        color = palette.inkSecondary,
                        modifier = Modifier.padding(start = 20.dp).weight(1f),
                    )
                }
            }
        }

        // A real SegmentedButtonRow: it brings its own traversal order and its own "2 of 5"
        // announcement, neither of which a row of buttons dressed as one would have.
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            MavRange.entries.forEachIndexed { index, option ->
                SegmentedButton(
                    selected = range == option,
                    onClick = { range = option },
                    shape = SegmentedButtonDefaults.itemShape(index, MavRange.entries.size),
                    label = { Text(option.label, style = MavType.caption) },
                    modifier = Modifier.semantics { contentDescription = option.spoken },
                )
            }
        }

        val points = if (usingFixture && state is MavMetricState.Value) {
            if (metric.id == "variability") {
                history.takeLast(range.days).mapNotNull { day ->
                    day.avgHrv?.let { day.day.takeLast(5) to it }
                }
            } else {
                val days = history.takeLast(range.days)
                val shape = MavDebugFixture.scoreHistory.takeLast(days.size)
                val amplitude = maxOf(kotlin.math.abs(state.numeric) * 0.045, 0.12)
                days.zip(shape).map { (day, score) ->
                    day.day.takeLast(5) to (state.numeric + (score - 82.0) * amplitude / 4.0)
                }
            }
        } else {
            history.takeLast(range.days).mapNotNull { day ->
                val value = when (metric.id) {
                    "variability" -> day.avgHrv
                    "heart_rate" -> day.restingHr?.toDouble()
                    else -> null
                }
                value?.let { day.day.takeLast(5) to it }
            }
        }

        if (points.size > 1) {
            MavTile {
                val band = (state as? MavMetricState.Value)?.band?.let { it.low to it.high }
                val selectedPoint = selection?.let { points.getOrNull(it) } ?: points.last()
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.Bottom,
                ) {
                    Text(
                        MavMetricMapper.decimal(
                            selectedPoint.second,
                            if (metric.unit == "°C") 1 else 0,
                        ),
                        style = MavType.numeralMedium,
                        color = palette.ink,
                    )
                    metric.unit?.let {
                        Text(
                            it,
                            style = MavType.sub,
                            color = palette.inkSecondary,
                            modifier = Modifier.padding(start = 5.dp, bottom = 4.dp),
                        )
                    }
                    Spacer(Modifier.weight(1f))
                    Text(selectedPoint.first, style = MavType.sub, color = palette.inkSecondary)
                }
                MavSeriesChart(
                    points = points,
                    band = band,
                    family = metric.family,
                    accessibilitySummary =
                    "${metric.displayName} over ${range.spoken}. ${points.size} days, from " +
                        "${MavMetricMapper.decimal(points.minOf { it.second }, 0)} to " +
                        "${MavMetricMapper.decimal(points.maxOf { it.second }, 0)}, latest " +
                        MavMetricMapper.decimal(points.last().second, 0),
                    selection = selection,
                    onSelect = { selection = it },
                )
                Row(
                    Modifier.fillMaxWidth().padding(top = 8.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(points.first().first, style = MavType.sub, color = palette.inkSecondary)
                    Text(points.last().first, style = MavType.sub, color = palette.inkSecondary)
                }
            }
        } else {
            MavTile {
                Text("Not enough history", style = MavType.title, color = palette.ink)
                Text(
                    "A chart needs at least two scored days.",
                    style = MavType.body,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 6.dp),
                )
            }
        }

        val hrv = snapshot?.hrv
        if (hrv != null && metric.id == "variability") {
            MavSectionHeader("Inside the number")
            MavTile(padded = false) {
                MavRow("RMSSD") { MavValueText("${MavMetricMapper.decimal(hrv.rmssdMs, 1)} ms") }
                MavDivider()
                MavRow("SDNN") { MavValueText("${MavMetricMapper.decimal(hrv.sdnnMs, 1)} ms") }
                MavDivider()
                MavRow("Mean interval") {
                    MavValueText("${MavMetricMapper.decimal(hrv.meanIntervalMs, 1)} ms")
                }
                MavDivider()
                MavRow("pNN50") { MavValueText("${MavMetricMapper.decimal(hrv.pnn50Percent, 1)}%") }
                MavDivider()
                MavRow("Intervals used") {
                    MavValueText("${hrv.intervalCount}, ${hrv.excludedCount} excluded")
                }
            }
        } else if (snapshot != null && metric.id == "heart_rate") {
            MavSectionHeader("Inside the number")
            MavTile(padded = false) {
                MavRow("Current") {
                    MavValueText(liveBpm?.let { "$it bpm" } ?: "No live reading")
                }
                MavDivider()
                MavRow("Day average") {
                    MavValueText(
                        snapshot.meanBpm?.let {
                            "${MavMetricMapper.decimal(it, 0)} bpm"
                        } ?: "No day average",
                    )
                }
                MavDivider()
                MavRow("Samples") {
                    MavValueText(
                        "${snapshot.hrSampleCount}, ${snapshot.hrExcludedCount} excluded",
                    )
                }
            }
        }

        MavSectionHeader("Where this number came from")
        MavTile {
            if (snapshot == null) {
                Text("No snapshot loaded.", style = MavType.body, color = palette.inkSecondary)
            } else {
                MavProvenanceLine("Day", snapshot.day)
                if (snapshot.algorithms.isNotEmpty()) {
                    MavProvenanceLine("Algorithms", snapshot.algorithms.joinToString(", "))
                }
                snapshot.hrv?.let { MavProvenanceLine("Interval label", it.label) }
                MavProvenanceLine(
                    "Heart-rate samples",
                    "${snapshot.hrSampleCount} used, ${snapshot.hrExcludedCount} excluded",
                )
                MavProvenanceLine("Snapshot", snapshot.snapshotHash)
            }
        }
    }
}

@Composable
private fun MavValueText(text: String) {
    Text(text, style = MavType.label, color = MavTheme.palette.inkSecondary)
}

@Composable
private fun MavProvenanceLine(key: String, value: String) {
    val palette = MavTheme.palette
    Row(
        Modifier
            .fillMaxWidth()
            .padding(vertical = 3.dp)
            .semantics(mergeDescendants = true) { contentDescription = "$key: $value" },
    ) {
        Text(key, style = MavType.sub, color = palette.inkSecondary, modifier = Modifier.width(132.dp))
        Text(value, style = MavType.sub, color = palette.ink)
    }
}

// ---------------------------------------------------------------------------------------------
// Workouts
// ---------------------------------------------------------------------------------------------

@Composable
fun MavWorkoutsScreen(
    workouts: List<WorkoutRow>,
    activeWorkout: AppViewModel.ActiveWorkout?,
    onStart: () -> Unit,
    onOpenActive: () -> Unit,
) {
    val palette = MavTheme.palette
    var selectedDay by remember { mutableIntStateOf(6) }
    val totalMinutes = workouts.sumOf { (it.durationS ?: (it.endTs - it.startTs).toDouble()) / 60.0 }
    val days = weekDays(workouts)
    val selectedKey = days.getOrNull(selectedDay)?.key
    val selectedWorkouts = workouts.filter { workoutDay(it) == selectedKey }
    val loadPoints = workouts
        .filter { it.strain != null }
        .sortedBy { it.startTs }
        .takeLast(10)
        .map {
            java.time.format.DateTimeFormatter.ofPattern("d MMM").format(
                Instant.ofEpochSecond(it.startTs).atZone(ZoneId.systemDefault()),
            ) to (it.strain ?: 0.0)
        }
    var loadSelection by remember { mutableStateOf<Int?>(null) }
    var selectedWorkout by remember { mutableStateOf<WorkoutRow?>(null) }

    MavTabScroll {
        Box(
            Modifier
                .fillMaxWidth()
                .defaultMinSize(minHeight = 148.dp)
                .clip(RoundedCornerShape(MavTheme.cardRadius)),
        ) {
            MavScene(Modifier.matchParentSize(), Alignment.BottomCenter)
            Column(Modifier.align(Alignment.BottomStart).padding(18.dp)) {
                Text(
                    "This week",
                    style = MavType.caption,
                    color = Color.White.copy(alpha = 0.88f),
                )
                Text(
                    if (workouts.isEmpty()) {
                        "No sessions yet"
                    } else {
                        "${workouts.size} session${if (workouts.size == 1) "" else "s"}"
                    },
                    style = MavType.title,
                    color = Color.White,
                    modifier = Modifier.padding(top = 5.dp),
                )
                if (workouts.isNotEmpty()) {
                    Row(
                        Modifier.padding(top = 14.dp),
                        horizontalArrangement = Arrangement.spacedBy(26.dp),
                    ) {
                        MavStatBlock("Time", durationLabel(totalMinutes), onPhoto = true)
                        MavStatBlock("Sessions", "${workouts.size}", onPhoto = true)
                    }
                }
            }
        }

        if (activeWorkout != null) {
            MavStatusCard(MavFamily.EFFORT, radius = MavTheme.tileRadius, onClick = onOpenActive) {
                MavRow(
                    "Workout in progress",
                    "View elapsed time and session controls",
                    trailing = {
                        Icon(MavIcons.chevronRight, contentDescription = null)
                    },
                )
            }
        }

        MavPrimaryButton("Start workout") { onStart() }

        if (workouts.isNotEmpty()) {
            MavSectionHeader("Weekly activity")
            MavTile {
                Text(
                    "Minutes",
                    style = MavType.caption,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(bottom = 4.dp),
                )
                MavWeekStrip(
                    days = days,
                    selected = selectedDay,
                    onSelect = { selectedDay = it },
                )
            }

            if (loadPoints.size > 1) {
                MavSectionHeader("Training load")
                MavTile {
                    val selected = loadSelection?.let { loadPoints.getOrNull(it) } ?: loadPoints.last()
                    Row(
                        Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.Bottom,
                    ) {
                        Text(
                            workoutEffortText(selected.second),
                            style = MavType.numeralMedium,
                            color = palette.ink,
                        )
                        Text(
                            "effort",
                            style = MavType.sub,
                            color = palette.inkSecondary,
                            modifier = Modifier.padding(start = 5.dp, bottom = 4.dp),
                        )
                        Spacer(Modifier.weight(1f))
                        Text(selected.first, style = MavType.sub, color = palette.inkSecondary)
                    }
                    MavSeriesChart(
                        points = loadPoints,
                        band = null,
                        family = MavFamily.EFFORT,
                        accessibilitySummary = "Effort over ${loadPoints.size} workouts, from " +
                            "${workoutEffortText(loadPoints.minOf { it.second })} to " +
                            workoutEffortText(loadPoints.maxOf { it.second }),
                        selection = loadSelection,
                        onSelect = { loadSelection = it },
                    )
                    Row(
                        Modifier.fillMaxWidth().padding(top = 8.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text(loadPoints.first().first, style = MavType.sub, color = palette.inkSecondary)
                        Text(loadPoints.last().first, style = MavType.sub, color = palette.inkSecondary)
                    }
                }
            }
        }

        MavSectionHeader("Sessions")
        if (workouts.isEmpty()) {
            Text(
                "Nothing here yet",
                style = MavType.title,
                color = palette.ink,
                modifier = Modifier.padding(top = 4.dp),
            )
            Text(
                "Start a workout above, or connect a source that provides sessions.",
                style = MavType.body,
                color = palette.inkSecondary,
            )
        } else if (selectedWorkouts.isEmpty()) {
            Text(
                "No sessions on this day.",
                style = MavType.body,
                color = palette.inkSecondary,
                modifier = Modifier.padding(vertical = 8.dp),
            )
        } else {
            MavTile(padded = false) {
                selectedWorkouts.forEachIndexed { index, workout ->
                    if (index > 0) MavDivider()
                    MavRow(
                        workout.sport,
                        "${durationLabel((workout.durationS ?: (workout.endTs - workout.startTs).toDouble()) / 60.0)}" +
                            (workout.avgHr?.let { " · $it avg bpm" } ?: ""),
                        modifier = Modifier.clickable { selectedWorkout = workout },
                        trailing = { Icon(MavIcons.chevronRight, contentDescription = null) },
                    )
                }
            }
        }
    }

    selectedWorkout?.let { workout ->
        MavWorkoutDetailSheet(workout) { selectedWorkout = null }
    }
}

private fun durationLabel(minutes: Double): String {
    val total = minutes.toInt()
    return if (total >= 60) "${total / 60}h ${total % 60}m" else "${total}m"
}

private fun workoutEffortText(value: Double): String =
    MavMetricMapper.decimal(value, if (value < 22) 1 else 0)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MavWorkoutDetailSheet(workout: WorkoutRow, onDismiss: () -> Unit) {
    val palette = MavTheme.palette
    val minutes = ((workout.durationS ?: (workout.endTs - workout.startTs).toDouble()) / 60.0)
        .roundToInt()
    val zones = parseZonePercents(workout.zonesJSON)
    val zoneNames = listOf("Easy", "Base", "Tempo", "Threshold", "Maximum")
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(bottom = 32.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                workout.sport,
                style = MavType.display,
                color = palette.ink,
            )
            Text(
                DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
                    .format(Date(workout.startTs * 1_000)),
                style = MavType.sub,
                color = palette.inkSecondary,
            )
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                MavWorkoutDetailStat("Duration", "$minutes min", Modifier.weight(1f))
                MavWorkoutDetailStat(
                    "Effort",
                    workout.strain?.let(::workoutEffortText) ?: "—",
                    Modifier.weight(1f),
                )
            }
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                MavWorkoutDetailStat(
                    "Average HR",
                    workout.avgHr?.let { "$it bpm" } ?: "—",
                    Modifier.weight(1f),
                )
                MavWorkoutDetailStat(
                    "Maximum HR",
                    workout.maxHr?.let { "$it bpm" } ?: "—",
                    Modifier.weight(1f),
                )
            }

            zones?.let {
                MavSectionHeader("Heart-rate distribution")
                MavTile {
                    it.indices.reversed().forEach { index ->
                        Row(
                            Modifier.fillMaxWidth().padding(vertical = 7.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                        ) {
                            Text("Z${index + 1}", style = MavType.label, modifier = Modifier.width(28.dp))
                            Text(
                                zoneNames[index],
                                style = MavType.sub,
                                color = palette.inkSecondary,
                                modifier = Modifier.width(72.dp),
                            )
                            Box(
                                Modifier
                                    .weight(1f)
                                    .height(8.dp)
                                    .clip(CircleShape)
                                    .background(palette.hairline),
                            ) {
                                Box(
                                    Modifier
                                        .fillMaxWidth((it[index] / 100).toFloat().coerceIn(0f, 1f))
                                        .height(8.dp)
                                        .clip(CircleShape)
                                        .background(palette.ink.copy(alpha = 0.42f + index * 0.1f)),
                                )
                            }
                            Text(
                                "${it[index].roundToInt()}%",
                                style = MavType.sub,
                                modifier = Modifier.width(38.dp),
                            )
                        }
                    }
                }
            }
            Text(
                "Source: ${workout.source}",
                style = MavType.sub,
                color = palette.inkSecondary,
                modifier = Modifier.padding(vertical = 8.dp),
            )
        }
    }
}

@Composable
private fun MavWorkoutDetailStat(label: String, value: String, modifier: Modifier = Modifier) {
    MavTile(modifier) {
        Text(label, style = MavType.caption, color = MavTheme.palette.inkSecondary)
        Text(value, style = MavType.numeralSmall, modifier = Modifier.padding(top = 5.dp))
    }
}

@Composable
private fun MavStatBlock(key: String, value: String, onPhoto: Boolean = false) {
    val palette = MavTheme.palette
    Column(
        Modifier.semantics(mergeDescendants = true) {
            contentDescription = "$key, ${if (value == "—") "no value" else value}"
        },
    ) {
        Text(
            key,
            style = MavType.caption,
            color = if (onPhoto) Color.White.copy(alpha = 0.82f) else palette.inkSecondary,
        )
        Text(value, style = MavType.numeralMedium, color = if (onPhoto) Color.White else palette.ink)
    }
}

private fun weekDays(workouts: List<WorkoutRow>): List<MavWeekDay> {
    val today = LocalDate.now()
    val peak = workouts
        .map { (it.durationS ?: (it.endTs - it.startTs).toDouble()) / 60.0 }
        .maxOrNull()
        ?.coerceAtLeast(1.0)
        ?: 1.0
    return (6 downTo 0).map { back ->
        val date = today.minusDays(back.toLong())
        val minutes = workouts
            .filter { workoutDay(it) == date.toString() }
            .sumOf { (it.durationS ?: (it.endTs - it.startTs).toDouble()) / 60.0 }
        MavWeekDay(
            letter = date.dayOfWeek.name.take(1),
            key = date.toString(),
            fraction = (minutes / peak).toFloat().coerceIn(0f, 1f),
            minutes = minutes.roundToInt(),
            summary = "${date.dayOfWeek.name.lowercase().replaceFirstChar { it.uppercase() }}, " +
                if (minutes > 0) "${minutes.toInt()} minutes" else "nothing recorded",
        )
    }
}

private fun workoutDay(workout: WorkoutRow): String =
    Instant.ofEpochSecond(workout.startTs)
        .atZone(ZoneId.systemDefault())
        .toLocalDate()
        .toString()

@Composable
fun MavWorkoutStartScreen(
    onConfigure: (MavSport) -> Unit,
    onStrength: () -> Unit,
    onBack: () -> Unit,
) {
    MavDetailScaffold("Start workout", onBack) {
        Text(
            "Choose an activity. The next screen sets how it ends.",
            style = MavType.body,
            color = MavTheme.palette.inkSecondary,
            modifier = Modifier.padding(vertical = 14.dp),
        )
        MavSportCatalog.categories.forEach { category ->
            MavSectionHeader(category.title)
            MavTile(padded = false) {
                category.sports.forEachIndexed { index, sport ->
                    if (index > 0) MavDivider()
                    MavRow(
                        sport.name,
                        sport.detail,
                        // Strength is reached here, inside the sport catalogue, rather than from a
                        // button of its own on the tab. It also skips the confirm screen entirely:
                        // a lifting session has no end condition and no zone target, so there
                        // would be nothing on it to decide.
                        modifier = Modifier.clickable {
                            if (sport.isStrength) onStrength() else onConfigure(sport)
                        },
                        trailing = {
                            Icon(MavIcons.chevronRight, contentDescription = null)
                        },
                    )
                }
            }
        }
    }
}

/**
 * The running session. The iOS twin is `MavLiveWorkoutView` and the two show the same things: the
 * elapsed clock, live heart rate, live effort, and one destructive way out.
 */
@Composable
fun MavLiveWorkoutScreen(
    active: AppViewModel.ActiveWorkout?,
    bpm: Int?,
    connected: Boolean,
    onStop: () -> Unit,
    onBack: () -> Unit,
) {
    val palette = MavTheme.palette
    var now by remember { mutableStateOf(System.currentTimeMillis()) }
    LaunchedEffect(active?.startMs) {
        while (active != null) {
            now = System.currentTimeMillis()
            delay(1_000)
        }
    }
    val elapsed = ((now - (active?.startMs ?: now)) / 1_000).coerceAtLeast(0)
    // The configuration the confirm screen persisted for this sport a moment ago. Read rather than
    // carried on the session, so a relaunch mid-workout recovers the same goal.
    val context = LocalContext.current
    val config = remember(active?.sport) {
        MavWorkoutPrefs(context).config(active?.sport ?: "")
    }
    MavDetailScaffold(active?.sport ?: "Live workout", onBack) {
        MavTile {
            Text("Elapsed", style = MavType.caption, color = palette.inkSecondary)
            Text(
                formatElapsed(elapsed),
                style = MavType.numeralXL,
                color = palette.ink,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 10.dp)
                    .semantics { contentDescription = "Elapsed time, ${spokenElapsed(elapsed)}" },
            )
        }

        if (config.goal.isActive) {
            MavSectionHeader("Goal")
            MavGoalCard(config.goal, elapsed.toInt())
        }

        MavSectionHeader("Now")
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(MavTheme.cardSpacing),
        ) {
            MavLiveStat("Heart rate", bpm?.toString() ?: "—", "bpm", Modifier.weight(1f))
            MavLiveStat(
                "Effort",
                active?.liveStrain?.let { workoutEffortText(it) } ?: "—",
                null,
                Modifier.weight(1f),
            )
        }

        // A strap that is not streaming is stated rather than shown as a dash and left ambiguous.
        if (!connected) {
            Text(
                "No strap is streaming, so heart rate and effort stay empty. The elapsed clock " +
                    "keeps running and the session still records.",
                style = MavType.sub,
                color = palette.inkSecondary,
                modifier = Modifier.padding(top = 2.dp),
            )
        }

        Spacer(Modifier.height(6.dp))
        MavWideButton(
            "End workout",
            modifier = Modifier.fillMaxWidth(),
            destructive = true,
            enabled = active != null,
            onClick = onStop,
        )
    }
}

/**
 * Progress toward the end condition.
 *
 * Only a time goal can be answered today: nothing records distance or energy live yet, and a
 * progress bar sitting at zero for an hour is worse than saying so. When the source arrives this
 * branch collapses — `MavMilestones.progress` already handles all three kinds.
 */
@Composable
private fun MavGoalCard(goal: MavGoal, elapsedSec: Int) {
    val palette = MavTheme.palette
    val headline = when (goal.kind) {
        MavGoalKind.NONE -> ""
        MavGoalKind.TIME -> "${goal.value.toInt()} min"
        MavGoalKind.DISTANCE -> "${goalDisplayText(goal, isImperial = false)} km"
        MavGoalKind.CALORIES -> "${goal.value.toInt()} kcal"
    }
    val fraction = if (goal.kind == MavGoalKind.TIME) {
        MavMilestones.progress(goal, elapsedSec, 0.0, 0.0)
    } else {
        null
    }

    MavTile {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
            Text(headline, style = MavType.label, color = palette.ink, modifier = Modifier.weight(1f))
            if (fraction != null) {
                Text(
                    "${(fraction * 100).roundToInt()}%",
                    style = MavType.sub,
                    color = palette.inkSecondary,
                )
            }
        }
        if (fraction != null) {
            val percent = (fraction * 100).roundToInt()
            LinearProgressIndicator(
                progress = { fraction.toFloat() },
                color = MavFamily.EFFORT.hue,
                trackColor = palette.hairline,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 10.dp)
                    .height(8.dp)
                    .semantics { contentDescription = "$headline, $percent percent complete" },
            )
        } else {
            Text(
                when (goal.kind) {
                    MavGoalKind.DISTANCE ->
                        "No source is recording distance yet, so this goal cannot be tracked live."
                    MavGoalKind.CALORIES ->
                        "No source is recording energy yet, so this goal cannot be tracked live."
                    else -> ""
                },
                style = MavType.sub,
                color = palette.inkSecondary,
                modifier = Modifier.padding(top = 8.dp),
            )
        }
    }
}

@Composable
private fun MavLiveStat(label: String, value: String, unit: String?, modifier: Modifier = Modifier) {
    val palette = MavTheme.palette
    MavTile(modifier) {
        Text(label, style = MavType.caption, color = palette.inkSecondary)
        Row(Modifier.padding(top = 5.dp), verticalAlignment = Alignment.Bottom) {
            Text(value, style = MavType.numeralMedium, color = palette.ink)
            if (unit != null) {
                Text(
                    unit,
                    style = MavType.sub,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(start = 3.dp, bottom = 4.dp),
                )
            }
        }
    }
}

/**
 * Hours appear once there are any. The previous formatter was `mm:ss` with no rollover, so a
 * ninety-minute session read "90:00".
 */
internal fun formatElapsed(seconds: Long): String = if (seconds >= 3_600) {
    "%d:%02d:%02d".format(seconds / 3_600, (seconds % 3_600) / 60, seconds % 60)
} else {
    "%02d:%02d".format(seconds / 60, seconds % 60)
}

/** TalkBack reads a duration, not a punctuation pattern. */
internal fun spokenElapsed(seconds: Long): String {
    val hours = seconds / 3_600
    val minutes = (seconds % 3_600) / 60
    val parts = buildList {
        if (hours > 0) add("$hours hour${if (hours == 1L) "" else "s"}")
        if (minutes > 0) add("$minutes minute${if (minutes == 1L) "" else "s"}")
        add("${seconds % 60} second${if (seconds % 60 == 1L) "" else "s"}")
    }
    return parts.joinToString(" ")
}

// ---------------------------------------------------------------------------------------------
// Cycle
// ---------------------------------------------------------------------------------------------

@Composable
fun MavCycleScreen(
    starts: List<String>,
    onLogToday: () -> Unit,
    onRemove: (String) -> Unit,
    onBack: () -> Unit,
) {
    val palette = MavTheme.palette
    val today = LocalDate.now().toString()
    val cycleDay = MavCycle.cycleDay(starts, today)
    val lengths = MavCycle.completedLengths(starts)

    MavDetailScaffold("Cycle", onBack) {
        MavStatusCard(MavFamily.CYCLE) {
            Text("CYCLE", style = MavType.eyebrow, color = palette.inkSecondary)
            if (cycleDay != null) {
                Row(verticalAlignment = Alignment.Bottom, modifier = Modifier.padding(top = 6.dp)) {
                    Text("$cycleDay", style = MavType.numeralXL, color = palette.ink)
                    Text(
                        "CYCLE DAY",
                        style = MavType.eyebrow,
                        color = palette.inkSecondary,
                        modifier = Modifier.padding(start = 11.dp, bottom = 8.dp),
                    )
                }
                Text(
                    "Counted from the period start you logged on " +
                        "${starts.lastOrNull { it <= today } ?: "—"}.",
                    style = MavType.body,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 14.dp),
                )
            } else {
                Text("Nothing logged yet", style = MavType.title, color = palette.ink)
                Text(
                    "Log the first day of a period and everything on this screen starts counting " +
                        "from it.",
                    style = MavType.body,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 12.dp),
                )
            }
            MavWideButton(
                title = if (starts.contains(today)) "Logged for today" else "Log a period start today",
                modifier = Modifier.fillMaxWidth().padding(top = 15.dp),
                onClick = onLogToday,
            )
        }

        MavSectionHeader("This cycle")
        val range = MavCycle.nextPeriodRange(starts)
        if (range != null) {
            MavTile {
                Text("Next period", style = MavType.title, color = palette.ink)
                Text(
                    "${range.first} – ${range.second}",
                    style = MavType.numeralSmall,
                    color = palette.ink,
                    modifier = Modifier.padding(top = 7.dp),
                )
                Text(
                    "A range, not a date — from the shortest and longest of your last " +
                        "${minOf(lengths.size, 6)} cycles.",
                    style = MavType.sub,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 7.dp),
                )
            }
        } else {
            val needed = MavCycle.cyclesNeeded(starts) ?: 3
            MavUnavailableCard(
                "Next period",
                "Needs $needed more logged cycle${if (needed == 1) "" else "s"}. Two points is not " +
                    "a pattern, so there is no estimate rather than a bad one.",
            )
        }

        MavSectionHeader("History")
        if (lengths.isEmpty()) {
            MavUnavailableCard(
                "Cycle lengths",
                "A length needs two logged starts. Nothing to chart yet.",
            )
        } else {
            MavTile {
                MavCycleHistoryChart(
                    lengths,
                    "${lengths.size} cycle lengths, from ${lengths.min()} to ${lengths.max()} days",
                )
                Text(
                    "Median ${MavCycle.medianLength(starts) ?: "—"} days · range " +
                        "${lengths.min()}–${lengths.max()}.",
                    style = MavType.sub,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 12.dp),
                )
            }
        }

        MavSectionHeader("Logged starts")
        if (starts.isEmpty()) {
            MavTile {
                Text("No period starts logged.", style = MavType.body, color = palette.inkSecondary)
            }
        } else {
            MavTile(padded = false) {
                starts.reversed().forEachIndexed { index, start ->
                    if (index > 0) MavDivider()
                    MavRow(start) {
                        Text(
                            "Remove",
                            style = MavType.label,
                            color = destructiveInk(),
                            modifier = Modifier
                                .clickable { onRemove(start) }
                                .mavTarget()
                                .padding(8.dp)
                                .semantics {
                                    contentDescription = "Remove the period start logged on $start"
                                },
                        )
                    }
                }
            }
        }

        Text(
            MavCycleLog.DISCLAIMER,
            style = MavType.sub,
            color = palette.inkSecondary,
            modifier = Modifier.padding(horizontal = 4.dp, vertical = 10.dp),
        )
    }
}
