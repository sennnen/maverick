package com.sennnen.mav.analytics

/**
 * Overnight heads-up signal bundle (cycle awareness + illness ward). In Maverick this is computed
 * nightly from stored days; Mav will publish it from the core once those analytics are admitted.
 * Only the fields the Aura recovery hub reads are carried.
 */
object V5HealthSignals {
    data class Snapshot(
        val cycle: CyclePhaseEngine.Result,
        val illness: IllnessSignalEngine.Result,
    )
}
