package com.sennnen.mav.ui.aura

import com.sennnen.mav.data.DailyMetric
import com.sennnen.mav.ui.lastVitalsRow
import com.sennnen.mav.ui.logicalDayKeyNow
import com.sennnen.mav.ui.widgetAnchorRow
import java.time.LocalDate
import kotlin.math.roundToInt

// Shared hub data helpers — the Android twins of the small Repository helpers
// the iOS Aura hubs lean on (widgetAnchor / lastVitalsDay / 21-day baselines).

/** The day row every hub anchors on (recovery-scored today, else the freshest prior scored day). */
fun auraAnchorDay(days: List<DailyMetric>): DailyMetric? =
    widgetAnchorRow(days, logicalDayKeyNow(), LocalDate.now().toString())

/** The freshest strictly-prior row carrying a real overnight vital (HRV / RHR / resp). */
fun auraLastVitalsDay(days: List<DailyMetric>, anchor: DailyMetric?): DailyMetric? =
    lastVitalsRow(days, anchor?.day ?: LocalDate.now().toString())

/** Day-keyed history for one metric (oldest → newest), nulls dropped. */
fun auraPoints(days: List<DailyMetric>, selector: (DailyMetric) -> Double?): List<AuraPoint> =
    days.mapNotNull { d -> selector(d)?.let { AuraPoint(d.day, it) } }

/** Trailing 21-day mean, excluding the newest point (the iOS baselineOf). */
fun auraBaselineOf(points: List<AuraPoint>): Double? {
    val v = points.dropLast(1).takeLast(21).map { it.value }
    return if (v.isEmpty()) null else v.sum() / v.size
}

/** Fractional deviation of value vs baseline ((v-b)/b), null-safe. */
fun auraFrac(v: Double?, b: Double?): Double? {
    if (v == null || b == null || b == 0.0) return null
    return (v - b) / b
}

/**
 * What a variability figure may be called on screen, decided by the core's own label.
 *
 * Only beats timed from the heart's electrical signal are heart-rate variability; an optical pulse
 * is a different event and reads as PRV. Every surface asks this rather than deciding for itself,
 * so the app cannot title the same number two ways on two tabs.
 */
fun auraVariabilityTitle(label: String?): String =
    if (label == "heart_rate_variability") "HRV" else "PRV"

// MARK: Formatting (the hub screens' shared text helpers)

fun auraIntText(v: Double?): String = v?.roundToInt()?.toString() ?: "--"

fun auraDecText(v: Double?, decimals: Int): String =
    v?.let { String.format(java.util.Locale.US, "%.${decimals}f", it) } ?: "--"

fun auraHmText(m: Double?): String {
    if (m == null || m <= 0) return "--"
    val t = m.roundToInt()
    return "${t / 60}h ${t % 60}m"
}

fun auraSignedText(v: Double?, decimals: Int = 1): String {
    if (v == null) return "--"
    val s = String.format(java.util.Locale.US, "%.${decimals}f", v)
    return if (v > 0) "+$s" else s
}
