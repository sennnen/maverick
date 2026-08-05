package com.sennnen.mav.ml

import uniffi.mav_ffi.MavRuntime
import uniffi.mav_ffi.ModelInferenceRequest
import uniffi.mav_ffi.ModelInferenceResult
import uniffi.mav_ffi.ModelStageReport
import uniffi.mav_ffi.ModelTensor

/**
 * The generated core, in the shape [MavAnalyticsEngine] asks for.
 *
 * Everything here is translation and nothing is a decision: the plan, the ordering, the reasons
 * and the cache all come from Rust. The one thing this file is allowed to do is turn the core's
 * strings into the Kotlin types the reducer switches on, and it fails loudly on an unrecognised
 * one rather than defaulting — a new unavailable reason should show up as a compile-adjacent
 * bug, not as a card that silently renders as "working".
 */
class MavCoreAnalyticsRuntime(private val runtime: MavRuntime) : MavAnalyticsRuntime {

    override fun host(): MavModelBridge.Host = object : MavModelBridge.Host {
        override fun nextModelInference(): ModelInferenceRequest? = runtime.nextModelInference()

        override fun submitModelInference(
            requestId: ULong,
            outputs: List<ModelTensor>,
            modelSha256: String,
            completedAtMs: Long,
        ): ModelInferenceResult =
            runtime.submitModelInference(requestId, outputs, modelSha256, completedAtMs)

        override fun cancelModelInference(requestId: ULong): Boolean =
            runtime.cancelModelInference(requestId)
    }

    override fun admitPpgStages(deviceId: ULong, atMs: Long) {
        runtime.admitPpgStages(deviceId, atMs)
    }

    override fun plan(
        deviceId: ULong,
        atMs: Long,
        mode: MavRunMode,
        profileFields: List<String>,
    ): MavPlan {
        val report = runtime.analyticsPlan(deviceId, atMs, mode.wire, profileFields)
        return MavPlan(
            stages = report.stages.map(::stageOf),
            coverage = report.coverage.associate {
                it.signal to MavSignalCoverage(it.total.toInt(), it.runnable.toInt())
            },
        )
    }

    override fun profileFields(): List<String> = runtime.wearerProfileFields()

    override fun cacheCompletedAt(): Map<String, Long> =
        runtime.analyticsCache().associate { it.modelSlug to it.completedAtMs }

    private fun stageOf(report: ModelStageReport): MavPlannedStage = MavPlannedStage(
        model = report.modelSlug,
        signal = report.signal,
        state = when (report.state) {
            "ready" -> MavStageState.READY
            "blocked" -> MavStageState.BLOCKED
            "cached" -> MavStageState.CACHED
            "unavailable" -> MavStageState.UNAVAILABLE
            else -> throw MavModelException("the core reported an unknown stage state ${report.state}")
        },
        displayable = report.displayable,
        unavailable = unavailableOf(report),
    )

    private fun unavailableOf(report: ModelStageReport): MavUnavailable? =
        when (report.unavailableReason) {
            null -> null
            "missing_streams" -> MavUnavailable.MissingStreams(report.missingStreams)
            "missing_profile" -> MavUnavailable.MissingProfile(report.missingProfile)
            "upstream_unavailable" ->
                MavUnavailable.UpstreamUnavailable(report.blockingModel.orEmpty())
            "preprocessing_not_ported" ->
                MavUnavailable.PreprocessingNotPorted(report.missingPreprocessing.orEmpty())
            else -> throw MavModelException(
                "the core reported an unknown unavailable reason ${report.unavailableReason}",
            )
        }
}
