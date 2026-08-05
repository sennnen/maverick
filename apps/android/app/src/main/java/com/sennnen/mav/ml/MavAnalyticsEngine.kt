package com.sennnen.mav.ml

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.ensureActive
import kotlin.coroutines.coroutineContext

/**
 * The production analytics loop: plan, queue, drain, publish.
 *
 * Until this existed the forty-one models in the bundle had no caller. `MavModelBridge` was
 * referenced by its own tests and by nothing else, and `MavMlSignals` published three hardcoded
 * nulls. Everything about *which* models to run and in what order lives in the Rust core, so
 * what is left here is genuinely only the platform's half:
 *
 * - **When.** Foreground open, background window, or an explicit retry.
 * - **How hard.** Interactive bursts; deferred passes trickle. The core turns that into a stage
 *   count; this class decides which mode it is in.
 * - **Off the main thread.** Every call below runs on [dispatcher]. Nothing here touches a view.
 * - **Once at a time.** One pass holds [pass]; a second caller returns rather than queueing, so
 *   an app resumed twice in a second does not run the night twice.
 *
 * Cancellation is cooperative and checked between stages: a pass that is cancelled mid-drain
 * cancels the inference it was waiting on so the core's queue does not keep a request in flight
 * for a result nobody will collect.
 */
class MavAnalyticsEngine(
    private val runtime: MavAnalyticsRuntime,
    private val runner: MavModelBridge.Runner,
    private val clock: () -> Long = System::currentTimeMillis,
    private val dispatcher: CoroutineDispatcher = Dispatchers.Default,
) {
    private val mutable = MutableStateFlow(MavAnalyticsSnapshot())
    val snapshot: StateFlow<MavAnalyticsSnapshot> = mutable.asStateFlow()

    private val pass = Mutex()
    private val failures = mutableMapOf<String, Int>()

    /**
     * Where housekeeping that must not block its caller runs. Deliberately not a view model's
     * scope: this engine outlives any one of those, and a release cancelled by a rotation would
     * leave the models resident it was asked to drop.
     */
    private val maintenance = CoroutineScope(dispatcher + SupervisorJob())

    /**
     * Run one pass for [deviceId].
     *
     * Returns what the pass achieved. A pass that could not start because another was already
     * running reports [Outcome.SKIPPED_BUSY] rather than waiting: the caller is a lifecycle
     * callback, and blocking one behind an inference is how a resume becomes a jank report.
     */
    suspend fun runPass(
        deviceId: ULong,
        mode: MavRunMode,
        permissionMissing: String? = null,
    ): Outcome {
        if (!pass.tryLock()) return Outcome.SKIPPED_BUSY
        try {
            return withContext(dispatcher) { onePass(deviceId, mode, permissionMissing) }
        } finally {
            pass.unlock()
        }
    }

    private suspend fun onePass(
        deviceId: ULong,
        mode: MavRunMode,
        permissionMissing: String?,
    ): Outcome {
        mutable.value = mutable.value.copy(working = true)
        try {
            val now = clock()
            // Queue whatever the day's stored optical signal can feed. The core reads the store
            // and builds the tensors; nothing here ever sees a raw sample.
            runCatching { runtime.admitPpgStages(deviceId, now) }
                .onFailure { return failPass() }

            var failed = 0
            // One bridge and one host for the whole pass. Building them per round allocated a
            // fresh `Host` adapter up to sixteen times for a queue that is usually empty by the
            // second.
            val bridge = MavModelBridge(runtime.host(), runner, clock)
            // Drain until the queue is empty or the burst is spent. Each drained encoder can
            // queue its heads inside the core, so "empty" is the real terminating condition
            // rather than a fixed count.
            var rounds = 0
            while (rounds < MAX_ROUNDS) {
                coroutineContext.ensureActive()
                rounds += 1
                val outcome = bridge.drain(mode.burst)
                failed += outcome.failed
                if (outcome.completed == 0 && outcome.failed == 0) break
            }

            val plan = runtime.plan(deviceId, now, mode, runtime.profileFields())
            val cached = runtime.cacheCompletedAt()
            recordFailures(plan.stages, failed)
            mutable.value = MavAnalyticsSnapshot(
                signals = MavSignalReducer.reduce(
                    stages = plan.stages,
                    coverage = plan.coverage,
                    completedAtMs = cached,
                    failures = failureCounts(),
                    deferred = mode == MavRunMode.DEFERRED && failed > 0,
                    missingPermission = permissionMissing,
                ),
                working = false,
                lastPassAtMs = now,
            )
            return if (failed > 0) Outcome.PARTIAL else Outcome.COMPLETED
        } finally {
            mutable.value = mutable.value.copy(working = false)
        }
    }

    /**
     * Count a failure against every stage that was ready and did not complete.
     *
     * Attributing it to specific models rather than to the pass is what lets one broken model
     * exhaust its own budget while the rest keep retrying — the alternative is a single global
     * failure that stops the whole zoo because one artefact is missing.
     */
    private fun recordFailures(plan: List<MavPlannedStage>, failed: Int) = synchronized(failures) {
        if (failed <= 0) {
            for (stage in plan.filter { it.state == MavStageState.CACHED }) failures.remove(stage.model)
            return
        }
        for (stage in plan.filter { it.state == MavStageState.READY }) {
            failures[stage.model] = (failures[stage.model] ?: 0) + 1
        }
    }

    /** A stable copy of the retry budgets, for the reducer. Taken under the same monitor. */
    private fun failureCounts(): Map<String, Int> = synchronized(failures) { failures.toMap() }

    /**
     * The pass could not even be planned.
     *
     * The `finally` in [onePass] clears `working` either way; this exists so the failure has one
     * name rather than a bare `return Outcome.FAILED` inside a lambda. Nothing is logged here on
     * purpose — the core has already journalled the error with its own code, and a second line
     * from the platform would be the same fact under a worse name.
     */
    private fun failPass(): Outcome = Outcome.FAILED

    /**
     * Forget every retry budget, so a wearer tapping retry gets a genuine fresh attempt.
     *
     * Synchronised on the same monitor [recordFailures] uses. The map is touched from whichever
     * thread the dispatcher gave the pass and from the main thread that handled the tap, and an
     * unsynchronised `HashMap` written from two threads can corrupt its own table rather than
     * merely losing an update.
     */
    fun resetRetries() {
        synchronized(failures) { failures.clear() }
    }

    /**
     * Drop whatever the runner is holding resident.
     *
     * Behind the pass lock rather than immediate, because releasing a model out from under a
     * running inference is at best a reload and at worst a fault in native code — and off the
     * caller's thread rather than blocking it, because the caller is `Activity.onStop` and a pass
     * over the zoo takes seconds. Late is fine; an ANR is not.
     */
    fun releaseRunnerCache() {
        maintenance.launch {
            pass.withLock { runner.releaseCache() }
        }
    }

    enum class Outcome { COMPLETED, PARTIAL, FAILED, SKIPPED_BUSY }

    private companion object {
        /**
         * Bound on drain rounds in one pass. The core's queue is bounded at 32 and each round
         * empties up to a burst of it, so this is far above any real terminating case; it exists
         * so a core that somehow kept re-queueing could not spin a background window away.
         */
        const val MAX_ROUNDS = 16
    }
}

/** Interactive or deferred, mirroring `mav_engine::analytics::RunMode`. */
enum class MavRunMode(val wire: String, val burst: Int) {
    /** The wearer is looking at the screen. */
    INTERACTIVE("interactive", 32),

    /** Nobody is watching; leave the accelerator alone between stages. */
    DEFERRED("deferred", 4),
}

/**
 * The core calls this engine needs, behind an interface.
 *
 * Not for indirection's sake: the generated uniffi `MavRuntime` cannot be constructed without a
 * database and a compiled core, so an interface here is what lets every state transition above be
 * tested on a JVM with no device, no model and no Rust.
 */
interface MavAnalyticsRuntime {
    fun host(): MavModelBridge.Host

    fun admitPpgStages(deviceId: ULong, atMs: Long)

    fun plan(
        deviceId: ULong,
        atMs: Long,
        mode: MavRunMode,
        profileFields: List<String>,
    ): MavPlan

    fun profileFields(): List<String>

    /** When each model last answered, from the core's persisted cache. */
    fun cacheCompletedAt(): Map<String, Long>
}
