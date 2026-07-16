package com.sennnen.mav

import org.json.JSONObject

data class MavSnapshot(
    val coreVersion: String,
    val storageSchema: Int,
    val revision: ULong,
    val connectionState: String,
    val deviceName: String?,
    val currentBpm: Int?,
    val hash: String,
)

object MavSnapshotDecoder {
    fun decode(json: String, hash: String): MavSnapshot {
        val root = JSONObject(json)
        val schema = root.getString("schema")
        require(schema == "host-snapshot/v1") { "unsupported snapshot schema: $schema" }
        val connection = root.getJSONObject("connection")
        val session = root.optJSONObject("session")
        return MavSnapshot(
            coreVersion = root.getString("core_version"),
            storageSchema = root.getInt("storage_schema"),
            revision = root.getLong("revision").toULong(),
            connectionState = connection.getString("state"),
            deviceName = connection.optNullableString("display_name"),
            currentBpm = session?.optNullableInt("current_bpm"),
            hash = hash,
        )
    }
}

private fun JSONObject.optNullableString(name: String): String? =
    if (isNull(name)) null else getString(name)

private fun JSONObject.optNullableInt(name: String): Int? =
    if (isNull(name)) null else getInt(name)
