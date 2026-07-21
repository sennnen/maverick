package com.sennnen.mav.connector

import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.security.MessageDigest
import uniffi.mav_ffi.ConnectorSourceKind
import uniffi.mav_ffi.ConnectorSourceMetadata
import uniffi.mav_ffi.ConnectorTransportRequest

enum class ConnectorImportOrigin {
    LOCAL,
    CONTENT,
    SHARE,
    REMOTE,
}

sealed class ConnectorAcquisitionException(message: String) : Exception(message) {
    class Empty : ConnectorAcquisitionException("The connector file is empty.")
    class TooLarge : ConnectorAcquisitionException("The connector is larger than the 4 MB safety limit.")
    class UnsupportedUri : ConnectorAcquisitionException("Use a local document, content URI, or HTTPS URL.")
    class RemoteDisabled : ConnectorAcquisitionException("Remote connector import is disabled in this release.")
    class InvalidResponse : ConnectorAcquisitionException("The connector download returned an invalid response.")
}

data class ConnectorAcquisition(
    val bytes: ByteArray,
    val source: ConnectorSourceMetadata,
) {
    companion object {
        const val MAXIMUM_BYTES = 4 * 1_024 * 1_024

        fun make(
            bytes: ByteArray,
            origin: ConnectorImportOrigin,
            displayName: String,
            locator: String,
        ): ConnectorAcquisition {
            if (bytes.isEmpty()) throw ConnectorAcquisitionException.Empty()
            if (bytes.size > MAXIMUM_BYTES) throw ConnectorAcquisitionException.TooLarge()
            val safeName = displayName.replace('\\', '/').substringAfterLast('/').ifBlank { "Connector" }
            val kind = when (origin) {
                ConnectorImportOrigin.REMOTE -> ConnectorSourceKind.REMOTE
                else -> ConnectorSourceKind.IMPORTED
            }
            return ConnectorAcquisition(
                bytes = bytes.copyOf(),
                source = ConnectorSourceMetadata(
                    kind = kind,
                    displayName = safeName,
                    locatorDigest = MessageDigest.getInstance("SHA-256").digest(locator.toByteArray()),
                ),
            )
        }
    }
}

object BoundedConnectorReader {
    fun read(input: InputStream): ByteArray {
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(16 * 1_024)
        while (true) {
            val count = input.read(buffer)
            if (count < 0) break
            if (count == 0) continue
            if (output.size() + count > ConnectorAcquisition.MAXIMUM_BYTES) {
                throw ConnectorAcquisitionException.TooLarge()
            }
            output.write(buffer, 0, count)
        }
        return output.toByteArray()
    }
}

data class ConnectorApprovalSummary(
    val connectorId: String,
    val version: String,
    val displayName: String,
    val publisherKeyId: String,
    val fixtureCount: UInt,
    val detail: String = "",
    val sourceName: String = "",
    val capabilities: List<String> = emptyList(),
    val permissions: List<String> = emptyList(),
)

sealed interface ConnectorApprovalPhase {
    data object Idle : ConnectorApprovalPhase
    data object Inspecting : ConnectorApprovalPhase
    data class AwaitingApproval(val summary: ConnectorApprovalSummary) : ConnectorApprovalPhase
    data class Installing(val summary: ConnectorApprovalSummary) : ConnectorApprovalPhase
    data class Installed(val connectorId: String) : ConnectorApprovalPhase
    data class Failed(val message: String) : ConnectorApprovalPhase
    data class RolledBack(val connectorId: String) : ConnectorApprovalPhase
    data class Revoked(val connectorId: String) : ConnectorApprovalPhase
}

sealed class ConnectorApprovalException(message: String) : Exception(message) {
    class InspectionRequired : ConnectorApprovalException("Inspect this connector before approving it.")
}

class ConnectorApprovalMachine {
    var phase: ConnectorApprovalPhase = ConnectorApprovalPhase.Idle
        private set
    var pendingBytes: ByteArray? = null
        private set

    fun beginInspection() {
        pendingBytes = null
        phase = ConnectorApprovalPhase.Inspecting
    }

    fun inspectionSucceeded(summary: ConnectorApprovalSummary, artifactBytes: ByteArray) {
        pendingBytes = artifactBytes.copyOf()
        phase = ConnectorApprovalPhase.AwaitingApproval(summary)
    }

    fun beginApproval() {
        val current = phase as? ConnectorApprovalPhase.AwaitingApproval
            ?: throw ConnectorApprovalException.InspectionRequired()
        if (pendingBytes == null) throw ConnectorApprovalException.InspectionRequired()
        phase = ConnectorApprovalPhase.Installing(current.summary)
    }

    fun installed(connectorId: String) {
        pendingBytes = null
        phase = ConnectorApprovalPhase.Installed(connectorId)
    }

    fun cancel() {
        pendingBytes = null
        phase = ConnectorApprovalPhase.Idle
    }

    fun fail(message: String) {
        pendingBytes = null
        phase = ConnectorApprovalPhase.Failed(message)
    }

    fun rolledBack(connectorId: String) {
        pendingBytes = null
        phase = ConnectorApprovalPhase.RolledBack(connectorId)
    }

    fun revoked(connectorId: String) {
        pendingBytes = null
        phase = ConnectorApprovalPhase.Revoked(connectorId)
    }
}

sealed interface ConnectorNativeOperation {
    data class Scan(val serviceUuids: List<String>, val manufacturerIds: List<UShort>) : ConnectorNativeOperation
    data object StopScan : ConnectorNativeOperation
    data class Connect(val address: String) : ConnectorNativeOperation
    data object EnsurePaired : ConnectorNativeOperation
    data object DiscoverServices : ConnectorNativeOperation
    data class Subscribe(val id: String, val service: String, val characteristic: String) : ConnectorNativeOperation
    data class Unsubscribe(val id: String, val service: String, val characteristic: String) : ConnectorNativeOperation
    data class Read(val id: String, val service: String, val characteristic: String) : ConnectorNativeOperation
    data class Write(
        val id: String,
        val service: String,
        val characteristic: String,
        val bytes: ByteArray,
        val confirmed: Boolean,
    ) : ConnectorNativeOperation
    data object Disconnect : ConnectorNativeOperation
    data class SetTimer(val token: ULong, val delayMs: ULong) : ConnectorNativeOperation
    data class CancelTimer(val token: ULong) : ConnectorNativeOperation

    companion object {
        fun map(request: ConnectorTransportRequest): ConnectorNativeOperation = when (request) {
            is ConnectorTransportRequest.StartScan -> Scan(request.serviceUuids, request.manufacturerIds)
            ConnectorTransportRequest.StopScan -> StopScan
            is ConnectorTransportRequest.Connect -> Connect(request.address)
            ConnectorTransportRequest.EnsurePaired -> EnsurePaired
            ConnectorTransportRequest.DiscoverServices -> DiscoverServices
            is ConnectorTransportRequest.Subscribe -> Subscribe(
                request.characteristicId, request.serviceUuid, request.characteristicUuid,
            )
            is ConnectorTransportRequest.Unsubscribe -> Unsubscribe(
                request.characteristicId, request.serviceUuid, request.characteristicUuid,
            )
            is ConnectorTransportRequest.Read -> Read(
                request.characteristicId, request.serviceUuid, request.characteristicUuid,
            )
            is ConnectorTransportRequest.Write -> Write(
                request.characteristicId,
                request.serviceUuid,
                request.characteristicUuid,
                request.bytes,
                request.confirmed,
            )
            ConnectorTransportRequest.Disconnect -> Disconnect
            is ConnectorTransportRequest.SetTimer -> SetTimer(request.token, request.delayMs)
            is ConnectorTransportRequest.CancelTimer -> CancelTimer(request.token)
        }
    }
}

data class ConnectorRestorationCheckpoint(
    val connectorId: String,
    val sessionId: ULong,
    val cancellationGeneration: ULong,
) {
    fun encode(): String = "$connectorId|$sessionId|$cancellationGeneration"

    companion object {
        fun decode(value: String): ConnectorRestorationCheckpoint? {
            val fields = value.split('|')
            if (fields.size != 3 || fields[0].isBlank()) return null
            return ConnectorRestorationCheckpoint(
                connectorId = fields[0],
                sessionId = fields[1].toULongOrNull() ?: return null,
                cancellationGeneration = fields[2].toULongOrNull() ?: return null,
            )
        }
    }
}
