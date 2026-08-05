package com.sennnen.mav.ml

import uniffi.mav_ffi.ModelInferenceRequest
import uniffi.mav_ffi.ModelInferenceResult
import uniffi.mav_ffi.ModelTensor

/**
 * Drives the core's inference queue: pull a request, run it, hand the result back.
 *
 * The core decides what to infer and what the numbers mean; the platform decides when. That split
 * is why this is a drain loop the app calls rather than a callback the core invokes — only the app
 * knows whether it is foregrounded and whether the accelerator is free.
 *
 * The loop is bounded per pass. A recompute can queue a night of PPG windows, and running all of
 * them in one turn would block the caller; [drain] takes as many as it was asked for and leaves
 * the rest for the next pass.
 */
class MavModelBridge(
    private val host: Host,
    private val runner: Runner,
    /**
     * The platform's clock. The core reads none of its own, so the one timestamp worth
     * remembering about an inference — when it finished — has to travel with the result.
     */
    private val clock: () -> Long = System::currentTimeMillis,
) {
    /**
     * Anything that can hand out work and take results. The uniffi `MavRuntime` satisfies it;
     * tests substitute a queue they control, so the loop runs without a bundled model.
     */
    interface Host {
        fun nextModelInference(): ModelInferenceRequest?

        fun submitModelInference(
            requestId: ULong,
            outputs: List<ModelTensor>,
            modelSha256: String,
            completedAtMs: Long,
        ): ModelInferenceResult

        fun cancelModelInference(requestId: ULong): Boolean
    }

    /** Anything that can run one model. [MavModelRunner] satisfies it. */
    interface Runner {
        fun run(slug: String, inputs: Map<String, FloatArray>): Map<String, FloatArray>

        fun loadedSha256(slug: String): String
    }

    /**
     * What one drain pass did. Failures are counted, not thrown: one model missing from the
     * bundle must not stop the others from running.
     */
    data class Outcome(val completed: Int = 0, val failed: Int = 0)

    fun drain(limit: Int = 8): Outcome {
        var completed = 0
        var failed = 0
        repeat(maxOf(0, limit)) {
            val request = runCatching { host.nextModelInference() }.getOrNull() ?: return Outcome(completed, failed)
            val ran = runCatching {
                val inputs = request.inputs.associate { it.name to it.values.toFloatArray() }
                val produced = runner.run(request.modelSlug, inputs)
                host.submitModelInference(
                    request.requestId,
                    produced.map { (name, values) -> ModelTensor(name, values.toList()) },
                    runner.loadedSha256(request.modelSlug),
                    clock(),
                )
            }
            if (ran.isSuccess) {
                completed += 1
            } else {
                // The request stays in flight inside core until it is cancelled, so a transient
                // failure could be retried; a missing or mismatched model will not fix itself,
                // and leaving it queued would stall every later inference behind it.
                runCatching { host.cancelModelInference(request.requestId) }
                failed += 1
            }
        }
        return Outcome(completed, failed)
    }
}
