package com.sennnen.mav.ui.mav

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.sennnen.mav.ui.UnitFormatter
import com.sennnen.mav.ui.UnitPrefs
import com.sennnen.mav.ui.UnitSystem

// What happens before a cardio session starts: the end condition, an optional zone target, and the
// two per-session options. The iOS twin is `MavWorkoutConfigView` in MavWorkoutsView.swift.
//
// This is the screen the rewrite lost. Without it every workout was a free workout — the strap
// recorded until you remembered to stop it — and the entire milestone vocabulary had nothing to
// fire against.
//
// Settings are sticky per sport, which is the whole template system: the last configuration used
// for a run is the next one offered for a run. A separate "save as template" step is a step nobody
// takes.

@Composable
fun MavWorkoutConfigScreen(
    sport: MavSport,
    deviceName: String?,
    onStart: (String, MavWorkoutConfig) -> Unit,
    onBack: () -> Unit,
) {
    val palette = MavTheme.palette
    val context = LocalContext.current
    val prefs = remember(context) { MavWorkoutPrefs(context) }
    val system = remember { UnitPrefs.system(context) }
    val isImperial = system == UnitSystem.IMPERIAL
    val distanceUnit = UnitFormatter.distanceUnit(system)

    var config by remember(sport.name) { mutableStateOf(prefs.config(sport.name)) }
    var goalText by remember(sport.name) {
        mutableStateOf(goalDisplayText(prefs.config(sport.name).goal, isImperial))
    }
    var zoneOn by remember(sport.name) { mutableStateOf(prefs.config(sport.name).zoneTarget != null) }
    var minutesMenuOpen by remember { mutableStateOf(false) }

    // Nothing declares haptics yet (ADR-032), so the buzz hints describe what *would* happen and
    // the screen says plainly that it will not. Promising a wrist tap the strap cannot deliver is
    // the failure this check exists to prevent.
    val haptics = MavHapticSupport.None

    // Distance is only offered where a distance means something. A "5 km" yoga session is not a
    // goal, it is a confused control.
    val kinds = MavGoalKind.entries.filter { it != MavGoalKind.DISTANCE || sport.isDistance }

    MavDetailScaffold(sport.name, onBack) {

        // End condition ---------------------------------------------------------------------
        MavTile {
            Text("End condition", style = MavType.caption, color = palette.inkSecondary)
            Row(
                Modifier.fillMaxWidth().padding(top = 14.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                kinds.forEach { kind ->
                    FilterChip(
                        selected = config.goal.kind == kind,
                        onClick = {
                            config = config.copy(
                                goal = MavGoal(kind, defaultGoalValue(kind, isImperial)),
                            )
                            goalText = goalDisplayText(config.goal, isImperial)
                        },
                        label = { Text(kind.label, style = MavType.caption) },
                        modifier = Modifier.weight(1f),
                    )
                }
            }

            if (config.goal.kind != MavGoalKind.NONE) {
                Row(
                    Modifier.fillMaxWidth().padding(top = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    OutlinedTextField(
                        value = goalText,
                        onValueChange = { entered ->
                            goalText = entered
                            val parsed = entered.replace(',', '.').toDoubleOrNull() ?: 0.0
                            // Stored natively — km, minutes, kcal — so the comparison in
                            // MavMilestones never sees a display unit.
                            val native =
                                if (config.goal.kind == MavGoalKind.DISTANCE && isImperial) {
                                    parsed / UnitFormatter.MILES_PER_KILOMETER
                                } else {
                                    parsed
                                }
                            config = config.copy(goal = config.goal.copy(value = native))
                        },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        modifier = Modifier
                            .width(120.dp)
                            .semantics {
                                contentDescription =
                                    "${config.goal.kind.label} goal in ${goalUnit(config.goal.kind, distanceUnit)}"
                            },
                    )
                    Text(
                        goalUnit(config.goal.kind, distanceUnit),
                        style = MavType.label,
                        color = palette.inkSecondary,
                        modifier = Modifier.padding(start = 10.dp),
                    )
                }

                Text(
                    interimHint(config.goal.kind, distanceUnit, prefs, haptics, deviceName),
                    style = MavType.sub,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 12.dp),
                )
            }
        }

        // Zone target -----------------------------------------------------------------------
        MavTile {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "Zone target",
                    style = MavType.label,
                    color = palette.ink,
                    modifier = Modifier.weight(1f),
                )
                Switch(
                    checked = zoneOn,
                    onCheckedChange = { on ->
                        zoneOn = on
                        config = config.copy(
                            zoneTarget = if (on) config.zoneTarget ?: MavZoneTarget(2, 20) else null,
                        )
                    },
                )
            }

            val target = config.zoneTarget
            if (zoneOn && target != null) {
                Row(
                    Modifier.fillMaxWidth().padding(top = 14.dp),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    (1..5).forEach { zone ->
                        FilterChip(
                            selected = target.zone == zone,
                            onClick = { config = config.copy(zoneTarget = target.copy(zone = zone)) },
                            label = { Text("Z$zone", style = MavType.caption) },
                            modifier = Modifier
                                .weight(1f)
                                .semantics { contentDescription = "Zone $zone" },
                        )
                    }
                }

                Row(
                    Modifier.fillMaxWidth().padding(top = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("for", style = MavType.sub, color = palette.inkSecondary)
                    Spacer(Modifier.width(10.dp))
                    TextButton(onClick = { minutesMenuOpen = true }) {
                        Text("${target.minutes} min", style = MavType.label)
                    }
                    DropdownMenu(minutesMenuOpen, onDismissRequest = { minutesMenuOpen = false }) {
                        listOf(10, 15, 20, 30, 45, 60).forEach { minutes ->
                            DropdownMenuItem(
                                text = { Text("$minutes min") },
                                onClick = {
                                    config = config.copy(zoneTarget = target.copy(minutes = minutes))
                                    minutesMenuOpen = false
                                },
                            )
                        }
                    }
                }

                Text(
                    "The zone bars track it live, and it is banked once the time is in.",
                    style = MavType.sub,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 4.dp),
                )
            }
        }

        // Options ---------------------------------------------------------------------------
        MavTile(padded = false) {
            if (sport.isDistance) {
                MavConfigToggleRow(
                    "GPS route",
                    "Distance, pace and the route map",
                    config.gpsEnabled ?: true,
                ) { config = config.copy(gpsEnabled = it) }
                MavDivider()
            }
            MavConfigToggleRow(
                "Keep screen on",
                "No auto-lock while the session runs",
                config.keepScreenOn,
            ) { config = config.copy(keepScreenOn = it) }
        }

        Spacer(Modifier.height(4.dp))
        MavWideButton(
            "Start ${sport.name.lowercase()}",
            modifier = Modifier.fillMaxWidth(),
            onClick = {
                val resolved = config.copy(
                    goal = if (config.goal.isActive) config.goal else MavGoal.None,
                    zoneTarget = if (zoneOn) config.zoneTarget else null,
                )
                // Persist before starting, so the settings are sticky even if the session is
                // abandoned.
                prefs.save(resolved, sport.name)
                onStart(sport.name, resolved)
            },
        )
    }
}

@Composable
private fun MavConfigToggleRow(
    title: String,
    detail: String,
    checked: Boolean,
    onChange: (Boolean) -> Unit,
) {
    val palette = MavTheme.palette
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = MavTheme.tilePadding, vertical = 13.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        androidx.compose.foundation.layout.Column(Modifier.weight(1f)) {
            Text(title, style = MavType.label, color = palette.ink)
            Text(
                detail,
                style = MavType.sub,
                color = palette.inkSecondary,
                modifier = Modifier.padding(top = 3.dp),
            )
        }
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

// Pure helpers, shared with the tests -------------------------------------------------------

internal fun defaultGoalValue(kind: MavGoalKind, isImperial: Boolean): Double = when (kind) {
    MavGoalKind.NONE -> 0.0
    MavGoalKind.DISTANCE -> if (isImperial) 3 / UnitFormatter.MILES_PER_KILOMETER else 5.0
    MavGoalKind.TIME -> 30.0
    MavGoalKind.CALORIES -> 300.0
}

internal fun goalDisplayText(goal: MavGoal, isImperial: Boolean): String {
    if (!goal.isActive) return ""
    val value = if (goal.kind == MavGoalKind.DISTANCE && isImperial) {
        UnitFormatter.kmToMiles(goal.value)
    } else {
        goal.value
    }
    val rounded = Math.round(value * 10) / 10.0
    return if (rounded == Math.floor(rounded)) rounded.toInt().toString() else "%.1f".format(rounded)
}

internal fun goalUnit(kind: MavGoalKind, distanceUnit: String): String = when (kind) {
    MavGoalKind.NONE -> ""
    MavGoalKind.DISTANCE -> distanceUnit
    MavGoalKind.TIME -> "min"
    MavGoalKind.CALORIES -> "kcal"
}

/** What the strap will do, stated where the decision is made rather than buried in settings. */
internal fun interimHint(
    kind: MavGoalKind,
    distanceUnit: String,
    prefs: MavWorkoutPrefs,
    haptics: MavHapticSupport,
    deviceName: String?,
): String {
    if (!haptics.supports(MavHapticSignal.GoalComplete)) {
        return haptics.reason(deviceName) + " The goal still tracks on screen."
    }
    return when (kind) {
        MavGoalKind.NONE -> ""
        MavGoalKind.DISTANCE ->
            "A light tap every ${if (distanceUnit == "mi") "mile" else "kilometre"}, " +
                "and a strong buzz at the goal."

        MavGoalKind.TIME -> {
            val cadence =
                if (prefs.timeMode() == MavTimeMilestoneMode.HALFWAY) "at halfway" else "on the interval"
            "A light tap $cadence, and a strong buzz at the goal."
        }

        MavGoalKind.CALORIES -> {
            val cadence =
                if (prefs.calorieMode() == MavCalorieMilestoneMode.HALFWAY) "at halfway" else "on the interval"
            "A light tap $cadence, and a strong buzz at the goal."
        }
    }
}
