package com.sennnen.mav.ui

import com.sennnen.mav.MavSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LiveStateMappingTest {
    private fun snapshot(
        connectionState: String,
        currentBpm: Int? = 72,
        batteryPercent: Int? = null,
        charging: Boolean? = null,
        onWrist: Boolean? = null,
    ) = MavSnapshot(
        coreVersion = "0.1.0",
        storageSchema = 1,
        revision = 1uL,
        asOfUnixMs = 1_752_600_500_000L,
        connectionState = connectionState,
        deviceName = "MG",
        batteryPercent = batteryPercent,
        charging = charging,
        onWrist = onWrist,
        lastSampleUnixMs = 1_752_600_500_000L,
        currentBpm = currentBpm,
        meanMilliBpm = 72_000,
        inRangeSamples = 1,
        excludedSamples = 0,
        prv = null,
        prvUnavailableReason = null,
        recoveryUnavailableReason = "Recovery model not admitted",
        hash = "abc",
    )

    @Test
    fun streamingLinkIsConnectedAndCarriesTheLiveReadout() {
        val live = liveStateOf(snapshot("streaming", batteryPercent = 81, charging = false))
        assertTrue(live.connected)
        assertTrue(live.bonded)
        assertEquals(72, live.heartRate)
        assertEquals(81.0, live.batteryPct!!, 0.0)
        assertEquals(false, live.charging)
        assertEquals("MG", live.advertisingName)
        assertFalse(live.scanning)
        assertEquals("Recovery model not admitted", live.statusNote)
        // No wrist event yet: assume worn (parity default).
        assertTrue(live.worn)
    }

    @Test
    fun wristStateMapsToWorn() {
        assertFalse(liveStateOf(snapshot("streaming", onWrist = false)).worn)
        assertTrue(liveStateOf(snapshot("streaming", onWrist = true)).worn)
        // Off the link, worn resets to the assume-worn default rather than a stale off-wrist.
        assertTrue(liveStateOf(snapshot("disconnected", onWrist = false)).worn)
    }

    @Test
    fun subscribingCountsAsConnectedScanningDoesNot() {
        assertTrue(liveStateOf(snapshot("subscribing")).connected)
        val scanning = liveStateOf(snapshot("scanning"))
        assertFalse(scanning.connected)
        assertTrue(scanning.scanning)
    }

    @Test
    fun aStoredHeartRateDoesNotOutliveTheLink() {
        val live = liveStateOf(snapshot("disconnected", currentBpm = 72))
        assertFalse(live.connected)
        assertNull(live.heartRate)
        assertNull(live.batteryPct)
    }
}
