package com.sennnen.mav.ui.mav

import com.sennnen.mav.BuildConfig
import uniffi.mav_ffi.AnalyticAvailabilityReport
import uniffi.mav_ffi.DailySnapshotReport
import java.util.Locale

// The metric catalogue and the mapping from the core's read models to what a screen draws. The iOS
// twin is Model/MavMetric.swift and the two must agree, because a metric that is unavailable on one
// platform and drawn on the other is a parity bug with a person's health data in it.
//
// This file computes nothing. Every number, band, and reason below is lifted out of a
// DailySnapshotReport exactly as the core produced it. What it owns is presentation: which metrics
// the UI knows how to lay out, what each is called, and how a value is formatted.

enum class MavMetricGroup(val title: String) {
    SCORES("Scores"),
    CYCLE("Cycle"),
    VITALS("Vitals"),
}

/**
 * A metric the UI knows how to draw. [analytic] is the core's own id, and matching against it is
 * the only link between this catalogue and the availability set.
 */
data class MavMetric(
    val id: String,
    val analytic: String?,
    val displayName: String,
    val family: MavFamily,
    val group: MavMetricGroup,
    val unit: String?,
    /**
     * What the score rail calls it. A gauge is 74dp wide and "Resting heart rate" ellipsises to
     * nonsense in that space, so every metric carries a name that fits rather than one that gets
     * cut. Defaults to the full name where it already fits.
     */
    val shortName: String = displayName,
) {
    companion object {
        val catalogue: List<MavMetric> = listOf(
            MavMetric("recovery", "recovery", "Recovery", MavFamily.CHARGE, MavMetricGroup.SCORES, "%"),
            MavMetric("sleep", "sleep_performance", "Sleep", MavFamily.REST, MavMetricGroup.SCORES, "%"),
            MavMetric("effort", "daily_effort", "Activity", MavFamily.EFFORT, MavMetricGroup.SCORES, null),
            MavMetric("variability", "time_domain_hrv", "Variability", MavFamily.CHARGE, MavMetricGroup.VITALS, "ms"),
            MavMetric("heart_rate", null, "Heart rate", MavFamily.HEART, MavMetricGroup.VITALS, "bpm"),
            MavMetric("respiration", "respiration_rate", "Respiratory rate", MavFamily.VITALS, MavMetricGroup.VITALS, "brpm", "Respiration"),
            MavMetric("blood_oxygen", "blood_oxygen", "Blood oxygen", MavFamily.VITALS, MavMetricGroup.VITALS, "%", "Blood O₂"),
            MavMetric("skin_temperature", "skin_temperature", "Skin temperature", MavFamily.ENERGY, MavMetricGroup.VITALS, "°C", "Skin temp"),
            MavMetric("illness_risk", "illness_risk", "Illness signals", MavFamily.VITALS, MavMetricGroup.VITALS, null, "Illness"),
            MavMetric("cycle_phase", "cycle_phase", "Cycle phase", MavFamily.CYCLE, MavMetricGroup.CYCLE, null, "Cycle"),
        )

        fun named(id: String): MavMetric? = catalogue.firstOrNull { it.id == id }
    }
}

/**
 * The core's own normal range for a metric, plus where today sits inside it. The track is padded
 * 25% either side of the band so a value outside the range is still visible rather than clamped
 * onto an end cap.
 */
data class MavBand(val low: Double, val high: Double, val value: Double) {
    val markerFraction: Double
        get() {
            val span = high - low
            if (span <= 0) return 0.5
            val padded = span * 0.25
            val lo = low - padded
            val hi = high + padded
            return ((value - lo) / (hi - lo)).coerceIn(0.0, 1.0)
        }

    val lowFraction: Double get() = if (high - low <= 0) 0.0 else 0.25 / 1.5
    val highFraction: Double get() = if (high - low <= 0) 1.0 else 1 - 0.25 / 1.5
}

/** What a Vitals row draws: either the core produced a value, or it said why it could not. */
sealed interface MavMetricState {
    data class Value(
        val text: String,
        val numeric: Double,
        val band: MavBand?,
        val status: MavStatus,
        val word: String,
    ) : MavMetricState

    data class Unavailable(val reason: String) : MavMetricState
}

data class MavMetricRow(val metric: MavMetric, val state: MavMetricState) {
    val isAvailable: Boolean get() = state is MavMetricState.Value
}

/** One gauge on Today's rail. [fraction] is null when there is nothing to fill it with. */
data class MavRailItem(val metric: MavMetric, val text: String, val fraction: Double?)

object MavMetricMapper {

    /**
     * Availability keys reach the app in two spellings: the host snapshot's JSON uses
     * `time_domain_hrv`, while the FFI report derives its string from a Rust `Debug` and produces
     * `timedomainhrv`. Normalising is a presentation tolerance, not a computation - the app still
     * reads whatever the core said, it just accepts both spellings of the same id.
     */
    fun normalise(key: String): String = key.lowercase(Locale.ROOT).replace("_", "")

    fun availability(reports: List<AnalyticAvailabilityReport>, analytic: String): AnalyticAvailabilityReport? {
        val wanted = normalise(analytic)
        return reports.firstOrNull { normalise(it.analytic) == wanted }
    }

    /** The core's reason, worded for a person. The kind and the stream names are the core's. */
    fun reasonText(report: AnalyticAvailabilityReport?, metric: MavMetric): String {
        if (report == null) {
            return "${metric.displayName} is not something this core version reports."
        }
        return when (report.reason) {
            "missing_streams" -> {
                val streams = report.missingStreams.joinToString(", ") { streamName(it) }
                if (streams.isEmpty()) {
                    "Waiting on a signal your strap has not sent yet."
                } else {
                    "Waiting on $streams from your strap."
                }
            }
            "algorithm_not_admitted" ->
                "Not published yet — the calculation has no reference we can stand behind."
            null -> "Unavailable."
            else -> "Unavailable: ${report.reason}."
        }
    }

    /** Stream ids are the core's vocabulary; these are the same streams said out loud. */
    fun streamName(stream: String): String = when (normalise(stream)) {
        "heartrate" -> "heart rate"
        "rrinterval" -> "electrical beat intervals"
        "pulseinterval" -> "optical beat intervals"
        "skintemperature", "skintemperatureraw" -> "skin temperature"
        "spo2", "bloodoxygen" -> "blood oxygen"
        "respiration", "respirationrate" -> "respiration"
        "sleepstage", "sleepstate" -> "sleep staging"
        "accelerometer", "imu" -> "motion"
        else -> stream.replace("_", " ").lowercase(Locale.ROOT)
    }

    /** The whole Vitals list for a day, in catalogue order, with every metric accounted for. */
    fun rows(snapshot: DailySnapshotReport?, cycleEnabled: Boolean): List<MavMetricRow> =
        MavMetric.catalogue.mapNotNull { metric ->
            // Cycle follows the body profile automatically. There is no second settings toggle.
            if (metric.group == MavMetricGroup.CYCLE && !cycleEnabled) {
                null
            } else {
                MavMetricRow(metric, state(metric, snapshot))
            }
        }

    fun state(metric: MavMetric, snapshot: DailySnapshotReport?): MavMetricState {
        if (snapshot == null) return MavMetricState.Unavailable("No day loaded yet.")

        if (BuildConfig.MAV_SHOW_SYNTHETIC_DATA && snapshot.snapshotHash.startsWith("fixture-")) {
            debugState(metric, snapshot)?.let { return it }
        }

        val report = metric.analytic?.let { availability(snapshot.availability, it) }

        // Variability is the one metric the core fully produces today: a value, and a band from the
        // readiness baseline. Everything else is honestly unavailable until its analytic is admitted.
        val hrv = snapshot.hrv
        if (metric.id == "variability" && report?.available == true && hrv != null) {
            val readiness = snapshot.readiness
            val band = readiness?.let { MavBand(it.normalLowMs, it.normalHighMs, hrv.rmssdMs) }
            val tier = readiness?.tier
            return MavMetricState.Value(
                text = decimal(hrv.rmssdMs, 0),
                numeric = hrv.rmssdMs,
                band = band,
                status = status(tier),
                word = word(tier, hrv.label),
            )
        }

        val meanBpm = snapshot.meanBpm
        if (metric.id == "heart_rate" && snapshot.hrSampleCount > 0u && meanBpm != null) {
            return MavMetricState.Value(
                text = decimal(meanBpm, 0),
                numeric = meanBpm,
                band = null,
                status = MavStatus.NEUTRAL,
                word = "Recorded",
            )
        }

        if (report?.available == true) {
            // Available but this build has no extractor. Say so rather than drawing an empty row;
            // a blank number is the one thing that must never reach a screen.
            return MavMetricState.Unavailable(
                "${metric.displayName} is available but this app build cannot read it yet.",
            )
        }

        return MavMetricState.Unavailable(reasonText(report, metric))
    }

    private fun debugState(metric: MavMetric, snapshot: DailySnapshotReport): MavMetricState.Value? {
        val hrv = snapshot.hrv
        return when (metric.id) {
            "recovery" -> sampleValue("82", 82.0, 65.0, 88.0, MavStatus.OPTIMAL, "Optimal")
            "sleep" -> sampleValue("78", 78.0, 72.0, 90.0, MavStatus.FAIR, "Fair")
            "effort" -> sampleValue("11.6", 11.6, 8.0, 15.0, MavStatus.OPTIMAL, "Balanced")
            "variability" -> hrv?.let {
                val readiness = snapshot.readiness
                MavMetricState.Value(
                    decimal(it.rmssdMs, 0),
                    it.rmssdMs,
                    readiness?.let { r -> MavBand(r.normalLowMs, r.normalHighMs, it.rmssdMs) },
                    status(readiness?.tier),
                    word(readiness?.tier, it.label),
                )
            }
            "heart_rate" -> sampleValue("68", 68.0, 58.0, 74.0, MavStatus.OPTIMAL, "In range")
            "respiration" -> sampleValue("14.2", 14.2, 12.4, 15.8, MavStatus.OPTIMAL, "In range")
            "blood_oxygen" -> sampleValue("97", 97.0, 95.0, 100.0, MavStatus.OPTIMAL, "Optimal")
            "skin_temperature" -> sampleValue("+0.1", 0.1, -0.3, 0.4, MavStatus.OPTIMAL, "Stable")
            "illness_risk" -> MavMetricState.Value(
                "Low", 0.12, null, MavStatus.OPTIMAL, "No change",
            )
            "cycle_phase" -> MavMetricState.Value(
                "Day 15", 15.0, null, MavStatus.NEUTRAL, "Follicular",
            )
            else -> null
        }
    }

    private fun sampleValue(
        text: String,
        value: Double,
        low: Double,
        high: Double,
        status: MavStatus,
        word: String,
    ) = MavMetricState.Value(text, value, MavBand(low, high, value), status, word)

    /** The readiness tier is the core's judgement, mapped onto what a surface tint can express. */
    fun status(tier: String?): MavStatus = when (tier) {
        "primed", "normal" -> MavStatus.OPTIMAL
        "suppressed" -> MavStatus.LOW
        else -> MavStatus.NEUTRAL
    }

    /** The status word comes from the core's tier, never from the tint. */
    fun word(tier: String?, label: String?): String = when (tier) {
        "primed" -> "Primed"
        "normal" -> "In range"
        "suppressed" -> "Suppressed"
        else -> if (label == "heart_rate_variability") "Measured" else "Provisional"
    }

    /**
     * What the core is willing to call a variability figure. Only beats timed from the heart's
     * electrical signal are HRV; an optical pulse is a different event and reads as PRV.
     */
    fun variabilityTitle(label: String?): String =
        if (label == "heart_rate_variability") "Heart-rate variability" else "Pulse-rate variability"

    fun decimal(value: Double, places: Int): String =
        String.format(Locale.getDefault(), "%.${places}f", value)

    /** An unavailable score is an em dash and a dashed arc — never a zero, which is a claim. */
    fun rail(rows: List<MavMetricRow>): List<MavRailItem> = rows.map { row ->
        when (val state = row.state) {
            is MavMetricState.Value -> MavRailItem(
                row.metric,
                state.text,
                when (row.metric.id) {
                    "recovery", "sleep", "blood_oxygen" -> state.numeric / 100.0
                    "effort" -> state.numeric / 21.0
                    "cycle_phase" -> state.numeric / 30.0
                    else -> state.band?.markerFraction ?: 0.66
                }.coerceIn(0.0, 1.0),
            )
            is MavMetricState.Unavailable -> MavRailItem(row.metric, "—", null)
        }
    }
}
