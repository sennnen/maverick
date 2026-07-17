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

/**
 * The history-sync progress line, from the core's `historical-status/v1` read model. Display
 * words only — the state names, counts, and failure code are the core's, and an unknown state
 * shows nothing rather than a guess.
 */
fun syncProgressLabel(
    state: String,
    recordsSeen: Long,
    recordsInserted: Long,
    duplicates: Long,
    failureCode: Int?,
): String? = when (state) {
    "historical_awaiting_range", "historical_awaiting_send_acceptance" -> "Preparing history sync"
    "historical_receiving", "historical_awaiting_durable_commit" -> when {
        recordsSeen == 0L -> "Syncing history"
        recordsSeen == 1L -> "Syncing history — 1 record"
        else -> "Syncing history — $recordsSeen records"
    }
    "historical_complete" ->
        if (recordsInserted == 0L && duplicates == 0L) {
            "History synced"
        } else {
            "History synced — $recordsInserted new, $duplicates duplicate"
        }
    "historical_failed" ->
        if (failureCode == null) "History sync failed" else "History sync failed (MAV-$failureCode)"
    else -> null
}

/** Fixed-point micros → "67.5 ms". Display formatting only; the value stays the core's. */
fun microsAsMs(micros: Long, locale: Locale = Locale.getDefault()): String =
    String.format(locale, "%.1f ms", micros / 1_000.0)

/** Fixed-point milli-percent → "50.0%". */
fun milliPercentAsPercent(milliPercent: Long, locale: Locale = Locale.getDefault()): String =
    String.format(locale, "%.1f%%", milliPercent / 1_000.0)
