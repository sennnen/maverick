package com.sennnen.mav.ui.mav

import uniffi.mav_ffi.AnalyticAvailabilityReport
import com.sennnen.mav.BuildConfig
import com.sennnen.mav.data.WorkoutRow
import uniffi.mav_ffi.DailySnapshotReport
import uniffi.mav_ffi.HrvReport
import uniffi.mav_ffi.ReadinessReport
import java.time.LocalDate
import java.time.ZoneId
import kotlin.math.round

// Debug-only fixture days. The iOS twin is Model/MavDebugFixture.swift and the two produce the same
// numbers, so a screenshot from one platform can be compared with the other.
//
// Fenced three ways: every entry point checks MAV_SHOW_SYNTHETIC_DATA, it is only consulted when nothing
// is connected, and every surface it feeds renders a visible SAMPLE badge.
//
// The shape mirrors what the core actually produces — one admitted analytic with a real band, the
// rest honestly unavailable with the core's own reason kinds — so the layout being judged is the
// layout that ships, not a prettier one.

object MavDebugFixture {

    const val DAY_COUNT = 45

    /** Hand-shaped daily variation; a sine fixture looked like a demo waveform, not a person. */
    private val variabilityValues = listOf(
        59.8, 61.2, 58.9, 62.4, 64.1, 63.0, 60.7, 61.9, 65.2, 66.0,
        64.4, 62.8, 63.6, 67.1, 65.9, 64.8, 66.5, 68.0, 67.2, 65.1,
        63.9, 66.8, 69.2, 68.4, 67.6, 70.1, 68.9, 66.7, 67.8, 71.0,
        69.7, 68.3, 70.5, 72.1, 71.4, 69.0, 70.2, 73.0, 71.8, 72.6,
        74.1, 72.9, 73.5, 75.0, 73.0,
    )

    /** Irregular debug-only score history; its final value matches the Recovery card. */
    val scoreHistory = listOf(
        72.0, 71.0, 73.0, 74.0, 76.0, 75.0, 74.0, 73.0, 72.0, 74.0,
        75.0, 77.0, 76.0, 74.0, 73.0, 75.0, 78.0, 79.0, 78.0, 77.0,
        75.0, 74.0, 76.0, 77.0, 79.0, 81.0, 80.0, 78.0, 77.0, 79.0,
        80.0, 82.0, 83.0, 82.0, 80.0, 79.0, 81.0, 82.0, 84.0, 85.0,
        84.0, 82.0, 81.0, 83.0, 82.0,
    )

    fun snapshots(today: LocalDate = LocalDate.now()): List<DailySnapshotReport> {
        if (!BuildConfig.MAV_SHOW_SYNTHETIC_DATA) return emptyList()
        return (DAY_COUNT - 1 downTo 0).map { back ->
            val date = today.minusDays(back.toLong())
            val offset = DAY_COUNT - back
            val rmssd = variabilityValues[offset - 1]
            DailySnapshotReport(
                day = date.toString(),
                dayIndex = offset.toLong(),
                currentBpm = if (back == 0) 64u else null,
                meanBpm = 68.4,
                hrSampleCount = 12_480u,
                hrExcludedCount = 214u,
                hrv = HrvReport(
                    label = "pulse_rate_variability",
                    meanIntervalMs = 862.0,
                    rmssdMs = rmssd,
                    sdnnMs = round(rmssd * 0.72 * 10) / 10,
                    pnn50Percent = 18.4,
                    sd1Ms = round(rmssd * 0.71 * 10) / 10,
                    sd2Ms = round(rmssd * 1.31 * 10) / 10,
                    alpha1 = 1.04,
                    intervalCount = 8_412u,
                    excludedCount = 118u,
                ),
                hrvSpectrum = null,
                readiness = ReadinessReport(
                    tier = if (rmssd > 74) "primed" else if (rmssd < 58) "suppressed" else "normal",
                    baseline7Ms = 66.0,
                    normalLowMs = 55.0,
                    normalHighMs = 78.0,
                    overreachingWatch = false,
                ),
                availability = availability,
                algorithms = listOf("time_domain_interval_variability@1.0.0", "hr_feature@1.0.0"),
                snapshotHash = "fixture-%04d".format(offset),
            )
        }
    }

    fun workouts(today: LocalDate = LocalDate.now()): List<WorkoutRow> {
        if (!BuildConfig.MAV_SHOW_SYNTHETIC_DATA) return emptyList()
        fun session(
            daysBack: Long,
            hour: Int,
            minutes: Int,
            sport: String,
            avgHr: Int,
            maxHr: Int,
            strain: Double,
            energy: Double,
            zones: String,
        ): WorkoutRow {
            val start = today.minusDays(daysBack).atTime(hour, 0)
                .atZone(ZoneId.systemDefault()).toEpochSecond()
            return WorkoutRow(
                deviceId = "fixture",
                startTs = start,
                endTs = start + minutes * 60L,
                sport = sport,
                source = "Sample",
                durationS = minutes * 60.0,
                energyKcal = energy,
                avgHr = avgHr,
                maxHr = maxHr,
                strain = strain,
                zonesJSON = zones,
                notes = "Sample session",
            )
        }
        return listOf(
            session(0, 17, 52, "Strength", 116, 151, 11.4, 338.0, """{"z1":18,"z2":34,"z3":29,"z4":15,"z5":4}"""),
            session(1, 7, 38, "Running", 146, 176, 14.8, 462.0, """{"z1":3,"z2":15,"z3":31,"z4":39,"z5":12}"""),
            session(2, 18, 42, "Rowing", 138, 169, 13.6, 418.0, """{"z1":5,"z2":21,"z3":35,"z4":31,"z5":8}"""),
            session(3, 18, 61, "Cycling", 132, 163, 12.7, 521.0, """{"z1":5,"z2":26,"z3":38,"z4":25,"z5":6}"""),
            session(4, 7, 45, "Swimming", 127, 158, 10.8, 356.0, """{"z1":9,"z2":33,"z3":36,"z4":18,"z5":4}"""),
            session(5, 12, 31, "Walking", 101, 124, 6.2, 174.0, """{"z1":48,"z2":38,"z3":12,"z4":2,"z5":0}"""),
            session(6, 8, 47, "Yoga", 88, 111, 4.8, 128.0, """{"z1":70,"z2":26,"z3":4,"z4":0,"z5":0}"""),
        )
    }

    /**
     * The same reason kinds the core emits, so the unavailable cards under test are the real ones
     * rather than a friendlier rewrite.
     */
    private val availability: List<AnalyticAvailabilityReport> = listOf(
        AnalyticAvailabilityReport("time_domain_hrv", true, null, emptyList()),
        AnalyticAvailabilityReport("recovery", false, "algorithm_not_admitted", emptyList()),
        AnalyticAvailabilityReport("sleep_performance", false, "missing_streams", listOf("sleep_stage")),
        AnalyticAvailabilityReport(
            "illness_risk", false, "missing_streams", listOf("skin_temperature", "respiration"),
        ),
        AnalyticAvailabilityReport("cycle_phase", false, "missing_streams", listOf("skin_temperature")),
    )
}
