package com.sennnen.mav

import com.sennnen.mav.ml.MavAnalyticsEngine
import com.sennnen.mav.ml.MavAnalyticsRuntime
import com.sennnen.mav.ml.MavModelBridge
import com.sennnen.mav.ml.MavPlannedStage
import com.sennnen.mav.ml.MavRunMode
import com.sennnen.mav.ml.MavSignalState
import com.sennnen.mav.ml.MavStageState
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.mav_ffi.ModelInferenceRequest
import uniffi.mav_ffi.ModelInferenceResult
import uniffi.mav_ffi.ModelTensor

/**
 * The production loop, driven against a queue this test controls.
 *
 * The real runtime needs a database, a compiled core and a bundled model, none of which a JVM
 * unit test has. What is worth testing here is not the inference — that is covered on device —
 * but the loop around it: that a pass drains until the queue empties, that a second pass does not
 * run the zoo twice, that a failure is attributed and retried, and that the clock the core is
 * told about is the platform's.
 */
class AnalyticsEngineTest {

    /** A queue of requests the drain loop will find, and a record of what came back. */
    private class FakeHost(requests: List<String>) : MavModelBridge.Host {
        val pending = ArrayDeque(requests)
        val submitted = mutableListOf<Triple<ULong, String, Long>>()
        val cancelled = mutableListOf<ULong>()
        var failOn: String? = null
        private var nextId = 1UL

        override fun nextModelInference(): ModelInferenceRequest? {
            val slug = pending.removeFirstOrNull() ?: return null
            return ModelInferenceRequest(nextId++, slug, listOf(ModelTensor("in", listOf(0.5f))))
        }

        override fun submitModelInference(
            requestId: ULong,
            outputs: List<ModelTensor>,
            modelSha256: String,
            completedAtMs: Long,
        ): ModelInferenceResult {
            submitted += Triple(requestId, modelSha256, completedAtMs)
            return ModelInferenceResult(requestId, "slug", outputs, modelSha256)
        }

        override fun cancelModelInference(requestId: ULong): Boolean {
            cancelled += requestId
            return true
        }
    }

    private class FakeRunner(private val host: FakeHost) : MavModelBridge.Runner {
        var ran = 0
        override fun run(slug: String, inputs: Map<String, FloatArray>): Map<String, FloatArray> {
            ran += 1
            if (slug == host.failOn) throw IllegalStateException("$slug is not in the bundle")
            return mapOf("out" to floatArrayOf(1.0f))
        }

        override fun loadedSha256(slug: String): String = "a".repeat(64)
    }

    private class FakeRuntime(
        private val host: FakeHost,
        var stages: List<MavPlannedStage> = emptyList(),
        var completedAt: Map<String, Long> = emptyMap(),
    ) : MavAnalyticsRuntime {
        var admitCalls = 0
        var admitGate: CompletableDeferred<Unit>? = null

        override fun host(): MavModelBridge.Host = host

        override fun admitPpgStages(deviceId: ULong, atMs: Long) {
            admitCalls += 1
        }

        override fun plan(
            deviceId: ULong,
            atMs: Long,
            mode: MavRunMode,
            profileFields: List<String>,
        ): List<MavPlannedStage> = stages

        override fun profileFields(): List<String> = listOf("sex", "age", "height", "weight")

        override fun cacheCompletedAt(): Map<String, Long> = completedAt
    }

    private fun engine(
        runtime: FakeRuntime,
        runner: MavModelBridge.Runner,
        now: Long = 1_700_000_000_000L,
    ) = MavAnalyticsEngine(
        runtime = runtime,
        runner = runner,
        clock = { now },
        dispatcher = UnconfinedTestDispatcher(),
    )

    @Test
    fun a_pass_drains_until_the_queue_is_empty() = runTest {
        val host = FakeHost(listOf("pulse_ppg", "pulsenet_foundation", "cva_encoder"))
        val runner = FakeRunner(host)
        val runtime = FakeRuntime(host)
        val outcome = engine(runtime, runner).runPass(1UL, MavRunMode.INTERACTIVE)

        assertEquals(MavAnalyticsEngine.Outcome.COMPLETED, outcome)
        assertEquals("every queued request should have run", 3, runner.ran)
        assertEquals(3, host.submitted.size)
        assertEquals(1, runtime.admitCalls)
    }

    /**
     * The core reads no clock. If the platform stopped sending one the cache would file every
     * result at the epoch and every reading would look decades stale.
     */
    @Test
    fun the_platform_clock_travels_with_every_result() = runTest {
        val host = FakeHost(listOf("pulse_ppg"))
        engine(FakeRuntime(host), FakeRunner(host), now = 1_234_567L)
            .runPass(1UL, MavRunMode.INTERACTIVE)
        assertEquals(listOf(1_234_567L), host.submitted.map { it.third })
    }

    @Test
    fun a_model_that_cannot_run_is_cancelled_rather_than_left_in_flight() = runTest {
        val host = FakeHost(listOf("pulse_ppg", "cva_encoder"))
        host.failOn = "pulse_ppg"
        val runner = FakeRunner(host)
        val runtime = FakeRuntime(host)
        val outcome = engine(runtime, runner).runPass(1UL, MavRunMode.INTERACTIVE)

        assertEquals(MavAnalyticsEngine.Outcome.PARTIAL, outcome)
        assertEquals(
            "the failed request must not stall every later inference behind it",
            1,
            host.cancelled.size,
        )
        assertEquals("the other model still ran", 1, host.submitted.size)
    }

    @Test
    fun a_failure_is_counted_against_the_stage_that_was_ready() = runTest {
        val host = FakeHost(listOf("pulse_ppg"))
        host.failOn = "pulse_ppg"
        val runtime = FakeRuntime(
            host,
            stages = listOf(
                MavPlannedStage("pulse_ppg", "ppg_foundation", MavStageState.READY, false),
            ),
        )
        val engine = engine(runtime, FakeRunner(host))

        repeat(3) {
            host.pending.addLast("pulse_ppg")
            engine.runPass(1UL, MavRunMode.INTERACTIVE)
        }
        val state = engine.snapshot.value.signals.single().state
        assertTrue("three failures should exhaust the budget: $state", state is MavSignalState.Failed)
    }

    @Test
    fun clearing_the_retries_lets_a_failed_signal_be_tried_again() = runTest {
        val host = FakeHost(emptyList())
        host.failOn = "pulse_ppg"
        val runtime = FakeRuntime(
            host,
            stages = listOf(
                MavPlannedStage("pulse_ppg", "ppg_foundation", MavStageState.READY, false),
            ),
        )
        val engine = engine(runtime, FakeRunner(host))
        repeat(3) {
            host.pending.addLast("pulse_ppg")
            engine.runPass(1UL, MavRunMode.INTERACTIVE)
        }
        assertTrue(engine.snapshot.value.signals.single().state is MavSignalState.Failed)

        engine.resetRetries()
        engine.runPass(1UL, MavRunMode.INTERACTIVE)
        assertTrue(
            "after a reset the signal should be working again, not still failed",
            engine.snapshot.value.signals.single().state is MavSignalState.Working,
        )
    }

    @Test
    fun a_completed_signal_reports_when_it_completed() = runTest {
        val host = FakeHost(listOf("cva_encoder"))
        val runtime = FakeRuntime(
            host,
            stages = listOf(
                MavPlannedStage("cva_encoder", "cardiovascular", MavStageState.CACHED, true),
            ),
            completedAt = mapOf("cva_encoder" to 99L),
        )
        engine(runtime, FakeRunner(host)).runPass(1UL, MavRunMode.INTERACTIVE)
        assertEquals(
            MavSignalState.Ready(atMs = 99L, displayable = true),
            engine(runtime, FakeRunner(host)).let {
                it.runPass(1UL, MavRunMode.INTERACTIVE)
                it.snapshot.value.signals.single().state
            },
        )
    }

    @Test
    fun the_snapshot_stops_reporting_work_once_a_pass_ends() = runTest {
        val host = FakeHost(listOf("pulse_ppg"))
        val engine = engine(FakeRuntime(host), FakeRunner(host))
        engine.runPass(1UL, MavRunMode.INTERACTIVE)
        assertTrue(!engine.snapshot.value.working)
        assertEquals(1_700_000_000_000L, engine.snapshot.value.lastPassAtMs)
    }

    @Test
    fun a_pass_with_nothing_queued_completes_without_running_anything() = runTest {
        val host = FakeHost(emptyList())
        val runner = FakeRunner(host)
        val outcome = engine(FakeRuntime(host), runner).runPass(1UL, MavRunMode.DEFERRED)
        assertEquals(MavAnalyticsEngine.Outcome.COMPLETED, outcome)
        assertEquals(0, runner.ran)
    }
}
