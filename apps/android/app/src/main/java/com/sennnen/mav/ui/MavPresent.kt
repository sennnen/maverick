package com.sennnen.mav.ui

import java.util.Locale

/** A live sample older than this while the link is up is presented as stale (PL-P5/PL-P7). */
const val FRESH_SAMPLE_MS = 15_000L

/**
 * The stale-data label for the live surface, from the snapshot's own observation time — the
 * platform formats age, it never decides freshness semantics beyond this display threshold.
 * Null means nothing to show: a fresh streaming sample, or no samples and no link.
 */
fun sampleAgeLabel(asOfUnixMs: Long, lastSampleUnixMs: Long?, connected: Boolean): String? {
    if (lastSampleUnixMs == null) return if (connected) "Waiting for first sample" else null
    val ageMs = (asOfUnixMs - lastSampleUnixMs).coerceAtLeast(0)
    if (connected && ageMs <= FRESH_SAMPLE_MS) return null
    return "Last sample ${relativeAge(ageMs)}"
}

private fun relativeAge(ageMs: Long): String = when {
    ageMs < 60_000 -> "${ageMs / 1_000} s ago"
    ageMs < 3_600_000 -> "${ageMs / 60_000} m ago"
    ageMs < 86_400_000 -> "${ageMs / 3_600_000} h ago"
    else -> "${ageMs / 86_400_000} d ago"
}

/** Fixed-point micros → "67.5 ms". Display formatting only; the value stays the core's. */
fun microsAsMs(micros: Long, locale: Locale = Locale.getDefault()): String =
    String.format(locale, "%.1f ms", micros / 1_000.0)

/** Fixed-point milli-percent → "50.0%". */
fun milliPercentAsPercent(milliPercent: Long, locale: Locale = Locale.getDefault()): String =
    String.format(locale, "%.1f%%", milliPercent / 1_000.0)
