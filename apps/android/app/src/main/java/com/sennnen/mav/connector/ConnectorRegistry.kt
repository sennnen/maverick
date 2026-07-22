package com.sennnen.mav.connector

import android.content.Context
import android.util.Base64
import com.sennnen.mav.BuildConfig
import java.io.ByteArrayOutputStream
import java.io.InputStream
import org.json.JSONArray
import org.json.JSONObject
import uniffi.mav_ffi.ConnectorRegistryCheckpoint
import uniffi.mav_ffi.ConnectorRegistryRoot
import uniffi.mav_ffi.ConnectorRevocationRecord
import uniffi.mav_ffi.ConnectorKeyScope
import uniffi.mav_ffi.ConnectorKeyStatus
import uniffi.mav_ffi.ConnectorPublisherKey

internal data class AndroidRegistryConfiguration(
    val url: String,
    val root: ConnectorRegistryRoot,
) {
    companion object {
        fun current(): AndroidRegistryConfiguration? {
            val url = BuildConfig.MAV_CONNECTOR_REGISTRY_URL
            val registryId = BuildConfig.MAV_CONNECTOR_REGISTRY_ID
            val keyId = BuildConfig.MAV_CONNECTOR_REGISTRY_ROOT_KEY_ID
            val publicKey = BuildConfig.MAV_CONNECTOR_REGISTRY_ROOT_PUBLIC_KEY_HEX.decodeHex()
            if (!url.startsWith("https://") || registryId.isBlank() || keyId.isBlank() || publicKey?.size != 32) {
                return null
            }
            return AndroidRegistryConfiguration(
                url,
                ConnectorRegistryRoot(registryId, keyId, publicKey),
            )
        }
    }
}

internal object AndroidConnectorTrust {
    fun configuredKeys(): List<ConnectorPublisherKey> {
        val id = BuildConfig.MAV_CONNECTOR_PUBLISHER_KEY_ID
        val publicKey = BuildConfig.MAV_CONNECTOR_PUBLISHER_PUBLIC_KEY_HEX.decodeHex()
        val keys = mutableListOf<ConnectorPublisherKey>()
        if (id.isNotBlank() && publicKey?.size == 32) {
            keys += ConnectorPublisherKey(
                id = id,
                publicKey = publicKey,
                scope = ConnectorKeyScope.DEVELOPMENT,
                validFromMs = 0,
                validUntilMs = null,
                status = ConnectorKeyStatus.ACTIVE,
                statusAtMs = null,
                statusDetail = null,
            )
        }
        if (BuildConfig.DEBUG) {
            // Local dev-loop key: original test signer's private key isn't recoverable on this
            // machine. Swapped to a throwaway Ed25519 keypair generated for this session so
            // freshly rebuilt whoop5 test artifacts can be sideloaded and verified again. Not a
            // production/distribution key.
            val livePublicKey =
                "04797a44551f1f41f977cae6227c867ec42dba22b4088704505aff7bfa287e4b".decodeHex()
            if (livePublicKey?.size == 32) {
                keys += ConnectorPublisherKey(
                    id = "maverick-whoop-live-test",
                    publicKey = livePublicKey,
                    scope = ConnectorKeyScope.DEVELOPMENT,
                    validFromMs = 0,
                    validUntilMs = null,
                    status = ConnectorKeyStatus.ACTIVE,
                    statusAtMs = null,
                    statusDetail = null,
                )
            }
        }
        return keys
    }
}

internal data class CachedConnectorRegistry(
    val bytes: ByteArray,
    val checkpoint: ConnectorRegistryCheckpoint,
)

internal class ConnectorRegistryCache(context: Context) {
    private val preferences = context.getSharedPreferences("connector-registry-v1", Context.MODE_PRIVATE)

    fun load(): CachedConnectorRegistry? = runCatching {
        val encoded = preferences.getString("cache", null) ?: return null
        val json = JSONObject(encoded)
        val revocations = json.getJSONArray("revocations")
        CachedConnectorRegistry(
            bytes = Base64.decode(json.getString("bytes"), Base64.NO_WRAP),
            checkpoint = ConnectorRegistryCheckpoint(
                registryId = json.getString("registry_id"),
                revision = json.getString("revision").toULong(),
                digest = Base64.decode(json.getString("digest"), Base64.NO_WRAP),
                revocationRevision = json.getString("revocation_revision").toULong(),
                revocations = (0 until revocations.length()).map { index ->
                    val entry = revocations.getJSONObject(index)
                    ConnectorRevocationRecord(
                        publisherKeyId = entry.getString("publisher_key_id"),
                        revokedAtMs = entry.getLong("revoked_at_ms"),
                        reason = entry.getString("reason"),
                    )
                },
            ),
        )
    }.getOrNull()

    fun save(bytes: ByteArray, checkpoint: ConnectorRegistryCheckpoint) {
        val revocations = JSONArray()
        checkpoint.revocations.forEach { entry ->
            revocations.put(
                JSONObject()
                    .put("publisher_key_id", entry.publisherKeyId)
                    .put("revoked_at_ms", entry.revokedAtMs)
                    .put("reason", entry.reason),
            )
        }
        val json = JSONObject()
            .put("bytes", Base64.encodeToString(bytes, Base64.NO_WRAP))
            .put("registry_id", checkpoint.registryId)
            .put("revision", checkpoint.revision.toString())
            .put("digest", Base64.encodeToString(checkpoint.digest, Base64.NO_WRAP))
            .put("revocation_revision", checkpoint.revocationRevision.toString())
            .put("revocations", revocations)
        preferences.edit().putString("cache", json.toString()).apply()
    }
}

internal object BoundedRegistryReader {
    const val MAXIMUM_BYTES = 1024 * 1024

    fun read(stream: InputStream): ByteArray {
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(16 * 1024)
        while (true) {
            val count = stream.read(buffer)
            if (count < 0) break
            if (output.size() + count > MAXIMUM_BYTES) throw ConnectorAcquisitionException.TooLarge()
            output.write(buffer, 0, count)
        }
        if (output.size() == 0) throw ConnectorAcquisitionException.Empty()
        return output.toByteArray()
    }
}

private fun String.decodeHex(): ByteArray? {
    if (length % 2 != 0 || any { it !in '0'..'9' && it !in 'a'..'f' }) return null
    return runCatching {
        ByteArray(length / 2) { index -> substring(index * 2, index * 2 + 2).toInt(16).toByte() }
    }.getOrNull()
}
