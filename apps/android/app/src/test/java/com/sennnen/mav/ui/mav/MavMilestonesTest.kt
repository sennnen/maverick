package com.sennnen.mav.ui.mav

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The milestone engine. The iOS twin is `MavMilestonesTests.swift` and asserts the same cases.
 *
 * The two properties worth protecting are that a mark fires exactly once, and that a catch-up
 * collapses into a single signal. Both are easy to regress into a wrist that buzzes four times when
 * an app returns from the background, which is the failure the wearer notices most.
 */
class MavMilestonesTest {

    private fun distanceConfig(km: Double, every: Double = 1.0) = MavMilestones.Config(
        goal = MavGoal(MavGoalKind.DISTANCE, km),
        distanceEveryKm = every,
    )

    private fun evaluate(
        state: MavMilestones.State,
        config: MavMilestones.Config,
        elapsed: Int = 0,
        distanceM: Double = 0.0,
        kcal: Double = 0.0,
        zoneSeconds: List<Double> = emptyList(),
    ) = MavMilestones.evaluate(state, config, elapsed, distanceM, kcal, zoneSeconds)

    // Silence ------------------------------------------------------------------------------------

    @Test
    fun `a free workout never buzzes`() {
        val state = MavMilestones.State()
        val config = MavMilestones.Config()
        assertEquals(
            emptyList<MavMilestones.Event>(),
            evaluate(state, config, elapsed = 7_200, distanceM = 42_000.0, kcal = 3_000.0),
        )
        assertEquals(MavMilestones.State(), state)
    }

    // Distance -----------------------------------------------------------------------------------

    @Test
    fun `each kilometre mark fires once`() {
        val state = MavMilestones.State()
        val config = distanceConfig(km = 5.0)

        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, distanceM = 999.0))
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, distanceM = 1_000.0))
        // Re-evaluating the same distance is silent.
        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, distanceM = 1_000.0))
        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, distanceM = 1_400.0))
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, distanceM = 2_000.0))
    }

    @Test
    fun `a catch-up across several marks collapses into one buzz`() {
        val state = MavMilestones.State()
        val config = distanceConfig(km = 10.0)

        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, distanceM = 1_000.0))
        // The app was away for three kilometres. The wrist buzzes once, not three times.
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, distanceM = 4_000.0))
        assertEquals(4, state.interimMarks)
    }

    @Test
    fun `the final mark is not announced twice`() {
        val state = MavMilestones.State()
        val config = distanceConfig(km = 3.0)

        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, distanceM = 1_000.0))
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, distanceM = 2_000.0))
        // Reaching 3 km is both a kilometre mark and the goal. Only the goal is announced.
        assertEquals(
            listOf(MavMilestones.Event.GOAL_COMPLETE),
            evaluate(state, config, distanceM = 3_000.0),
        )
    }

    @Test
    fun `the goal fires once and the session keeps running`() {
        val state = MavMilestones.State()
        val config = distanceConfig(km = 2.0)

        assertEquals(
            listOf(MavMilestones.Event.GOAL_COMPLETE),
            evaluate(state, config, distanceM = 2_000.0),
        )
        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, distanceM = 2_500.0))
        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, distanceM = 9_000.0))
    }

    @Test
    fun `a custom spacing changes where the marks land`() {
        val state = MavMilestones.State()
        val config = distanceConfig(km = 20.0, every = 5.0)

        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, distanceM = 4_999.0))
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, distanceM = 5_000.0))
    }

    // Time ---------------------------------------------------------------------------------------

    @Test
    fun `time halfway fires once at the midpoint`() {
        val state = MavMilestones.State()
        val config = MavMilestones.Config(goal = MavGoal(MavGoalKind.TIME, 30.0))

        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, elapsed = 14 * 60))
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, elapsed = 15 * 60))
        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, elapsed = 16 * 60))
        assertEquals(
            listOf(MavMilestones.Event.GOAL_COMPLETE),
            evaluate(state, config, elapsed = 30 * 60),
        )
    }

    @Test
    fun `time interval mode marks every ten minutes`() {
        val state = MavMilestones.State()
        val config = MavMilestones.Config(
            goal = MavGoal(MavGoalKind.TIME, 60.0),
            timeMode = MavTimeMilestoneMode.EVERY10,
        )

        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, elapsed = 599))
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, elapsed = 600))
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, elapsed = 1_200))
    }

    @Test
    fun `time mode off silences interims but not the goal`() {
        val state = MavMilestones.State()
        val config = MavMilestones.Config(
            goal = MavGoal(MavGoalKind.TIME, 20.0),
            timeMode = MavTimeMilestoneMode.OFF,
        )

        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, elapsed = 10 * 60))
        assertEquals(
            listOf(MavMilestones.Event.GOAL_COMPLETE),
            evaluate(state, config, elapsed = 20 * 60),
        )
    }

    // Calories -----------------------------------------------------------------------------------

    @Test
    fun `calorie halfway and goal`() {
        val state = MavMilestones.State()
        val config = MavMilestones.Config(goal = MavGoal(MavGoalKind.CALORIES, 400.0))

        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, kcal = 199.0))
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, kcal = 200.0))
        assertEquals(listOf(MavMilestones.Event.GOAL_COMPLETE), evaluate(state, config, kcal = 400.0))
    }

    @Test
    fun `calorie interval mode marks every fifty`() {
        val state = MavMilestones.State()
        val config = MavMilestones.Config(
            goal = MavGoal(MavGoalKind.CALORIES, 500.0),
            calorieMode = MavCalorieMilestoneMode.EVERY50,
        )

        assertEquals(emptyList<MavMilestones.Event>(), evaluate(state, config, kcal = 49.0))
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, kcal = 50.0))
        assertEquals(listOf(MavMilestones.Event.MILESTONE), evaluate(state, config, kcal = 100.0))
    }

    // Zone target --------------------------------------------------------------------------------

    @Test
    fun `the zone target fires once when the time is banked`() {
        val state = MavMilestones.State()
        val config = MavMilestones.Config(zoneTarget = MavZoneTarget(2, 15))

        assertEquals(
            emptyList<MavMilestones.Event>(),
            evaluate(state, config, zoneSeconds = listOf(0.0, 14 * 60.0, 0.0, 0.0, 0.0)),
        )
        assertEquals(
            listOf(MavMilestones.Event.ZONE_TARGET_MET),
            evaluate(state, config, zoneSeconds = listOf(0.0, 15 * 60.0, 0.0, 0.0, 0.0)),
        )
        assertEquals(
            emptyList<MavMilestones.Event>(),
            evaluate(state, config, zoneSeconds = listOf(0.0, 40 * 60.0, 0.0, 0.0, 0.0)),
        )
    }

    @Test
    fun `a zone target is ignored when that zone has no reading`() {
        val state = MavMilestones.State()
        val config = MavMilestones.Config(zoneTarget = MavZoneTarget(5, 1))

        // Three zones reported, target names the fifth. Nothing fires, and nothing crashes.
        assertEquals(
            emptyList<MavMilestones.Event>(),
            evaluate(state, config, zoneSeconds = listOf(600.0, 600.0, 600.0)),
        )
        assertFalse(state.zoneTargetFired)
    }

    // Signals ------------------------------------------------------------------------------------

    @Test
    fun `events map onto the closed haptic vocabulary`() {
        assertEquals(MavHapticSignal.Milestone, MavMilestones.Event.MILESTONE.signal)
        assertEquals(MavHapticSignal.Milestone, MavMilestones.Event.ZONE_TARGET_MET.signal)
        assertEquals(MavHapticSignal.GoalComplete, MavMilestones.Event.GOAL_COMPLETE.signal)
    }

    // Progress -----------------------------------------------------------------------------------

    @Test
    fun `progress is null without a goal and clamps with one`() {
        assertNull(MavMilestones.progress(MavGoal.None, 60, 500.0, 10.0))

        val goal = MavGoal(MavGoalKind.DISTANCE, 4.0)
        assertEquals(0.25, MavMilestones.progress(goal, 0, 1_000.0, 0.0)!!, 0.0001)
        assertEquals(1.0, MavMilestones.progress(goal, 0, 9_000.0, 0.0)!!, 0.0001)
    }
}
