package com.sennnen.mav

import org.json.JSONObject

data class MavSnapshot(
    val coreVersion: String,
    val storageSchema: Int,
    val revision: ULong,
    val asOfUnixMs: Long,
    val connectionState: String,
    val deviceName: String?,
    val batteryPercent: Int?,
    val charging: Boolean?,
    val lastSampleUnixMs: Long?,
    val currentBpm: Int?,
    val meanMilliBpm: Int?,
    val inRangeSamples: Int?,
    val excludedSamples: Int?,
    val prv: MavPrv?,
    val prvUnavailableReason: String?,
    val recoveryUnavailableReason: String?,
    val hash: String,
)

/**
 * The admitted time-domain variability read model. WHOOP intervals are PPG-derived, so the core
 * labels the result `pulse_rate_variability`; it is presented as PRV, never as ECG HRV
 * (docs/analytics.md).
 */
data class MavPrv(
    val label: String,
    val intervalSource: String,
    val meanIntervalMicros: Long,
    val rmssdMicros: Long,
    val sdnnMicros: Long,
    val nn50Count: Int,
    val pnn50MilliPercent: Long,
    val intervalCount: Int,
    val excludedIntervalCount: Int,
    val algorithm: String,
    val algorithmVersion: String,
    val provenanceId: Long,
)

object MavSnapshotDecoder {
    fun decode(json: String, hash: String): MavSnapshot {
        val root = JSONObject(json)
        val schema = root.getString("schema")
        require(schema == "host-snapshot/v1") { "unsupported snapshot schema: $schema" }
        val connection = root.getJSONObject("connection")
        val session = root.optJSONObject("session")
        val analytics = root.optJSONObject("analytics")
        return MavSnapshot(
            coreVersion = root.getString("core_version"),
            storageSchema = root.getInt("storage_schema"),
            revision = root.getLong("revision").toULong(),
            asOfUnixMs = root.getLong("as_of_unix_ms"),
            connectionState = connection.getString("state"),
            deviceName = connection.optNullableString("display_name"),
            batteryPercent = connection.optNullableInt("battery_percent"),
            charging = connection.optNullableBoolean("charging"),
            lastSampleUnixMs = connection.optNullableLong("last_sample_unix_ms"),
            currentBpm = session?.optNullableInt("current_bpm"),
            meanMilliBpm = session?.optNullableInt("mean_milli_bpm"),
            inRangeSamples = session?.getInt("in_range_samples"),
            excludedSamples = session?.getInt("excluded_samples"),
            prv = analytics?.let(::decodePrv),
            prvUnavailableReason = analytics?.unavailableReason("time_domain_hrv", "PRV"),
            recoveryUnavailableReason = analytics?.unavailableReason("recovery", "Recovery"),
            hash = hash,
        )
    }

    private fun decodePrv(analytics: JSONObject): MavPrv? {
        if (analytics.isNull("variability_label")) return null
        return MavPrv(
            label = analytics.getString("variability_label"),
            intervalSource = analytics.getString("interval_source"),
            meanIntervalMicros = analytics.getLong("mean_interval_micros"),
            rmssdMicros = analytics.getLong("rmssd_micros"),
            sdnnMicros = analytics.getLong("sdnn_micros"),
            nn50Count = analytics.getInt("nn50_count"),
            pnn50MilliPercent = analytics.getLong("pnn50_milli_percent"),
            intervalCount = analytics.getInt("interval_count"),
            excludedIntervalCount = analytics.getInt("excluded_interval_count"),
            algorithm = analytics.getString("algorithm"),
            algorithmVersion = analytics.getString("algorithm_version"),
            provenanceId = analytics.getLong("provenance_id"),
        )
    }
}

private fun JSONObject.unavailableReason(analytic: String, displayName: String): String? {
    val availability = optJSONArray("availability") ?: return null
    for (index in 0 until availability.length()) {
        val item = availability.getJSONObject(index)
        if (item.getString("analytic") != analytic || item.getBoolean("available")) continue
        val reason = item.optJSONObject("reason") ?: return "Unavailable"
        return when (reason.getString("kind")) {
            "algorithm_not_admitted" -> "$displayName model not admitted"
            "missing_streams" -> {
                val streams = reason.getJSONArray("streams")
                val names = (0 until streams.length()).joinToString { streams.getString(it) }
                "Needs $names"
            }
            else -> "Unavailable"
        }
    }
    return null
}

private fun JSONObject.optNullableString(name: String): String? =
    if (isNull(name)) null else getString(name)

private fun JSONObject.optNullableInt(name: String): Int? =
    if (isNull(name)) null else getInt(name)

private fun JSONObject.optNullableLong(name: String): Long? =
    if (isNull(name)) null else getLong(name)

private fun JSONObject.optNullableBoolean(name: String): Boolean? =
    if (isNull(name)) null else getBoolean(name)
