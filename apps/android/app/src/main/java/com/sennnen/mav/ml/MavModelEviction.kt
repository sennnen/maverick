package com.sennnen.mav.ml

/**
 * Which loaded models to close when the resident set is over budget.
 *
 * Separated from [MavModelRunner] so it can be tested without a device. The runner's half of
 * this is mechanical — close an interpreter, close its delegate, drop the entry — and the part
 * that can be *wrong* is the choosing: evicting a model another thread is executing is a crash
 * in native code, and evicting the one just asked for turns a cache into a treadmill. Neither
 * failure is visible in a benchmark, and both are cheap to assert against here.
 *
 * The budget is on *idle* models. Whatever is being inferred is exempt, so a single model
 * larger than the whole budget still runs; it simply does not get to keep company. That
 * matters on this zoo, where `pulse_ppg` alone is 436 MB of the 1.05 GB the forty-one cost
 * together.
 */
internal object MavModelEviction {
    /** One resident model, as the policy needs to see it. */
    data class Candidate(
        val slug: String,
        val nativeBytes: Long,
        /** True while a thread is inside an inference for this model. */
        val inUse: Boolean,
    )

    /**
     * Choose what to close, given [candidates] in least-recently-used order first.
     *
     * Returns the slugs to evict, in the order they should go. Stops as soon as the total is
     * within [budgetBytes]; returns empty when it already is, so the common call does no work
     * beyond one sum.
     */
    fun choose(
        candidates: List<Candidate>,
        budgetBytes: Long,
        keeping: String,
    ): List<String> {
        var resident = candidates.sumOf { it.nativeBytes }
        if (resident <= budgetBytes) return emptyList()
        val evicting = mutableListOf<String>()
        for (candidate in candidates) {
            if (resident <= budgetBytes) break
            if (candidate.slug == keeping || candidate.inUse) continue
            evicting += candidate.slug
            resident -= candidate.nativeBytes
        }
        return evicting
    }
}
