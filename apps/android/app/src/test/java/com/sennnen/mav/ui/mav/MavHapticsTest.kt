package com.sennnen.mav.ui.mav

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The haptic vocabulary (ADR-032). The Swift twin is `MavHapticsTests.swift`.
 *
 * The wire names matter more than they look: a manifest declares them and the host snapshot lists
 * them, so a rename here silently stops a connector's declaration from matching and the feature
 * quietly disappears. They are asserted literally for that reason.
 */
class MavHapticsTest {

    @Test
    fun `wire names are exactly the vocabulary ADR-032 fixed`() {
        assertEquals("milestone", MavHapticSignal.Milestone.id)
        assertEquals("goal_complete", MavHapticSignal.GoalComplete.id)
        assertEquals("set_logged", MavHapticSignal.SetLogged.id)
        assertEquals("rest_complete", MavHapticSignal.RestComplete.id)
        assertEquals("zone_alert_1", MavHapticSignal.ZoneAlert(1).id)
        assertEquals("zone_alert_5", MavHapticSignal.ZoneAlert(5).id)
    }

    @Test
    fun `the vocabulary is nine signals and every name is distinct`() {
        val all = MavHapticSignal.all
        assertEquals(9, all.size)
        assertEquals("a signal name is duplicated", 9, all.map { it.id }.toSet().size)
        assertTrue("a signal has no explanation", all.all { it.explanation.isNotBlank() })
    }

    @Test
    fun `nothing declared means nothing is supported`() {
        val support = MavHapticSupport.None
        assertFalse(support.canBuzz)
        for (signal in MavHapticSignal.all) {
            assertFalse("${signal.id} claimed support with an empty declaration", support.supports(signal))
        }
    }

    @Test
    fun `a partial declaration supports exactly what it named`() {
        // A strap that can tap but not run a five-zone pattern is a real shape, not a hypothetical.
        val support = MavHapticSupport(setOf("milestone", "goal_complete"))
        assertTrue(support.canBuzz)
        assertTrue(support.supports(MavHapticSignal.Milestone))
        assertTrue(support.supports(MavHapticSignal.GoalComplete))
        assertFalse(support.supports(MavHapticSignal.SetLogged))
        assertFalse(support.supports(MavHapticSignal.ZoneAlert(3)))
    }

    @Test
    fun `the unavailable reason distinguishes no strap from a strap that cannot buzz`() {
        val support = MavHapticSupport.None
        assertEquals(
            "No strap is connected, so there is nothing to buzz.",
            support.reason(null),
        )
        assertEquals(
            "No strap is connected, so there is nothing to buzz.",
            support.reason(""),
        )
        assertEquals(
            "WHOOP 4.0 does not report a haptic motor, so it cannot buzz.",
            support.reason("WHOOP 4.0"),
        )
    }
}
