package com.sennnen.mav.connector

import java.io.ByteArrayInputStream
import java.util.UUID
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.mav_ffi.ConnectorSourceKind
import uniffi.mav_ffi.ConnectorLifecycleState
import uniffi.mav_ffi.ConnectorTelemetrySnapshot
import uniffi.mav_ffi.ConnectorTransportRequest

class ConnectorManagementTest {
    @Test
    fun `scan catalog deduplicates devices and keeps strongest first`() {
        val catalog = ConnectorScanCatalog()
        catalog.observe(ConnectorScanDevice("mg", "WHOOP MG", -70))
        catalog.observe(ConnectorScanDevice("four", "WHOOP 4.0", -45))
        catalog.observe(ConnectorScanDevice("mg", "WHOOP MG", -30))

        assertEquals(listOf("mg", "four"), catalog.devices().map { it.id })
        assertEquals(-30, catalog.devices().first().rssi)
    }

    @Test
    fun `bluetooth base uuids cross the connector boundary in canonical short form`() {
        assertEquals("180d", connectorWireUuid(UUID.fromString("0000180d-0000-1000-8000-00805f9b34fb")))
        assertEquals(
            "fd4b0001-cce1-4033-93ce-002d5875f58a",
            connectorWireUuid(UUID.fromString("fd4b0001-cce1-4033-93ce-002d5875f58a")),
        )
    }

    @Test
    fun `url local content and share preserve identical bytes with sanitized provenance`() {
        val bytes = byteArrayOf(0, 1, 2, -1)
        val inputs = listOf(
            ConnectorAcquisition.make(bytes, ConnectorImportOrigin.LOCAL, "sensor.mavconn", "/private/a"),
            ConnectorAcquisition.make(bytes, ConnectorImportOrigin.CONTENT, "sensor.mavconn", "content://secret/a"),
            ConnectorAcquisition.make(bytes, ConnectorImportOrigin.SHARE, "sensor.mavconn", "content://secret/b"),
            ConnectorAcquisition.make(bytes, ConnectorImportOrigin.REMOTE, "sensor.mavconn", "https://example.test/a"),
        )
        inputs.forEach {
            assertArrayEquals(bytes, it.bytes)
            assertEquals(32, it.source.locatorDigest.size)
            assertFalse(it.source.locatorDigest.decodeToString().contains("secret"))
        }
        assertEquals(ConnectorSourceKind.IMPORTED, inputs[0].source.kind)
        assertEquals(ConnectorSourceKind.IMPORTED, inputs[1].source.kind)
        assertEquals(ConnectorSourceKind.IMPORTED, inputs[2].source.kind)
        assertEquals(ConnectorSourceKind.REMOTE, inputs[3].source.kind)
    }

    @Test
    fun `oversized content provider is stopped at the byte bound`() {
        val bomb = ByteArrayInputStream(ByteArray(ConnectorAcquisition.MAXIMUM_BYTES + 1))
        assertThrows(ConnectorAcquisitionException.TooLarge::class.java) {
            BoundedConnectorReader.read(bomb)
        }
    }

    @Test
    fun `oversized registry response is stopped independently of artifact limit`() {
        val bomb = ByteArrayInputStream(ByteArray(BoundedRegistryReader.MAXIMUM_BYTES + 1))
        assertThrows(ConnectorAcquisitionException.TooLarge::class.java) {
            BoundedRegistryReader.read(bomb)
        }
    }

    @Test
    fun `debug release pins signed registry and official publisher`() {
        val registry = AndroidRegistryConfiguration.current()!!
        assertEquals("dev.maverick.connectors", registry.root.registryId)
        assertEquals("registry-root-v1", registry.root.keyId)
        assertEquals(32, registry.root.publicKey.size)
        val keys = AndroidConnectorTrust.configuredKeys()
        assertEquals(
            setOf("maverick-whoop-test", "maverick-whoop-live-test"),
            keys.map { it.id }.toSet(),
        )
        assertTrue(keys.all { it.publicKey.size == 32 })
    }

    @Test
    fun `connector telemetry maps to visible live state`() {
        val state = ConnectorConnectionState.from(
            ConnectorTelemetrySnapshot(
                connectorId = "dev.maverick.whoop5",
                lifecycle = ConnectorLifecycleState.STREAMING,
                sessionId = 42u,
                cancellationGeneration = 0u,
                deviceId = 1u,
                heartRateBpm = 73u,
                batteryPercent = 82u,
                onWrist = true,
                lastSampleWallTimeMs = 1_700_000_000_123,
            ),
        )
        assertEquals("Streaming", state.label)
        assertEquals(73, state.heartRateBpm)
        assertEquals(82, state.batteryPercent)
        assertEquals(true, state.connected)
    }

    @Test
    fun `approval requires inspection and cancel erases pending bytes`() {
        val machine = ConnectorApprovalMachine()
        assertThrows(ConnectorApprovalException.InspectionRequired::class.java) { machine.beginApproval() }
        machine.beginInspection()
        machine.inspectionSucceeded(
            ConnectorApprovalSummary(
                connectorId = "org.example.sensor",
                version = "1.0.0",
                displayName = "Example Sensor",
                publisherKeyId = "publisher.example",
                fixtureCount = 4u,
            ),
            byteArrayOf(1, 2, 3),
        )
        machine.beginApproval()
        machine.cancel()
        assertEquals(ConnectorApprovalPhase.Idle, machine.phase)
        assertNull(machine.pendingBytes)
    }

    @Test
    fun `failure rollback and revocation are explicit states`() {
        val machine = ConnectorApprovalMachine()
        machine.fail("Signature rejected")
        assertEquals(ConnectorApprovalPhase.Failed("Signature rejected"), machine.phase)
        machine.rolledBack("org.example.sensor")
        assertEquals(ConnectorApprovalPhase.RolledBack("org.example.sensor"), machine.phase)
        machine.revoked("org.example.sensor")
        assertEquals(ConnectorApprovalPhase.Revoked("org.example.sensor"), machine.phase)
    }

    @Test
    fun `generic action mapping carries signed native uuids`() {
        assertEquals(
            ConnectorNativeOperation.Scan(listOf("180D"), listOf()),
            ConnectorNativeOperation.map(ConnectorTransportRequest.StartScan(listOf("180D"), listOf())),
        )
        assertEquals(
            ConnectorNativeOperation.Subscribe("measurement", "180D", "2A37"),
            ConnectorNativeOperation.map(
                ConnectorTransportRequest.Subscribe("measurement", "180D", "2A37"),
            ),
        )
        val write = ConnectorNativeOperation.map(
            ConnectorTransportRequest.Write("control", "180D", "2A39", byteArrayOf(1), true),
        ) as ConnectorNativeOperation.Write
        assertEquals("control", write.id)
        assertEquals("180D", write.service)
        assertEquals("2A39", write.characteristic)
        assertArrayEquals(byteArrayOf(1), write.bytes)
        assertEquals(true, write.confirmed)
    }

    @Test
    fun `process restoration stores opaque identity and no locator`() {
        val checkpoint = ConnectorRestorationCheckpoint("org.example.sensor", 42u, 3u)
        val encoded = checkpoint.encode()
        assertEquals(checkpoint, ConnectorRestorationCheckpoint.decode(encoded))
        assertFalse(encoded.contains("mavconn"))
        assertFalse(encoded.contains("content:"))
    }
}
