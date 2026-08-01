package com.sennnen.mav.ui.mav

import android.content.Context
import android.content.SharedPreferences
import org.json.JSONObject

// Cardio session configuration: the per-sport sticky start config and the milestone deep settings.
// The Swift twin is `WorkoutPrefs.swift`, which has carried these types since the Aura port; this
// file is Android catching up so the confirm screen can exist on both platforms.
//
// Values are stored natively — kilometres, minutes, kilocalories — and converted only for display.
// A stored config that meant miles on one phone and kilometres on another is a class of bug worth
// designing out rather than testing for.

/** What ends (or measures) a cardio session. Mutually exclusive — pick one. */
enum class MavGoalKind(val label: String) {
    NONE("Free"),
    DISTANCE("Distance"),
    TIME("Time"),
    CALORIES("Calories"),
}

/**
 * The chosen end condition. [value] is native per kind: km for distance, minutes for time, kcal for
 * calories, ignored for [MavGoalKind.NONE].
 */
data class MavGoal(val kind: MavGoalKind = MavGoalKind.NONE, val value: Double = 0.0) {
    val isActive: Boolean get() = kind != MavGoalKind.NONE && value > 0.0

    companion object {
        val None = MavGoal()
    }
}

/** Optional zone-time target for a session ("zone 2 for 15 min"). */
data class MavZoneTarget(val zone: Int, val minutes: Int)

/** Everything the confirm screen configures, persisted per sport so the next start pre-fills. */
data class MavWorkoutConfig(
    val goal: MavGoal = MavGoal.None,
    val zoneTarget: MavZoneTarget? = null,
    /** Per-session GPS override; null means the sport's own default. */
    val gpsEnabled: Boolean? = null,
    val keepScreenOn: Boolean = false,
)

/** Interim-buzz cadence for a time end condition. */
enum class MavTimeMilestoneMode(val label: String) {
    HALFWAY("Halfway"),
    EVERY10("Every 10 min"),
    EVERY15("Every 15 min"),
    OFF("Off"),
}

/** Interim-buzz cadence for a calorie end condition. */
enum class MavCalorieMilestoneMode(val label: String) {
    HALFWAY("Halfway"),
    EVERY50("Every 50 kcal"),
    EVERY100("Every 100 kcal"),
    OFF("Off"),
}

/**
 * Sticky settings, which are the whole template system: the last configuration used for a sport is
 * the next one offered for it. A separate "save as template" step is a step nobody takes.
 */
class MavWorkoutPrefs(private val prefs: SharedPreferences) {

    constructor(context: Context) : this(
        context.getSharedPreferences("mav_workout", Context.MODE_PRIVATE),
    )

    fun config(sport: String): MavWorkoutConfig {
        val raw = prefs.getString(configKey(sport), null) ?: return MavWorkoutConfig()
        return runCatching {
            val json = JSONObject(raw)
            MavWorkoutConfig(
                goal = MavGoal(
                    kind = MavGoalKind.entries.firstOrNull { it.name == json.optString("goalKind") }
                        ?: MavGoalKind.NONE,
                    value = json.optDouble("goalValue", 0.0),
                ),
                zoneTarget = if (json.has("zone")) {
                    MavZoneTarget(json.getInt("zone"), json.getInt("zoneMinutes"))
                } else {
                    null
                },
                gpsEnabled = if (json.has("gps")) json.getBoolean("gps") else null,
                keepScreenOn = json.optBoolean("keepScreenOn", false),
            )
            // A config that will not parse is a config from an older shape. Falling back to the
            // default is correct and silent; there is no user data to lose in a start preset.
        }.getOrDefault(MavWorkoutConfig())
    }

    fun save(config: MavWorkoutConfig, sport: String) {
        val json = JSONObject()
            .put("goalKind", config.goal.kind.name)
            .put("goalValue", config.goal.value)
            .put("keepScreenOn", config.keepScreenOn)
        config.zoneTarget?.let { json.put("zone", it.zone).put("zoneMinutes", it.minutes) }
        config.gpsEnabled?.let { json.put("gps", it) }
        prefs.edit().putString(configKey(sport), json.toString()).apply()
    }

    /** Distance interim spacing in the wearer's display unit, default 1. */
    fun distanceEveryUnits(): Double =
        prefs.getFloat(DISTANCE_EVERY_KEY, 1f).toDouble().takeIf { it > 0 } ?: 1.0

    fun timeMode(): MavTimeMilestoneMode =
        MavTimeMilestoneMode.entries.firstOrNull { it.name == prefs.getString(TIME_MODE_KEY, null) }
            ?: MavTimeMilestoneMode.HALFWAY

    fun calorieMode(): MavCalorieMilestoneMode =
        MavCalorieMilestoneMode.entries
            .firstOrNull { it.name == prefs.getString(CALORIE_MODE_KEY, null) }
            ?: MavCalorieMilestoneMode.HALFWAY

    fun setTimeMode(mode: MavTimeMilestoneMode) =
        prefs.edit().putString(TIME_MODE_KEY, mode.name).apply()

    fun setCalorieMode(mode: MavCalorieMilestoneMode) =
        prefs.edit().putString(CALORIE_MODE_KEY, mode.name).apply()

    private fun configKey(sport: String) = "workout.config.${slug(sport)}"

    companion object {
        const val DISTANCE_EVERY_KEY = "workout.milestone.distanceEvery"
        const val TIME_MODE_KEY = "workout.milestone.timeMode"
        const val CALORIE_MODE_KEY = "workout.milestone.calorieMode"

        /** Slug a display name into a stable id ("Outdoor run" → "outdoor-run"). */
        fun slug(name: String): String = name.lowercase()
            .split(Regex("[^a-z0-9]+"))
            .filter { it.isNotEmpty() }
            .joinToString("-")
    }
}
