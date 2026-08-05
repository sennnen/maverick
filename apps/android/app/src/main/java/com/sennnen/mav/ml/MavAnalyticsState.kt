package com.sennnen.mav.ml

/**
 * What one product signal is doing, and what a surface may say about it.
 *
 * Kept as plain data with no Android types so the state machine can be tested on the host. The
 * states a wearer can actually end up in are the reason this is an exhaustive sealed hierarchy
 * rather than a handful of booleans: "not ready" covers at least six genuinely different
 * situations, and a card that renders them identically is a card that lies about five of them.
 */
sealed interface MavSignalState {
    /** Nothing has been attempted yet this session. */
    data object Idle : MavSignalState

    /** Work is queued or running now. [done] of [total] stages have answered. */
    data class Working(val done: Int, val total: Int) : MavSignalState

    /**
     * Every stage answered. [atMs] is when the last one did, so a surface can age the reading
     * rather than presenting an overnight result as current.
     */
    data class Ready(val atMs: Long, val displayable: Boolean) : MavSignalState

    /**
     * Answered, but from inputs that have since moved. The previous values stay on screen —
     * blanking a good reading because a newer one is pending is worse than labelling it.
     */
    data class Stale(val atMs: Long, val displayable: Boolean) : MavSignalState

    /**
     * Nothing here can run on this device as it stands. [reasons] carries one entry per stage,
     * already collapsed to the distinct causes, so the card says "needs a strap that reports
     * SpO2" and not "5 models unavailable".
     */
    data class Unavailable(val reasons: List<MavUnavailable>) : MavSignalState

    /** The OS declined or postponed the work. It will be retried when the app is next open. */
    data object Deferred : MavSignalState

    /**
     * A stage failed. [attempts] is how many times it has been tried; [retryable] is false once
     * the budget is spent, which is what turns a spinner into a retry button.
     */
    data class Failed(val message: String, val attempts: Int, val retryable: Boolean) :
        MavSignalState

    /** A permission the work needs has not been granted. */
    data class PermissionRequired(val permission: String) : MavSignalState
}

/**
 * Why one stage cannot run, mirroring `mav_engine::analytics::Unmet` across the FFI.
 *
 * The distinction between these four is the whole point. A missing sensor is answered by a
 * different strap; a missing profile field is answered by a tap; an unported front-end is
 * answered by neither and should not send anyone shopping.
 */
sealed interface MavUnavailable {
    data class MissingStreams(val streams: List<String>) : MavUnavailable

    data class MissingProfile(val fields: List<String>) : MavUnavailable

    data class UpstreamUnavailable(val model: String) : MavUnavailable

    data class PreprocessingNotPorted(val detail: String) : MavUnavailable
}

/** One signal as the UI reads it. */
data class MavSignal(
    val name: String,
    val state: MavSignalState,
    /** Total stages in this signal, including the ones that cannot run. */
    val total: Int,
    /** Stages that could run on this device. */
    val runnable: Int,
)

/** Everything the analytics surface renders. */
data class MavAnalyticsSnapshot(
    val signals: List<MavSignal> = emptyList(),
    /** True while any pass is in flight, for the one global spinner. */
    val working: Boolean = false,
    /** When the last complete pass finished, or null before the first. */
    val lastPassAtMs: Long? = null,
) {
    fun signal(name: String): MavSignal? = signals.firstOrNull { it.name == name }
}

/**
 * Turns one core plan into the states the UI renders.
 *
 * Pure on purpose. Everything that decides what a wearer is told about their data happens here,
 * and none of it needs a device, a runtime, or a model to prove.
 */
object MavSignalReducer {
    /** How many times one stage is retried before a surface offers the wearer the button. */
    const val RETRY_BUDGET: Int = 3

    /**
     * @param stages the plan's per-model rows, as the FFI reports them.
     * @param completedAtMs when each model last answered, from the persisted cache.
     * @param invalidated models whose remembered result no longer matches the current inputs.
     * @param failures attempts so far per model, kept by the engine across retries.
     * @param deferred true when the OS declined the background window this pass.
     * @param missingPermission a permission the work needs and does not have, if any.
     */
    fun reduce(
        stages: List<MavPlannedStage>,
        completedAtMs: Map<String, Long> = emptyMap(),
        invalidated: Set<String> = emptySet(),
        failures: Map<String, Int> = emptyMap(),
        deferred: Boolean = false,
        missingPermission: String? = null,
    ): List<MavSignal> =
        stages.groupBy { it.signal }.map { (name, group) ->
            MavSignal(
                name = name,
                state = stateOf(group, completedAtMs, invalidated, failures, deferred, missingPermission),
                total = group.size,
                runnable = group.count { it.state != MavStageState.UNAVAILABLE },
            )
        }

    private fun stateOf(
        group: List<MavPlannedStage>,
        completedAtMs: Map<String, Long>,
        invalidated: Set<String>,
        failures: Map<String, Int>,
        deferred: Boolean,
        missingPermission: String?,
    ): MavSignalState {
        // A permission the work cannot proceed without outranks everything: the wearer can fix
        // it, and every other state would be describing a consequence rather than the cause.
        if (missingPermission != null) return MavSignalState.PermissionRequired(missingPermission)

        val runnable = group.filter { it.state != MavStageState.UNAVAILABLE }
        if (runnable.isEmpty()) {
            return MavSignalState.Unavailable(group.mapNotNull { it.unavailable }.distinct())
        }

        // A failure that has spent its budget is the most useful thing to say next: the work is
        // not going to finish on its own, and the wearer is the one who decides whether to retry.
        val spent = runnable.firstOrNull { (failures[it.model] ?: 0) >= RETRY_BUDGET }
        if (spent != null) {
            return MavSignalState.Failed(
                message = spent.model,
                attempts = failures[spent.model] ?: 0,
                retryable = true,
            )
        }

        val done = runnable.count { it.state == MavStageState.CACHED }
        if (done < runnable.size) {
            // Deferred only matters while something is genuinely outstanding; a signal that
            // finished before the OS said no is finished.
            return if (deferred) MavSignalState.Deferred else MavSignalState.Working(done, runnable.size)
        }

        val displayable = runnable.any { it.displayable }
        val at = runnable.mapNotNull { completedAtMs[it.model] }.maxOrNull() ?: 0L
        return if (runnable.any { invalidated.contains(it.model) }) {
            MavSignalState.Stale(at, displayable)
        } else {
            MavSignalState.Ready(at, displayable)
        }
    }
}

/** The states the core's plan reports, as an enum the reducer can switch on exhaustively. */
enum class MavStageState { READY, BLOCKED, CACHED, UNAVAILABLE }

/** One plan row, decoupled from the generated uniffi record so the reducer is host-testable. */
data class MavPlannedStage(
    val model: String,
    val signal: String,
    val state: MavStageState,
    val displayable: Boolean,
    val unavailable: MavUnavailable? = null,
)
