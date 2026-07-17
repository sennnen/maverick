package com.sennnen.mav.analytics

import com.sennnen.mav.data.DailyMetric
import kotlin.math.min
import kotlin.math.roundToInt


/*
 * RestScorer — Maverick "Rest" (sleep_performance) composite, 0–100.
 *
 * Faithful Kotlin mirror of the Swift Rest composite (AnalyticsEngine / RestScorer). Keep every
 * constant and the weight set byte-identical to Swift — parity tests enforce it.
 *
 *   Rest = 0.50·duration + 0.20·efficiency + 0.20·restorative + 0.10·consistency
 *
 * Each sub-component is itself on 0–100:
 *   duration     — asleep hours / personal need, clamped at 100 (8 h default, refined by recent avg).
 *   efficiency   — asleep / in-bed (0..1) × 100.
 *   restorative  — (deep + REM) / asleep share, normalized by a healthy target share, clamped 100.
 *   consistency  — sleep/wake regularity (0..1) × 100; when the caller has no history it is null and
 *                  the term DROPS, renormalizing the remaining weights (same discipline as recovery).
 *
 * Outputs APPROXIMATE — not WHOOP's proprietary Sleep Performance.
 */
object RestScorer {

    /** Component weights (sum 1.0 when all present). Byte-identical to Swift. */
    const val wDuration: Double = 0.50
    const val wEfficiency: Double = 0.20
    const val wRestorative: Double = 0.20
    const val wConsistency: Double = 0.10

    /** Default personal sleep need (hours) before any recent-average refinement. */
    const val defaultSleepNeedHours: Double = 8.0

    /**
     * Healthy restorative (deep + REM) share of asleep time. A share at/above this earns full
     * restorative credit; below it scales linearly. ~0.50 reflects ~20% deep + ~25–30% REM in a
     * well-structured night.
     */
    const val restorativeTargetShare: Double = 0.50

    /**
     * Deep-sleep share of asleep time that earns FULL restorative credit (~13% is the healthy floor
     * for adults; below it the restorative term scales down toward [deepFloorFactor]). DEEP honesty
     * (Reddit HRV/sleep report): pooling deep+REM let a night with normal REM but almost no DEEP earn
     * near-full restorative credit (Rest read 95+ with little deep). Byte-identical to Swift.
     */
    const val deepShareTarget: Double = 0.13

    /** Most the restorative term is scaled down when deep is ~absent — half, never zeroed, so a
     *  low-deep night reads honestly without the whole night tanking. Swift parity. */
    const val deepFloorFactor: Double = 0.5

    /** Neutral consistency (fraction) used when the caller supplies no regularity signal. Swift parity. */
    const val NEUTRAL_CONSISTENCY: Double = 0.5

    /**
     * Rest composite [0,100], or null when there is no asleep time.
     *
     * @param asleepSeconds total sleep time (TST) for the night, seconds.
     * @param efficiency asleep / in-bed in [0,1].
     * @param deepSeconds deep-stage seconds.
     * @param remSeconds REM-stage seconds.
     * @param sleepNeedHours personal need (hours); null → [defaultSleepNeedHours].
     * @param consistency sleep/wake regularity in [0,1]; null drops the term + renormalizes.
     */
    fun rest(
        asleepSeconds: Double,
        efficiency: Double,
        deepSeconds: Double,
        remSeconds: Double,
        sleepNeedHours: Double? = null,
        consistency: Double? = null,
    ): Double? {
        if (asleepSeconds <= 0.0) return null

        val asleepHours = asleepSeconds / 3600.0
        val needHours = (sleepNeedHours ?: defaultSleepNeedHours).coerceAtLeast(1e-9)

        // Duration vs personal need (clamped at 100 — sleeping past need does not over-credit).
        val durationScore = min(100.0, asleepHours / needHours * 100.0)
        // Efficiency (0..1 → 0..100), clamped.
        val efficiencyScore = (efficiency * 100.0).coerceIn(0.0, 100.0)
        // Restorative share vs healthy target (clamped at 100), then scaled by a gentle deep-adequacy
        // factor in [deepFloorFactor, 1]: full once deep ≥ target share, ramping to the floor as
        // deep → 0, so a near-zero-deep night loses up to half this term (~10 pts) — honest, not
        // tanking, no fabricated stages. Mirrors Swift Rest.composite EXACTLY.
        val restorativeShare = (deepSeconds + remSeconds) / asleepSeconds
        val deepAdequacy = ((deepSeconds / asleepSeconds) / deepShareTarget).coerceIn(0.0, 1.0)
        val deepFactor = deepFloorFactor + (1.0 - deepFloorFactor) * deepAdequacy
        val restorativeScore = min(100.0, restorativeShare / restorativeTargetShare * 100.0) * deepFactor

        // Consistency uses a NEUTRAL 0.5 (→50) when the caller supplies none — matching the Swift
        // Rest.composite EXACTLY (parity is required; Swift adds a neutral term, it does NOT drop +
        // renormalize). Weights sum to 1.0 so the weighted sum is already on 0..100.
        val consistencyScore = ((consistency ?: NEUTRAL_CONSISTENCY) * 100.0).coerceIn(0.0, 100.0)
        val weighted = wDuration * durationScore +
            wEfficiency * efficiencyScore +
            wRestorative * restorativeScore +
            wConsistency * consistencyScore
        return (weighted * 100.0).roundToInt() / 100.0
    }

    /**
     * Sleep & Rest test-mode (E11) diagnostic line for the Rest composite. Recomputes the four weighted
     * sub-scores from the SAME inputs `rest()` reads (on the 0..1 scale, byte-aligned with the Swift
     * `Rest.subScoreLine`), and reuses `rest()` for the final `composite=` value so the trace can never
     * disagree with the score. `groupFragments` / `groupInBedSeconds` describe the main-night GROUP
     * composition (#525/#561). Pure, side-effect-free, no em-dashes. Mirrors Swift exactly.
     */
    fun subScoreLine(
        tstSeconds: Double, inBedSeconds: Double, efficiency: Double, restorativeSeconds: Double,
        needHours: Double, consistency: Double?, deepSeconds: Double?,
        groupFragments: Int, groupInBedSeconds: Double,
    ): String {
        fun clamp01(x: Double) = maxOf(0.0, minOf(1.0, x))
        fun r2(x: Double) = Math.round(x * 100.0) / 100.0
        val needSeconds = maxOf(needHours, 0.1) * 3600.0
        val durationScore = clamp01(tstSeconds / needSeconds)
        val efficiencyScore = clamp01(efficiency)
        val deepFactor = if (deepSeconds != null && tstSeconds > 0 && deepShareTarget > 0) {
            val adequacy = clamp01((deepSeconds / tstSeconds) / deepShareTarget)
            deepFloorFactor + (1.0 - deepFloorFactor) * adequacy
        } else 1.0
        val restorativeScore = if (tstSeconds > 0)
            clamp01((restorativeSeconds / tstSeconds) / restorativeTargetShare) * deepFactor else 0.0
        val consistencyScore = clamp01(consistency ?: NEUTRAL_CONSISTENCY)
        // Reuse the real scorer for the composite (cannot diverge). `rest()` takes deep + REM separately;
        // restorative = deep + REM, so REM = restorative - deep. null deep -> 0 deep (no-adequacy path).
        val composite = rest(
            asleepSeconds = tstSeconds, efficiency = efficiency,
            deepSeconds = deepSeconds ?: 0.0,
            remSeconds = restorativeSeconds - (deepSeconds ?: 0.0),
            sleepNeedHours = needHours, consistency = consistency,
        ) ?: 0.0
        return "rest composite=${r2(composite)} " +
            "dur=${r2(durationScore)}*wDur=$wDuration " +
            "eff=${r2(efficiencyScore)}*wEff=$wEfficiency " +
            "restor=${r2(restorativeScore)}*wRestor=$wRestorative deepFactor=${r2(deepFactor)} " +
            "consist=${r2(consistencyScore)}*wConsist=$wConsistency " +
            "group=$groupFragments groupInBedMin=${(groupInBedSeconds / 60).toInt()}"
    }

    /**
     * Rest composite [0,100] derived from a persisted [DailyMetric] (the pass-2 / display path — raw
     * streams are gone but the night's totals remain). null when there's no sleep. Single source of
     * truth so the persisted sleep_performance series and the Charge "Rest quality" term agree. Mirrors
     * Swift `AnalyticsEngine.Rest.composite(daily:)`.
     */
    fun restFromDaily(daily: DailyMetric, consistency: Double? = null): Double? {
        val tstMin = daily.totalSleepMin ?: return null
        val eff = daily.efficiency ?: return null
        if (tstMin <= 0.0) return null
        return rest(
            asleepSeconds = tstMin * 60.0,
            efficiency = eff,
            deepSeconds = (daily.deepMin ?: 0.0) * 60.0,
            remSeconds = (daily.remMin ?: 0.0) * 60.0,
            sleepNeedHours = null,
            consistency = consistency,
        )
    }
}
