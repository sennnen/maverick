package com.sennnen.mav.ui.mav

// The interim-milestone and goal-completion engine for a cardio session.
//
// Ported from the Aura prototype's `WorkoutMilestones`, which is the one piece of that flow worth
// carrying across unchanged: it is pure. No clock reads, no BLE, no storage. The caller feeds it
// the session's current metrics and it says which signals newly fired.
//
// Two properties matter and are asserted rather than assumed:
//
//  - **Each mark fires exactly once.** `State` records what has already fired, so re-evaluating the
//    same metrics is silent.
//  - **A catch-up collapses into one signal.** If the app was backgrounded across three kilometre
//    marks, the wrist buzzes once on return, not three times. A late buzz is a wrong buzz; three
//    late buzzes are a nuisance.
//
// The signals it returns are `MavHapticSignal` values (ADR-032), which the connector may or may not
// be able to render. Deciding *when* is this engine's job; deciding *whether* is the connector's.
//
// The Swift twin is `MavMilestones.swift`.

object MavMilestones {

    /** Resolved once at session start, from the confirm screen plus the milestone deep settings. */
    data class Config(
        val goal: MavGoal = MavGoal.None,
        val zoneTarget: MavZoneTarget? = null,
        /** Interim distance spacing in kilometres, always positive. */
        val distanceEveryKm: Double = 1.0,
        val timeMode: MavTimeMilestoneMode = MavTimeMilestoneMode.HALFWAY,
        val calorieMode: MavCalorieMilestoneMode = MavCalorieMilestoneMode.HALFWAY,
    )

    /**
     * What has already fired. Kept as plain data so a persisted session can carry it: a relaunch
     * must not replay every buzz since the start.
     */
    data class State(
        var interimMarks: Int = 0,
        var halfwayFired: Boolean = false,
        var goalFired: Boolean = false,
        var zoneTargetFired: Boolean = false,
    )

    /** Something the wearer should be told about, and the reason it is worth telling them. */
    enum class Event {
        /** Progress update — a light tap. */
        MILESTONE,

        /**
         * The end condition is met — a hard buzz. The session keeps recording; a goal is a target,
         * not a guillotine, and only the wearer ends a workout.
         */
        GOAL_COMPLETE,

        /** The session's zone target was banked — a light tap, and a checkmark on the bars. */
        ZONE_TARGET_MET,
        ;

        /**
         * The haptic signal this event asks for. `ZONE_TARGET_MET` borrows `Milestone` rather than
         * claiming a vocabulary entry of its own: to the wrist it is the same light tap, and
         * ADR-032's vocabulary is closed.
         */
        val signal: MavHapticSignal
            get() = when (this) {
                MILESTONE, ZONE_TARGET_MET -> MavHapticSignal.Milestone
                GOAL_COMPLETE -> MavHapticSignal.GoalComplete
            }
    }

    /**
     * Advance [state] against the session's current metrics.
     *
     * @param elapsedSec seconds since the session started.
     * @param distanceM metres travelled, zero when there is no route.
     * @param kcal energy burned so far.
     * @param zoneSeconds seconds banked per heart-rate zone, index 0 being zone 1.
     */
    fun evaluate(
        state: State,
        config: Config,
        elapsedSec: Int,
        distanceM: Double,
        kcal: Double,
        zoneSeconds: List<Double>,
    ): List<Event> {
        val events = mutableListOf<Event>()

        // Interim marks follow from the *kind* of end condition. A free workout buzzes nothing —
        // silence is the honest default when the wearer named no target.
        when (config.goal.kind) {
            MavGoalKind.NONE -> Unit

            MavGoalKind.DISTANCE -> {
                val every = maxOf(config.distanceEveryKm, 0.001)
                val marks = ((distanceM / 1_000) / every).toInt()
                if (marks > state.interimMarks) {
                    state.interimMarks = marks
                    // The goal buzz below already covers the final mark, so it is not announced
                    // twice.
                    if (!reached(config.goal, elapsedSec, distanceM, kcal)) events.add(Event.MILESTONE)
                }
            }

            MavGoalKind.TIME -> when (config.timeMode) {
                MavTimeMilestoneMode.OFF -> Unit
                MavTimeMilestoneMode.HALFWAY ->
                    if (!state.halfwayFired && config.goal.isActive &&
                        elapsedSec >= config.goal.value * 60 / 2
                    ) {
                        state.halfwayFired = true
                        events.add(Event.MILESTONE)
                    }

                MavTimeMilestoneMode.EVERY10, MavTimeMilestoneMode.EVERY15 -> {
                    val every = if (config.timeMode == MavTimeMilestoneMode.EVERY10) 600 else 900
                    val marks = elapsedSec / every
                    if (marks > state.interimMarks) {
                        state.interimMarks = marks
                        if (!reached(config.goal, elapsedSec, distanceM, kcal)) {
                            events.add(Event.MILESTONE)
                        }
                    }
                }
            }

            MavGoalKind.CALORIES -> when (config.calorieMode) {
                MavCalorieMilestoneMode.OFF -> Unit
                MavCalorieMilestoneMode.HALFWAY ->
                    if (!state.halfwayFired && config.goal.isActive && kcal >= config.goal.value / 2) {
                        state.halfwayFired = true
                        events.add(Event.MILESTONE)
                    }

                MavCalorieMilestoneMode.EVERY50, MavCalorieMilestoneMode.EVERY100 -> {
                    val every =
                        if (config.calorieMode == MavCalorieMilestoneMode.EVERY50) 50.0 else 100.0
                    val marks = (kcal / every).toInt()
                    if (marks > state.interimMarks) {
                        state.interimMarks = marks
                        if (!reached(config.goal, elapsedSec, distanceM, kcal)) {
                            events.add(Event.MILESTONE)
                        }
                    }
                }
            }
        }

        if (!state.goalFired && config.goal.isActive &&
            reached(config.goal, elapsedSec, distanceM, kcal)
        ) {
            state.goalFired = true
            events.add(Event.GOAL_COMPLETE)
        }

        val target = config.zoneTarget
        if (!state.zoneTargetFired && target != null && target.zone in 1..5 &&
            target.zone - 1 in zoneSeconds.indices &&
            zoneSeconds[target.zone - 1] >= target.minutes * 60.0
        ) {
            state.zoneTargetFired = true
            events.add(Event.ZONE_TARGET_MET)
        }

        return events
    }

    /**
     * Whether the end condition is satisfied. Values are stored natively — kilometres, minutes,
     * kilocalories — so the display unit never reaches this comparison.
     */
    fun reached(goal: MavGoal, elapsedSec: Int, distanceM: Double, kcal: Double): Boolean {
        if (!goal.isActive) return false
        return when (goal.kind) {
            MavGoalKind.NONE -> false
            MavGoalKind.DISTANCE -> distanceM / 1_000 >= goal.value
            MavGoalKind.TIME -> elapsedSec >= goal.value * 60
            MavGoalKind.CALORIES -> kcal >= goal.value
        }
    }

    /**
     * How far through the end condition the session is, 0..1, or null when there is no goal.
     * Drives the live screen's progress bar.
     */
    fun progress(goal: MavGoal, elapsedSec: Int, distanceM: Double, kcal: Double): Double? {
        if (!goal.isActive) return null
        val done = when (goal.kind) {
            MavGoalKind.NONE -> return null
            MavGoalKind.DISTANCE -> distanceM / 1_000
            MavGoalKind.TIME -> elapsedSec / 60.0
            MavGoalKind.CALORIES -> kcal
        }
        return (done / goal.value).coerceIn(0.0, 1.0)
    }
}
