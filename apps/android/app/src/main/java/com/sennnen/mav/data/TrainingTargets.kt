package com.sennnen.mav.data

import java.time.Instant
import java.time.ZoneId
import java.time.temporal.WeekFields
import java.util.Locale
import kotlin.math.roundToInt

/**
 * Weekly time-in-zone targets (Android port of Strand/Data/TrainingTargets.swift) —
 * rule-based and honest, no ML. Starts from a polarized 80/20 split over a weekly
 * volume the user actually trains at (never a fantasy plan), then applies two
 * adjustments:
 *
 * 1. Behaviour: a Z3/Z4-heavy history ("grey zone" training) shifts target share
 *    toward Z2 — the classic polarized-training correction.
 * 2. Recovery: a low recent Charge trend halves the Z4/Z5 targets and banks the
 *    time into Z1/Z2 — train, but easy.
 */
object TrainingTargets {

    /** Base share of weekly cardio time per zone (Z1…Z5), polarized 80/20. */
    val baseShare: List<Double> = listOf(0.25, 0.55, 0.10, 0.07, 0.03)

    /** Weekly minute floor: at least the WHO-ish 150 easy minutes. Never more than a
     *  2x stretch of what the user actually does — a target should pull, not mock. */
    const val floorMinutes = 150.0

    /**
     * Compute this week's per-zone minute targets.
     * @param recentWeeks per-week zone minutes (Z1…Z5) for up to the last ~4 full weeks,
     *   any order. Empty = no history, fall back to the floor volume.
     * @param recoveryAvg mean Charge over the recent window (0–100), null when unknown.
     */
    fun weeklyTargets(recentWeeks: List<List<Double>>, recoveryAvg: Double?): List<Double> {
        val weeklyTotals = recentWeeks.map { it.sum() }.filter { it > 0 }
        val observed = if (weeklyTotals.isEmpty()) 0.0 else weeklyTotals.sum() / weeklyTotals.size
        val volume = observed.coerceIn(floorMinutes, floorMinutes * 4)

        val share = baseShare.toMutableList()

        val totalAll = recentWeeks.flatten().sum()
        if (totalAll > 0) {
            val greyShare = recentWeeks.sumOf { week -> (week.getOrNull(2) ?: 0.0) + (week.getOrNull(3) ?: 0.0) } / totalAll
            if (greyShare > 0.3) {
                share[1] += 0.08
                share[2] -= 0.05
                share[3] -= 0.03
            }
        }

        if (recoveryAvg != null && recoveryAvg < 50) {
            val cut = (share[3] + share[4]) / 2
            share[3] = share[3] / 2
            share[4] = share[4] / 2
            share[0] = share[0] + cut * 0.4
            share[1] = share[1] + cut * 0.6
        }

        return share.map { (it * volume).roundToInt().toDouble() }
    }

    /** The single most useful "to go" sentence for a week in progress, or null when the
     *  week is on track (all targets met) or nothing meaningful remains. Prefers the
     *  LOW-zone gap — easy volume is the target people actually under-fill. */
    fun nudgeLine(done: List<Double>, targets: List<Double>): String? {
        if (done.size != 5 || targets.size != 5) return null
        for (i in listOf(1, 0, 2, 3, 4)) {
            if (targets[i] <= 0) continue
            val gap = targets[i] - done[i]
            if (gap < 5) continue
            return "${gap.roundToInt()} min of Zone ${i + 1} to go this week."
        }
        return null
    }

    /** Bucket workout rows into per-week zone minutes (Z1…Z5), keyed by the ISO week's
     *  Monday (device timezone) — mirrors Calendar.dateInterval(of: .weekOfYear). */
    fun weeklyZoneMinutes(rows: List<WorkoutRow>, zone: ZoneId = ZoneId.systemDefault()): Map<Long, List<Double>> {
        val weekFields = WeekFields.of(Locale.getDefault())
        val out = HashMap<Long, MutableList<Double>>()
        for (row in rows) {
            val pct = parseZonePercentsForTargets(row.zonesJSON) ?: continue
            val durMin = (row.durationS ?: (row.endTs - row.startTs).toDouble()) / 60.0
            if (durMin <= 0) continue
            val date = Instant.ofEpochSecond(row.startTs).atZone(zone).toLocalDate()
            val weekStart = date.minusDays((date.get(weekFields.dayOfWeek()) - 1).toLong())
            val weekKey = weekStart.toEpochDay()
            val mins = out.getOrPut(weekKey) { MutableList(5) { 0.0 } }
            for (i in 0 until 5) mins[i] += durMin * pct[i] / 100.0
        }
        return out
    }

    /** This ISO week's key, for looking up [weeklyZoneMinutes]'s map. */
    fun currentWeekKey(zone: ZoneId = ZoneId.systemDefault()): Long {
        val weekFields = WeekFields.of(Locale.getDefault())
        val today = Instant.now().atZone(zone).toLocalDate()
        val weekStart = today.minusDays((today.get(weekFields.dayOfWeek()) - 1).toLong())
        return weekStart.toEpochDay()
    }
}

/** Local zone-percent parser (mirrors [com.sennnen.mav.ui.parseZonePercents] but avoids a UI-layer
 *  dependency from the data package). Tolerates both "z1".."z5" and "zone1".."zone5" keys. */
private val ZONE_KEY = Regex("\"z(?:one)?([1-5])\"\\s*:\\s*(-?[0-9]+(?:\\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)")
private fun parseZonePercentsForTargets(zonesJSON: String?): List<Double>? {
    if (zonesJSON.isNullOrBlank()) return null
    val out = MutableList(5) { 0.0 }
    var any = false
    for (m in ZONE_KEY.findAll(zonesJSON)) {
        val v = m.groupValues[2].toDoubleOrNull() ?: continue
        out[m.groupValues[1].toInt() - 1] = v.coerceIn(0.0, 100.0)
        any = true
    }
    return if (any && out.sum() > 0.0) out else null
}
