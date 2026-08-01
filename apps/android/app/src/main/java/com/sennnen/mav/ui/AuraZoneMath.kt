package com.sennnen.mav.ui

import kotlin.math.roundToInt
import uniffi.mav_ffi.MavRuntime

/**
 * Heart-rate zone math, resolved by the shared core.
 *
 * There used to be a Kotlin ladder here and an identical one in Rust. Two implementations of the
 * same formula are two answers waiting to disagree, and only one of them has fixtures — so this
 * defers to `mav-analytic::hr_zones` through the FFI and holds no arithmetic of its own beyond the
 * rounding the display wants.
 */
object AuraZoneMath {

    /** Set once by the connector manager when the runtime opens. */
    @Volatile
    var runtime: MavRuntime? = null

    /**
     * The Tanaka ceiling for an age. Falls back to the published formula only if the runtime is not
     * open yet — the same number, computed here rather than not at all, and it converges the moment
     * the core is available.
     */
    fun tanakaMaxHr(age: Int): Int =
        runtime?.let { it.heartRateZones(age.toDouble(), null).maxHr.roundToInt() }
            ?: (208.0 - 0.7 * age).roundToInt()

    /** The effective ceiling: a manual override when the wearer has set one, else the estimate. */
    fun maxHr(age: Int, override: Int): Int = if (override > 0) override else tanakaMaxHr(age)

    /** The zone (1..5) a reading falls in; 0 below zone one. */
    fun zone(bpm: Int, age: Int, maxHrOverride: Int?): Int =
        runtime?.let {
            it.heartRateZoneFor(bpm.toDouble(), age.toDouble(), maxHrOverride?.toDouble()).toInt()
        } ?: 0

    /** Display bounds from the core's admitted ladder. No platform-side zone arithmetic. */
    fun bounds(age: Int, maxHrOverride: Int): List<String>? {
        val report = runtime?.heartRateZones(
            age.toDouble(),
            maxHrOverride.takeIf { it > 0 }?.toDouble(),
        ) ?: return null
        val lows = report.lowerBpm
        if (lows.size < 5) return null
        return (0 until 5).map { index ->
            val low = lows[index].roundToInt()
            if (index == 4) "$low+ bpm"
            else "$low–${lows[index + 1].roundToInt() - 1}"
        }.mapIndexed { index, value ->
            if (index == 4) value else "$value bpm"
        }
    }
}
