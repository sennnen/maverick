package com.sennnen.mav.ui.mav

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The confirm screen's pure half, and the sport catalogue behind it. The iOS twin is
 * `MavWorkoutConfigTests.swift` and asserts the same numbers, because a catalogue that lists a
 * sport on one phone and not the other is a parity break the shared core cannot catch.
 */
class MavWorkoutConfigTest {

    // Catalogue ----------------------------------------------------------------------------------

    @Test
    fun `the catalogue has six categories and every sport name is unique`() {
        assertEquals(6, MavSportCatalog.categories.size)
        assertEquals(18, MavSportCatalog.all.size)
        assertEquals(
            "a sport name is duplicated, so the sticky-config key would collide",
            18,
            MavSportCatalog.all.map { it.name }.toSet().size,
        )
        assertTrue(MavSportCatalog.all.all { it.detail.isNotBlank() })
    }

    @Test
    fun `exactly one sport is strength and it is the one the logger opens for`() {
        val strength = MavSportCatalog.all.filter { it.isStrength }
        assertEquals(1, strength.size)
        assertEquals("Strength training", strength.single().name)
        // Strength has no route: it must not offer GPS, and it never reaches the confirm screen.
        assertFalse(strength.single().isDistance)
    }

    @Test
    fun `only sports with a route are distance sports`() {
        val distance = MavSportCatalog.all.filter { it.isDistance }.map { it.name }.toSet()
        assertEquals(setOf("Outdoor run", "Walking", "Hiking", "Cycling"), distance)
        // A treadmill reports no route, so offering a route map on it would be a lie.
        assertFalse(MavSportCatalog.sport("Treadmill")!!.isDistance)
        assertFalse(MavSportCatalog.sport("Rowing")!!.isDistance)
    }

    @Test
    fun `an unknown sport name resolves to nothing rather than a guess`() {
        assertNull(MavSportCatalog.sport("Underwater basket weaving"))
    }

    // Goal defaults and display ------------------------------------------------------------------

    @Test
    fun `metric and imperial distance defaults are five kilometres and three miles`() {
        assertEquals(5.0, defaultGoalValue(MavGoalKind.DISTANCE, isImperial = false), 0.0001)
        // Three miles, stored in kilometres.
        assertEquals(4.828, defaultGoalValue(MavGoalKind.DISTANCE, isImperial = true), 0.001)
        assertEquals(30.0, defaultGoalValue(MavGoalKind.TIME, isImperial = false), 0.0001)
        assertEquals(300.0, defaultGoalValue(MavGoalKind.CALORIES, isImperial = false), 0.0001)
        assertEquals(0.0, defaultGoalValue(MavGoalKind.NONE, isImperial = false), 0.0001)
    }

    @Test
    fun `display text converts to miles but the stored value stays kilometres`() {
        val fiveKm = MavGoal(MavGoalKind.DISTANCE, 5.0)
        assertEquals("5", goalDisplayText(fiveKm, isImperial = false))
        assertEquals("3.1", goalDisplayText(fiveKm, isImperial = true))
        // The stored value is untouched by how it was shown.
        assertEquals(5.0, fiveKm.value, 0.0001)
    }

    @Test
    fun `a whole number drops its decimal and a fraction keeps one`() {
        assertEquals("30", goalDisplayText(MavGoal(MavGoalKind.TIME, 30.0), isImperial = false))
        assertEquals("7.5", goalDisplayText(MavGoal(MavGoalKind.TIME, 7.5), isImperial = false))
    }

    @Test
    fun `an inactive goal has no display text`() {
        assertEquals("", goalDisplayText(MavGoal.None, isImperial = false))
        // Zero is not a goal, whatever kind it claims.
        assertEquals("", goalDisplayText(MavGoal(MavGoalKind.TIME, 0.0), isImperial = false))
    }

    @Test
    fun `the unit label follows the kind`() {
        assertEquals("km", goalUnit(MavGoalKind.DISTANCE, "km"))
        assertEquals("mi", goalUnit(MavGoalKind.DISTANCE, "mi"))
        assertEquals("min", goalUnit(MavGoalKind.TIME, "km"))
        assertEquals("kcal", goalUnit(MavGoalKind.CALORIES, "km"))
        assertEquals("", goalUnit(MavGoalKind.NONE, "km"))
    }

    // Goal activity --------------------------------------------------------------------------

    @Test
    fun `a goal needs both a kind and a positive value to be active`() {
        assertFalse(MavGoal.None.isActive)
        assertFalse(MavGoal(MavGoalKind.DISTANCE, 0.0).isActive)
        assertFalse(MavGoal(MavGoalKind.NONE, 5.0).isActive)
        assertTrue(MavGoal(MavGoalKind.DISTANCE, 5.0).isActive)
    }

    // Sticky-config keys -------------------------------------------------------------------------

    @Test
    fun `sport names slug into stable keys`() {
        assertEquals("outdoor-run", MavWorkoutPrefs.slug("Outdoor run"))
        assertEquals("strength-training", MavWorkoutPrefs.slug("Strength training"))
        assertEquals("mind-body", MavWorkoutPrefs.slug("Mind & body"))
        assertEquals("other-activity", MavWorkoutPrefs.slug("Other activity"))
    }

    @Test
    fun `every catalogue sport slugs to something distinct`() {
        val slugs = MavSportCatalog.all.map { MavWorkoutPrefs.slug(it.name) }
        assertEquals(
            "two sports share a preferences key, so one would overwrite the other",
            slugs.size,
            slugs.toSet().size,
        )
        assertTrue(slugs.none { it.isBlank() })
    }
}
