package com.sennnen.mav.ui

/*
 * Presentation value types the sleep and graph screens draw with. They arrived alongside the
 * on-device scorers and are not analytics: they carry no calculation, and the numbers that fill
 * them come from the core.
 *
 * Every `ts` / `start` / `end` is wall-clock unix SECONDS, matching the com.sennnen.mav.data layer.
 */

/** User profile for HRmax + calorie estimation. Mirrors Swift `UserProfile`. */
data class UserProfile(
    val weightKg: Double = 70.0,
    val heightCm: Double = 170.0,
    val age: Double = 30.0,
    /** "male" | "female" | "nonbinary". */
    val sex: String = "nonbinary",
    /**
     * Counter ticks per real step for the @57 motion counter (#139). The WHOOP 5/MG
     * counter overcounts and its true tick rate is unknown, so the daily-steps total
     * divides by this. 1.0 = raw pass-through (default); the engine clamps ≥ 0.5.
     */
    val stepTicksPerStep: Double = 1.0,
    /**
     * Waist circumference (cm) for the Fitness Age VO₂max estimate (Phase 2). 0 = not set.
     * Optional — it UNLOCKS the VO₂max readout but does NOT sharpen the headline Fitness Age
     * (the body term cancels out of the age formula). Default param so existing call-sites compile.
     */
    val waistCm: Double = 0.0,
)

/**
 * A contiguous sleep-stage segment. Times are wall-clock unix seconds.
 * Mirrors Swift `StageSegment` (Codable → encoded verbatim into stagesJSON).
 * `start`/`end` are `var` so the stager can extend the trailing segment in place.
 */
data class StageSegment(
    var start: Long,
    var end: Long,
    /** "wake" | "light" | "deep" | "rem". */
    var stage: String,
)

/**
 * A detected sleep session (in-bed span) with APPROXIMATE staging.
 *
 * Named [DetectedSleep] (NOT SleepSession) to avoid clashing with the Room
 * entity com.sennnen.mav.data.SleepSession. Mirrors Swift `SleepSession` (the analytics
 * shape in SleepStager.swift), one-to-one.
 */
data class DetectedSleep(
    val start: Long,
    val end: Long,
    /** asleep / in-bed in [0, 1] (AASM TST/TIB; asleep = in-bed − wake). */
    val efficiency: Double,
    val stages: List<StageSegment>,
    /** Lowest 5-min rolling-mean HR during the session (bpm), or null. */
    val restingHR: Int?,
    /** Mean RMSSD over 5-min windows across the session (ms), or null. */
    val avgHRV: Double?,
)

/**
 * AASM-style metrics from a session's stage segments.
 * Mirrors Swift `SleepStager.HypnogramMetrics`.
 */
data class HypnogramMetrics(
    val tibS: Double,
    val tstS: Double,
    val sptS: Double,
    val solS: Double,
    /** NaN if no REM. */
    val remLatencyS: Double,
    val wasoS: Double,
    val efficiency: Double,
    val disturbances: Int,
    val deepMin: Double,
    val remMin: Double,
    val lightMin: Double,
    val deepPct: Double,
    val remPct: Double,
    val lightPct: Double,
)
