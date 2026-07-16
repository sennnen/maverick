package com.sennnen.mav.data

// Read-model twins of the NOOP entity types the Aura UI renders (Entities.kt), minus the
// Room annotations: Mav has no on-device Room store — rows come from the Rust core through
// the FFI snapshot, so these are plain value types. Field sets and semantics are unchanged
// so the copied Aura screens compile verbatim.

/** Daily metric row ("YYYY-MM-DD" day key). All metric columns nullable — absent stays absent. */
data class DailyMetric(
    val deviceId: String,
    val day: String,
    val totalSleepMin: Double? = null,
    val efficiency: Double? = null,
    val deepMin: Double? = null,
    val remMin: Double? = null,
    val lightMin: Double? = null,
    val disturbances: Int? = null,
    val restingHr: Int? = null,
    val avgHrv: Double? = null,
    val recovery: Double? = null,
    val strain: Double? = null,
    val exerciseCount: Int? = null,
    val spo2Pct: Double? = null,
    val skinTempDevC: Double? = null,
    val respRateBpm: Double? = null,
    val steps: Int? = null,
    val activeKcalEst: Double? = null,
    val sourcePriority: Int? = null,
) {
    companion object {
        const val SOURCE_PRIORITY_WHOOP = 0
        const val SOURCE_PRIORITY_MANUAL = 1
        const val SOURCE_PRIORITY_SYSTEM_HEALTH = 2
    }
}

/** One sleep session; `stagesJSON` is the verbatim stage-segments JSON array. */
data class SleepSession(
    val deviceId: String,
    val startTs: Long,
    val endTs: Long,
    val efficiency: Double? = null,
    val restingHr: Int? = null,
    val avgHrv: Double? = null,
    val stagesJSON: String? = null,
    val userEdited: Boolean = false,
    val startTsAdjusted: Long? = null,
    val motionJSON: String? = null,
    val sleepStateJSON: String? = null,
) {
    /** The bed (onset) time to display / sort by: the user's hand-set onset when edited. */
    val effectiveStartTs: Long get() = startTsAdjusted ?: startTs

    /** Whole-block duration in hours (effective onset → wake). */
    val durationHours: Double get() = (endTs - effectiveStartTs) / 3600.0

    /** Nap-shaped: short (< [NAP_MAX_HOURS]) or daytime-onset. Mirrors iOS SleepView.isNap. */
    val isNapShaped: Boolean
        get() {
            val cal = java.util.Calendar.getInstance().apply { timeInMillis = effectiveStartTs * 1000L }
            val h = cal.get(java.util.Calendar.HOUR_OF_DAY)
            val overnightOnset = h >= 20 || h < 10
            return durationHours < NAP_MAX_HOURS || !overnightOnset
        }

    companion object {
        const val NAP_MAX_HOURS: Double = 3.0
    }
}

/** Generic long-format metric row; natural key (deviceId, day, key). */
data class MetricSeriesRow(
    val deviceId: String,
    val day: String,
    val key: String,
    val value: Double,
)

/** One logged journal answer for a day. */
data class JournalEntry(
    val deviceId: String,
    val day: String,
    val question: String,
    val answeredYes: Boolean,
    val notes: String? = null,
    val numericValue: Double? = null,
)

/** One workout; `zonesJSON` is verbatim HR-zone-percentages JSON, times unix seconds. */
data class WorkoutRow(
    val deviceId: String,
    val startTs: Long,
    val endTs: Long,
    val sport: String,
    val source: String,
    val durationS: Double? = null,
    val energyKcal: Double? = null,
    val avgHr: Int? = null,
    val maxHr: Int? = null,
    val strain: Double? = null,
    val distanceM: Double? = null,
    val zonesJSON: String? = null,
    val notes: String? = null,
    val routePolyline: String? = null,
)
