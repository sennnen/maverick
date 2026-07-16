package com.sennnen.mav.ui.aura

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.DirectionsBike
import androidx.compose.material.icons.automirrored.filled.DirectionsRun
import androidx.compose.material.icons.automirrored.filled.DirectionsWalk
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.DownhillSkiing
import androidx.compose.material.icons.filled.FitnessCenter
import androidx.compose.material.icons.filled.Hiking
import androidx.compose.material.icons.filled.LocalFireDepartment
import androidx.compose.material.icons.filled.MonitorHeart
import androidx.compose.material.icons.filled.Pool
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Rowing
import androidx.compose.material.icons.filled.SelfImprovement
import androidx.compose.material.icons.filled.SportsBasketball
import androidx.compose.material.icons.filled.SportsGolf
import androidx.compose.material.icons.filled.SportsMma
import androidx.compose.material.icons.filled.SportsSoccer
import androidx.compose.material.icons.filled.SportsTennis
import androidx.compose.material.icons.filled.Terrain
import androidx.compose.material.icons.filled.Timer
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.data.TrainingTargets
import com.sennnen.mav.data.WorkoutRow
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.zoneSummary
import java.text.DateFormat
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.roundToInt

// Strain hub (Android port of Strand/UI/AuraStrainView.swift) — day strain
// building live, time in HR zones, the activities list (tap → workout detail),
// and workout start / live session entry.

@Composable
fun AuraStrainScreen(
    vm: AppViewModel,
    onOpenWorkouts: () -> Unit,
    onOpenLive: () -> Unit,
    onOpenStrength: () -> Unit = {},
) {
    val p = Aura.palette
    val days by vm.recentDays.collectAsStateWithLifecycle()
    val live by vm.live.collectAsStateWithLifecycle()
    val activeWorkout by vm.activeWorkout.collectAsStateWithLifecycle()
    val workouts by vm.workouts.collectAsStateWithLifecycle()

    LaunchedEffect(days) { vm.loadWorkouts() }

    val anchor = auraAnchorDay(days)
    val strain = anchor?.strain
    val factor = AuraEffort.displayFactor()

    val sorted = remember(workouts) { workouts.sortedByDescending { it.startTs } }
    val anchorKey = anchor?.day ?: java.time.LocalDate.now().toString()
    val todayRows = remember(sorted, anchorKey) {
        val fmt = SimpleDateFormat("yyyy-MM-dd", Locale.US)
        sorted.filter { fmt.format(Date(it.startTs * 1000)) == anchorKey }
    }
    val recentRows = remember(sorted) { sorted.take(14) }

    // Weekly zone targets: this week's banked minutes vs rule-based targets from
    // the prior full weeks + the recent Charge trend (mirrors iOS's load()).
    var weekDone by remember { mutableStateOf<List<Double>?>(null) }
    var weekTargets by remember { mutableStateOf<List<Double>?>(null) }
    LaunchedEffect(workouts) {
        val byWeek = TrainingTargets.weeklyZoneMinutes(sorted)
        val thisWeek = TrainingTargets.currentWeekKey()
        weekDone = byWeek[thisWeek] ?: List(5) { 0.0 }
        val priorWeeks = byWeek.filter { it.key != thisWeek }.values.toList().takeLast(4)
        val recovery = days.takeLast(7).mapNotNull { it.recovery }
        val recoveryAvg = if (recovery.isEmpty()) null else recovery.sum() / recovery.size
        weekTargets = TrainingTargets.weeklyTargets(recentWeeks = priorWeeks, recoveryAvg = recoveryAvg)
    }

    var revealed by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { revealed = true }
    var editing by remember { mutableStateOf(false) }
    val (hiddenCSV, setHiddenCSV) = rememberHubHiddenCards("strain")
    var selectedWorkout by remember { mutableStateOf<WorkoutRow?>(null) }
    var showTimer by remember { mutableStateOf(false) }

    AuraScreen(lead = AuraFamily.EFFORT) {
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .statusBarsPadding()
                .padding(horizontal = Aura.screenMargin)
                .padding(top = 8.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(Aura.sectionGap),
        ) {
            AuraHubHeader(
                title = "Strain",
                subtitle = "The load you're putting in",
                editing = editing,
                onToggleEditing = { editing = !editing },
            )

            // MARK: Live banner
            activeWorkout?.let { w ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .auraGlass(CircleShape)
                        .auraPressable(onClick = onOpenLive)
                        .padding(horizontal = 18.dp, vertical = 14.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    // The live dot breathes while a session is recording.
                    val pulse = rememberInfiniteTransition(label = "liveDot")
                    val dotScale by pulse.animateFloat(
                        initialValue = 1f, targetValue = 1.4f,
                        animationSpec = infiniteRepeatable(tween(750), RepeatMode.Reverse),
                        label = "liveDotScale",
                    )
                    Box(Modifier.size(8.dp).scale(dotScale).background(p.bad, CircleShape))
                    Text("Workout in progress", style = AuraType.label, color = p.ink)
                    Spacer(Modifier.weight(1f))
                    Text(
                        AuraEffort.text(w.liveStrain, factor),
                        style = AuraType.number(20.sp), color = AuraFamily.EFFORT.glow,
                    )
                    Icon(
                        Icons.Filled.ChevronRight, contentDescription = null,
                        tint = p.ink.copy(alpha = 0.4f), modifier = Modifier.size(16.dp),
                    )
                }
            }

            // MARK: Hero
            AuraGlowTile(
                AuraFamily.EFFORT,
                modifier = Modifier.auraReveal(revealed, 1),
                padding = 22.dp, radius = 34.dp,
            ) {
                Column(Modifier.heightIn(min = 240.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Text("Effort", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                        Spacer(Modifier.weight(1f))
                        AuraStatusChip(text = strainWord(strain), kind = AuraChipKind.NEUTRAL)
                    }
                    Text(
                        AuraEffort.text(strain, factor),
                        style = AuraType.mega(88.sp), color = p.ink, maxLines = 1,
                    )
                    AuraSlider(value = (strain ?: 0.0) / 100, glow = AuraFamily.EFFORT.glow)
                    Text(
                        "Cardiovascular load for the day, built from your heart-rate.",
                        style = AuraType.sub, color = p.ink.copy(alpha = 0.8f),
                    )
                }
            }

            // MARK: Start
            AuraDarkCard(padding = 0.dp) {
                Spacer(Modifier.padding(top = 4.dp))
                AuraNavRow(
                    icon = Icons.Filled.PlayArrow, title = "Start a workout",
                    detail = "Live HR + strain", tint = p.accentInk, onClick = onOpenWorkouts,
                )
                HorizontalDivider(color = p.hairline, thickness = 1.dp, modifier = Modifier.padding(start = 18.dp))
                AuraNavRow(
                    icon = Icons.Filled.FitnessCenter, title = "Strength trainer",
                    detail = "Log sets, reps & rest", tint = AuraFamily.EFFORT.glow, onClick = onOpenStrength,
                )
                HorizontalDivider(color = p.hairline, thickness = 1.dp, modifier = Modifier.padding(start = 18.dp))
                AuraNavRow(
                    icon = Icons.Filled.MonitorHeart, title = "Live heart-rate",
                    detail = live.heartRate?.let { "$it bpm" } ?: "",
                    tint = AuraFamily.HEART.glow, onClick = onOpenLive,
                )
                HorizontalDivider(color = p.hairline, thickness = 1.dp, modifier = Modifier.padding(start = 18.dp))
                AuraNavRow(
                    icon = Icons.Filled.Timer, title = "Timer",
                    detail = timerDetail(), tint = AuraFamily.ENERGY.glow, onClick = { showTimer = true },
                )
                Spacer(Modifier.padding(top = 4.dp))
            }

            // MARK: Zones (today)
            zoneSummary(todayRows)?.let { summary ->
                AuraEditableCard("zones", hiddenCSV, setHiddenCSV, editing) {
                    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                        AuraSectionHeader(title = "Time in zones")
                        AuraDarkCard {
                            AuraZoneBars(minutes = summary.minutes)
                        }
                    }
                }
            }

            // MARK: Weekly zone targets
            if ((weekTargets?.any { it > 0 }) == true) {
                AuraEditableCard("weeklyTargets", hiddenCSV, setHiddenCSV, editing) {
                    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                        AuraSectionHeader(title = "This week's zones")
                        AuraDarkCard(padding = 18.dp) {
                            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                                AuraZoneBars(
                                    minutes = weekDone ?: List(5) { 0.0 },
                                    targets = weekTargets!!.map { if (it > 0) it else null },
                                )
                                Text(
                                    TrainingTargets.nudgeLine(weekDone ?: List(5) { 0.0 }, weekTargets!!)
                                        ?: "Weekly targets met. Nice week.",
                                    style = AuraType.caption, color = p.ink.copy(alpha = 0.6f),
                                )
                                Text(
                                    "Targets adapt to your last four weeks and your recovery. Low Charge weeks plan easier.",
                                    style = AuraType.caption, color = p.ink.copy(alpha = 0.4f),
                                )
                            }
                        }
                    }
                }
            }

            // MARK: Activities
            AuraEditableCard("activities", hiddenCSV, setHiddenCSV, editing) {
                Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    AuraSectionHeader(title = "Activities", actionTitle = "All", action = onOpenWorkouts)
                    if (recentRows.isEmpty()) {
                        AuraDarkCard {
                            Text(
                                "No workouts yet. Start one, or let auto-detection catch the next.",
                                style = AuraType.sub, color = p.ink.copy(alpha = 0.6f),
                            )
                        }
                    } else {
                        AuraDarkCard(padding = 0.dp) {
                            Spacer(Modifier.padding(top = 4.dp))
                            recentRows.forEachIndexed { i, w ->
                                WorkoutRowItem(w, factor) { selectedWorkout = w }
                                if (i < recentRows.size - 1) {
                                    HorizontalDivider(
                                        color = p.hairline, thickness = 1.dp,
                                        modifier = Modifier.padding(start = 18.dp),
                                    )
                                }
                            }
                            Spacer(Modifier.padding(top = 4.dp))
                        }
                    }
                }
            }
        }
    }

    selectedWorkout?.let { w ->
        AuraWorkoutDetailSheet(vm = vm, row = w, onDismiss = { selectedWorkout = null })
    }

    if (showTimer) {
        AuraTimerSheet(onDismiss = { showTimer = false }, strapBonded = live.bonded)
    }
}

/** Live countdown readout on the Timer nav row while it runs (twin of Today's timerSub). */
@Composable
private fun timerDetail(): String {
    @Suppress("UNUSED_EXPRESSION") AuraCountdown.heartbeat
    if (AuraCountdown.isRinging) return "Time's up"
    val r = AuraCountdown.remaining ?: return "Buzz at zero"
    return String.format(java.util.Locale.US, "%d:%02d", r / 60, r % 60)
}

private fun strainWord(strain: Double?): String {
    val s = strain ?: return "No data"
    return when {   // stored 0–100 axis
        s >= 86 -> "All out"
        s >= 67 -> "Strenuous"
        s >= 48 -> "Moderate"
        else -> "Light"
    }
}

@Composable
private fun WorkoutRowItem(w: WorkoutRow, factor: Double, onClick: () -> Unit) {
    val p = Aura.palette
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 18.dp, vertical = 13.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            sportIcon(w.sport), contentDescription = null,
            tint = AuraFamily.EFFORT.glow, modifier = Modifier.width(26.dp).size(20.dp),
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(w.sport, style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
            Text(
                SimpleDateFormat("EEE d MMM", Locale.getDefault()).format(Date(w.startTs * 1000)) +
                    " " + DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(w.startTs * 1000)),
                style = AuraType.caption, color = p.ink.copy(alpha = 0.5f),
            )
        }
        Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(2.dp)) {
            if (w.strain != null) {
                Text(
                    AuraEffort.text(w.strain, factor),
                    style = AuraType.number(20.sp), color = AuraFamily.EFFORT.glow,
                )
            }
            val durMin = ((w.durationS ?: (w.endTs - w.startTs).toDouble()) / 60.0).roundToInt()
            Text(
                if (durMin >= 60) "${durMin / 60}h ${durMin % 60}m" else "${durMin}m",
                style = AuraType.caption, color = p.ink.copy(alpha = 0.55f),
            )
        }
        Icon(
            Icons.Filled.ChevronRight, contentDescription = null,
            tint = p.ink.copy(alpha = 0.35f), modifier = Modifier.size(16.dp),
        )
    }
}

fun sportIcon(sport: String): ImageVector = when (sport.lowercase()) {
    "running", "trail running" -> Icons.AutoMirrored.Filled.DirectionsRun
    "walking" -> Icons.AutoMirrored.Filled.DirectionsWalk
    "hiking" -> Icons.Filled.Hiking
    "cycling", "mountain biking", "spin" -> Icons.AutoMirrored.Filled.DirectionsBike
    "swimming" -> Icons.Filled.Pool
    "rowing" -> Icons.Filled.Rowing
    "yoga", "pilates" -> Icons.Filled.SelfImprovement
    "weightlifting", "functional fitness", "strength" -> Icons.Filled.FitnessCenter
    "tennis", "padel", "squash", "badminton", "pickleball" -> Icons.Filled.SportsTennis
    "football", "soccer" -> Icons.Filled.SportsSoccer
    "basketball" -> Icons.Filled.SportsBasketball
    "golf" -> Icons.Filled.SportsGolf
    "boxing", "martial arts", "kickboxing" -> Icons.Filled.SportsMma
    "skiing", "snowboarding" -> Icons.Filled.DownhillSkiing
    "climbing", "rock climbing" -> Icons.Filled.Terrain
    else -> Icons.Filled.LocalFireDepartment
}
