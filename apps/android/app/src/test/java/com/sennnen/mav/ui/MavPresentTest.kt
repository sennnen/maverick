package com.sennnen.mav.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MavPresentTest {
    private val asOf = 1_752_600_500_000L

    @Test
    fun aFreshStreamingSampleShowsNoLabel() {
        assertNull(sampleAgeLabel(asOf, asOf - 10_000, connected = true))
        assertNull(sampleAgeLabel(asOf, asOf - FRESH_SAMPLE_MS, connected = true))
    }

    @Test
    fun aStaleStreamingSampleIsVisiblyStale() {
        assertEquals("Last sample 20 s ago", sampleAgeLabel(asOf, asOf - 20_000, connected = true))
    }

    @Test
    fun aDisconnectedSnapshotAlwaysCarriesItsAge() {
        assertEquals("Last sample 5 s ago", sampleAgeLabel(asOf, asOf - 5_000, connected = false))
        assertEquals("Last sample 1 m ago", sampleAgeLabel(asOf, asOf - 90_000, connected = false))
        assertEquals("Last sample 3 h ago", sampleAgeLabel(asOf, asOf - 3 * 3_600_000L, connected = false))
        assertEquals("Last sample 2 d ago", sampleAgeLabel(asOf, asOf - 2 * 86_400_000L, connected = false))
    }

    @Test
    fun noSamplesShowWaitingOnlyWhileTheLinkIsUp() {
        assertEquals("Waiting for first sample", sampleAgeLabel(asOf, null, connected = true))
        assertNull(sampleAgeLabel(asOf, null, connected = false))
    }

    @Test
    fun fixedPointDisplayConversionsAreExact() {
        assertEquals("67.5 ms", microsAsMs(67_454, java.util.Locale.US))
        assertEquals("828.0 ms", microsAsMs(828_000, java.util.Locale.US))
        assertEquals("50.0%", milliPercentAsPercent(50_000, java.util.Locale.US))
    }
}
