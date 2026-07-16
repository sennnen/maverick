package com.sennnen.mav.ui

import com.sennnen.mav.data.WorkoutRow

// stored shapes — "zone1".."zone5" (WhoopCsvImporter.zonesJson) and "z1".."z5" (the macOS
// importer's rows) — so an anchored regex is safe, and it keeps org.json (an unmocked
// Android stub in plain-JVM unit tests) out of test-reachable code.

internal val ZONE_KEY = Regex("\"z(?:one)?([1-5])\"\\s*:\\s*(-?[0-9]+(?:\\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)")

/** Zone percentages (0–100) indexed Z1..Z5, or null when the row has no usable zone data. */
internal fun parseZonePercents(zonesJSON: String?): List<Double>? {
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

internal data class ZoneSummary(val minutes: List<Double>, val sessionsWithZones: Int) {
    val totalMinutes: Double get() = minutes.sum()
}

/** Duration-weighted zone minutes across [rows] — mirrors the macOS WorkoutZones.summary
 *  (duration-minutes × pct ÷ 100). APPROXIMATE: an on-device aggregate of imported
 *  per-workout percentages, not a WHOOP-computed figure. */
internal fun zoneSummary(rows: List<WorkoutRow>): ZoneSummary? {
    val mins = MutableList(5) { 0.0 }
    var n = 0
    for (r in rows) {
        val p = parseZonePercents(r.zonesJSON) ?: continue
        val durMin = (r.durationS ?: (r.endTs - r.startTs).toDouble()) / 60.0
        if (durMin <= 0.0) continue
        for (i in 0 until 5) mins[i] += durMin * p[i] / 100.0
        n++
    }
    return if (n > 0 && mins.sum() > 0.0) ZoneSummary(mins, n) else null
}

