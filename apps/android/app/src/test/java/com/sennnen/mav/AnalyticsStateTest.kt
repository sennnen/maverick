package com.sennnen.mav

import com.sennnen.mav.ml.MavApplicability
import com.sennnen.mav.ml.MavPlannedStage
import com.sennnen.mav.ml.MavStageHealth
import com.sennnen.mav.ml.MavSignalCoverage
import com.sennnen.mav.ml.MavSignalReducer
import com.sennnen.mav.ml.MavSignalState
import com.sennnen.mav.ml.MavStageState
import com.sennnen.mav.ml.MavUnavailable
import com.sennnen.mav.ui.mav.MavSignalCopy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The states a wearer can be shown, tested where no device is needed to know the answer.
 *
 * Every case here is one the UI cannot survive getting wrong and no on-device run would catch:
 * a signal that renders "unavailable" when it is merely still working looks broken, and one that
 * renders a spinner forever when the sensor is genuinely absent is worse — the wearer waits for
 * something that is never going to arrive.
 */
class AnalyticsStateTest {

    private fun stage(
        model: String,
        signal: String = "cardiovascular",
        state: MavStageState = MavStageState.READY,
        displayable: Boolean = true,
        unavailable: MavUnavailable? = null,
    ) = MavPlannedStage(model, signal, state, displayable, unavailable)

    @Test
    fun a_signal_with_work_outstanding_reports_how_far_it_has_got() {
        val signals = MavSignalReducer.reduce(
            listOf(
                stage("cva_encoder", state = MavStageState.CACHED),
                stage("cva_probes_male", state = MavStageState.READY),
            ),
        )
        assertEquals(MavSignalState.Working(done = 1, total = 2), signals.single().state)
    }

    @Test
    fun a_finished_signal_carries_when_it_finished_so_a_reading_can_be_aged() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("cva_encoder", state = MavStageState.CACHED)),
            completedAtMs = mapOf("cva_encoder" to 1_700_000_000_000L),
        )
        assertEquals(
            MavSignalState.Ready(atMs = 1_700_000_000_000L, displayable = true),
            signals.single().state,
        )
    }

    @Test
    fun new_data_marks_a_finished_signal_stale_rather_than_blanking_it() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("cva_encoder", state = MavStageState.CACHED)),
            completedAtMs = mapOf("cva_encoder" to 42L),
            invalidated = setOf("cva_encoder"),
        )
        assertEquals(MavSignalState.Stale(atMs = 42L, displayable = true), signals.single().state)
    }

    @Test
    fun a_signal_nothing_can_run_reports_the_distinct_reasons_not_a_count() {
        val signals = MavSignalReducer.reduce(
            listOf(
                stage(
                    "sleepnet_bdi",
                    signal = "sleep",
                    state = MavStageState.UNAVAILABLE,
                    unavailable = MavUnavailable.PreprocessingNotPorted("the per-epoch ibi channel"),
                ),
                stage(
                    "sleepnet_bdi_v3",
                    signal = "sleep",
                    state = MavStageState.UNAVAILABLE,
                    unavailable = MavUnavailable.PreprocessingNotPorted("the per-epoch ibi channel"),
                ),
            ),
        )
        val state = signals.single().state as MavSignalState.Unavailable
        assertEquals(
            "two stages blocked by the same cause should say it once",
            1,
            state.reasons.size,
        )
    }

    /**
     * The distinction that matters most: a missing sensor sends someone shopping, a missing
     * profile field is one tap, and an unported front-end is neither. Collapsing them would make
     * the app tell people to buy a strap for a model that could not run either way.
     */
    @Test
    fun the_four_unavailable_causes_stay_distinguishable() {
        val signals = MavSignalReducer.reduce(
            listOf(
                stage(
                    "a",
                    signal = "s",
                    state = MavStageState.UNAVAILABLE,
                    unavailable = MavUnavailable.MissingStreams(listOf("spo2_percent")),
                ),
                stage(
                    "b",
                    signal = "s",
                    state = MavStageState.UNAVAILABLE,
                    unavailable = MavUnavailable.MissingProfile(listOf("age")),
                ),
                stage(
                    "c",
                    signal = "s",
                    state = MavStageState.UNAVAILABLE,
                    unavailable = MavUnavailable.UpstreamUnavailable("cva_encoder"),
                ),
                stage(
                    "d",
                    signal = "s",
                    state = MavStageState.UNAVAILABLE,
                    unavailable = MavUnavailable.PreprocessingNotPorted("the 77 features"),
                ),
            ),
        )
        val state = signals.single().state as MavSignalState.Unavailable
        assertEquals(4, state.reasons.size)
        assertTrue(state.reasons.any { it is MavUnavailable.MissingStreams })
        assertTrue(state.reasons.any { it is MavUnavailable.MissingProfile })
        assertTrue(state.reasons.any { it is MavUnavailable.UpstreamUnavailable })
        assertTrue(state.reasons.any { it is MavUnavailable.PreprocessingNotPorted })
    }

    @Test
    fun a_signal_computed_but_not_interpretable_says_so_rather_than_showing_a_number() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("sleepnet_bdi", signal = "sleep", state = MavStageState.CACHED, displayable = false)),
            completedAtMs = mapOf("sleepnet_bdi" to 7L),
        )
        val state = signals.single().state as MavSignalState.Ready
        assertTrue("the staging vocabulary is not admitted", !state.displayable)
    }

    @Test
    fun a_stage_that_exhausts_its_retries_offers_the_wearer_the_button() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("cva_encoder")),
            failures = mapOf("cva_encoder" to MavSignalReducer.RETRY_BUDGET),
        )
        val state = signals.single().state as MavSignalState.Failed
        assertTrue(state.retryable)
        assertEquals(MavSignalReducer.RETRY_BUDGET, state.attempts)
    }

    @Test
    fun a_stage_still_inside_its_budget_keeps_working_rather_than_giving_up() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("cva_encoder")),
            failures = mapOf("cva_encoder" to MavSignalReducer.RETRY_BUDGET - 1),
        )
        assertTrue(signals.single().state is MavSignalState.Working)
    }

    @Test
    fun a_deferred_pass_says_it_is_waiting_rather_than_that_it_failed() {
        val signals = MavSignalReducer.reduce(listOf(stage("cva_encoder")), deferred = true)
        assertEquals(MavSignalState.Deferred, signals.single().state)
    }

    @Test
    fun a_signal_that_finished_before_the_os_deferred_is_still_finished() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("cva_encoder", state = MavStageState.CACHED)),
            deferred = true,
        )
        assertTrue(signals.single().state is MavSignalState.Ready)
    }

    @Test
    fun a_missing_permission_outranks_every_other_explanation() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("cva_encoder")),
            missingPermission = "android.permission.BLUETOOTH_CONNECT",
        )
        assertEquals(
            MavSignalState.PermissionRequired("android.permission.BLUETOOTH_CONNECT"),
            signals.single().state,
        )
    }

    @Test
    fun coverage_counts_runnable_against_total_per_signal() {
        val signals = MavSignalReducer.reduce(
            listOf(
                stage("a", signal = "s", state = MavStageState.CACHED),
                stage(
                    "b",
                    signal = "s",
                    state = MavStageState.UNAVAILABLE,
                    unavailable = MavUnavailable.MissingStreams(listOf("ppg")),
                ),
            ),
        )
        assertEquals(2, signals.single().total)
        assertEquals(1, signals.single().runnable)
    }

    /**
     * Every signal the core can plan has written-out copy. Title-casing the slug instead gives
     * "Daytime Hrv" and "Ppg Foundation", which is how a product surface starts looking generated.
     * The list is `Signal::name` in `core/crates/mav-analytic/src/model_zoo/pipeline.rs`; the iOS
     * twin of this test is `testEverySignalHasCopyRatherThanATitleCasedSlug`.
     */
    @Test
    fun every_signal_the_core_can_plan_has_a_written_out_name() {
        val planned = listOf(
            "activity", "energy_expenditure", "step_eligibility", "awake_heart_rate",
            "daytime_hrv", "workout_heart_rate", "cardiovascular", "hypertension_risk",
            "sleep", "illness_risk", "cycle_awareness", "ppg_foundation",
        )
        for (slug in planned) {
            assertTrue("$slug has no written-out name", MavSignalCopy.knows(slug))
        }
    }

    /**
     * The core counts coverage on every plan so that two platforms do not each write the same
     * loop. This proves the platform actually reads it rather than quietly recounting: the figures
     * below are deliberately not what counting the group would produce.
     */
    @Test
    fun the_cores_own_coverage_is_used_rather_than_recounted() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("a", signal = "s", state = MavStageState.CACHED)),
            coverage = mapOf("s" to MavSignalCoverage(total = 9, runnable = 4)),
        )
        assertEquals(9, signals.single().total)
        assertEquals(4, signals.single().runnable)
    }

    private fun health(
        model: String,
        applicability: MavApplicability,
        vararg substitutions: String,
    ) = model to MavStageHealth(model, applicability, substitutions.toList())

    /**
     * The case the whole health path exists for: every stage answered, and answered about
     * padding. It must not arrive as `Ready`, because a card that matches on `Ready` to draw a
     * number would draw one.
     */
    @Test
    fun a_signal_computed_entirely_from_padding_is_unfounded_not_ready() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("popsicle_ovulation_detection", signal = "cycle_awareness", state = MavStageState.CACHED)),
            completedAtMs = mapOf("popsicle_ovulation_detection" to 1_000L),
            health = mapOf(
                health("popsicle_ovulation_detection", MavApplicability.UNFOUNDED, "out_of_range"),
            ),
        )
        assertEquals(
            MavSignalState.Unfounded(atMs = 1_000L, substitutions = listOf("out_of_range")),
            signals.single().state,
        )
    }

    /** Degraded still reads, and says so. */
    @Test
    fun a_partly_substituted_signal_is_ready_and_carries_the_qualification() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("cva_encoder", state = MavStageState.CACHED)),
            completedAtMs = mapOf("cva_encoder" to 5L),
            health = mapOf(health("cva_encoder", MavApplicability.DEGRADED, "padded")),
        )
        assertEquals(
            MavSignalState.Ready(atMs = 5L, displayable = true, applicability = MavApplicability.DEGRADED),
            signals.single().state,
        )
    }

    /**
     * A signal is only as sound as its weakest stage. Taking the best would let one complete
     * model vouch for the padded ones beside it.
     */
    @Test
    fun a_signal_takes_the_worst_verdict_among_its_stages() {
        val signals = MavSignalReducer.reduce(
            listOf(
                stage("cva_encoder", state = MavStageState.CACHED),
                stage("cva_probes_male", state = MavStageState.CACHED),
            ),
            completedAtMs = mapOf("cva_encoder" to 9L, "cva_probes_male" to 9L),
            health = mapOf(
                health("cva_encoder", MavApplicability.SOUND),
                health("cva_probes_male", MavApplicability.UNFOUNDED, "missing"),
            ),
        )
        assertTrue(signals.single().state is MavSignalState.Unfounded)
    }

    /** A stage the core never measured must not be reported worse than one it measured and passed. */
    @Test
    fun an_unmeasured_stage_does_not_degrade_a_sound_signal() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("cva_encoder", state = MavStageState.CACHED)),
            completedAtMs = mapOf("cva_encoder" to 3L),
        )
        assertEquals(
            MavSignalState.Ready(atMs = 3L, displayable = true, applicability = MavApplicability.SOUND),
            signals.single().state,
        )
    }

    @Test
    fun the_worst_verdict_ranks_unfounded_above_degraded_above_unmeasured() {
        assertEquals(MavApplicability.SOUND, MavApplicability.worst(emptyList()))
        assertEquals(
            MavApplicability.UNFOUNDED,
            MavApplicability.worst(listOf(MavApplicability.SOUND, MavApplicability.DEGRADED, MavApplicability.UNFOUNDED)),
        )
        assertEquals(
            MavApplicability.DEGRADED,
            MavApplicability.worst(listOf(MavApplicability.SOUND, MavApplicability.DEGRADED, MavApplicability.UNMEASURED)),
        )
    }

    /** An unknown wire name must never be read as the flattering answer. */
    @Test
    fun an_unrecognised_verdict_parses_as_unmeasured_rather_than_sound() {
        assertEquals(MavApplicability.SOUND, MavApplicability.parse("sound"))
        assertEquals(MavApplicability.DEGRADED, MavApplicability.parse("degraded"))
        assertEquals(MavApplicability.UNFOUNDED, MavApplicability.parse("unfounded"))
        assertEquals(MavApplicability.UNMEASURED, MavApplicability.parse("unmeasured"))
        assertEquals(MavApplicability.UNMEASURED, MavApplicability.parse("a_verdict_from_a_newer_core"))
    }

    /** An unfounded verdict outranks staleness: the reading was never founded to go stale. */
    @Test
    fun unfounded_outranks_stale() {
        val signals = MavSignalReducer.reduce(
            listOf(stage("cva_encoder", state = MavStageState.CACHED)),
            completedAtMs = mapOf("cva_encoder" to 2L),
            invalidated = setOf("cva_encoder"),
            health = mapOf(health("cva_encoder", MavApplicability.UNFOUNDED, "missing")),
        )
        assertTrue(signals.single().state is MavSignalState.Unfounded)
    }
}
