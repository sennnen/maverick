package com.sennnen.mav

import org.json.JSONObject

data class MavSnapshot(
    val coreVersion: String,
    val storageSchema: Int,
    val revision: ULong,
    val connectionState: String,
    val deviceName: String?,
    val currentBpm: Int?,
    val recoveryUnavailableReason: String?,
    val hash: String,
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
            connectionState = connection.getString("state"),
            deviceName = connection.optNullableString("display_name"),
            currentBpm = session?.optNullableInt("current_bpm"),
            recoveryUnavailableReason = analytics?.recoveryUnavailableReason(),
            hash = hash,
        )
    }
}

private fun JSONObject.recoveryUnavailableReason(): String? {
    val availability = optJSONArray("availability") ?: return null
    for (index in 0 until availability.length()) {
        val item = availability.getJSONObject(index)
        if (item.getString("analytic") != "recovery" || item.getBoolean("available")) continue
        val reason = item.optJSONObject("reason") ?: return "Unavailable"
        return when (reason.getString("kind")) {
            "algorithm_not_admitted" -> "Recovery model not admitted"
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
