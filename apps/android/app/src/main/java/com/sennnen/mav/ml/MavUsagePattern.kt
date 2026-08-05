package com.sennnen.mav.ml

import kotlin.math.abs

/**
 * When this wearer tends to open the app, so the work can be done before they do.
 *
 * The whole benefit of background analytics is that a result is already there on open. That only
 * pays off if the work happens *near* the open — too early and it is stale before it is seen, too
 * late and it was pointless. Nobody can be asked what time they check their sleep, so it is
 * learned from when they actually have.
 *
 * Deliberately crude: a count per hour of the local day, decayed so a changed routine takes weeks
 * rather than months to show. Anything more is a model, and a model about when to run models is
 * not a thing this app needs. It never leaves the device and is not a sensor reading.
 */
class MavUsagePattern(
    private val counts: DoubleArray = DoubleArray(HOURS),
) {
    init {
        require(counts.size == HOURS) { "a usage pattern has one bucket per hour of the day" }
    }

    /**
     * Record an app open at [hourOfDay], decaying what came before.
     *
     * Decay on write rather than on read so the pattern is a plain array with no clock in it,
     * which is what makes it storable and testable.
     */
    fun record(hourOfDay: Int): MavUsagePattern {
        require(hourOfDay in 0 until HOURS) { "hour of day is 0..23, not $hourOfDay" }
        val next = DoubleArray(HOURS) { counts[it] * DECAY }
        next[hourOfDay] += 1.0
        return MavUsagePattern(next)
    }

    /** The hours this wearer opens the app most, strongest first. Empty until anything is known. */
    fun likelyHours(limit: Int = 3): List<Int> =
        counts.indices
            .filter { counts[it] > 0.0 }
            .sortedWith(compareByDescending<Int> { counts[it] }.thenBy { it })
            .take(limit)

    /**
     * True when [hourOfDay] is close enough to a likely open to be worth precomputing for.
     *
     * [LEAD_HOURS] ahead, because a background window is granted when the OS feels like it, not
     * when it is asked for; aiming at the hour itself would routinely land after the open.
     */
    fun shouldPrecomputeAt(hourOfDay: Int): Boolean {
        val likely = likelyHours()
        if (likely.isEmpty()) return false
        return likely.any { target ->
            val ahead = circularDistance(hourOfDay, target)
            ahead in 1..LEAD_HOURS
        }
    }

    /** How confident the pattern is, 0 until enough opens have been seen to mean anything. */
    fun confidence(): Double {
        val total = counts.sum()
        if (total < MINIMUM_OPENS) return 0.0
        val peak = counts.maxOrNull() ?: 0.0
        return peak / total
    }

    /** The buckets, for persistence. */
    fun buckets(): DoubleArray = counts.copyOf()

    /** Distance in hours from [from] forward to [to], wrapping at midnight. */
    private fun circularDistance(from: Int, to: Int): Int {
        val forward = (to - from + HOURS) % HOURS
        return forward
    }

    companion object {
        const val HOURS: Int = 24

        /**
         * Applied to every bucket on each open. At roughly two opens a day this halves a habit's
         * weight in about three weeks, which is fast enough to follow a new job and slow enough
         * that one holiday does not erase a year.
         */
        const val DECAY: Double = 0.98

        /** How far ahead of a likely open to aim. */
        const val LEAD_HOURS: Int = 2

        /** Below this many recorded opens the pattern says nothing. */
        const val MINIMUM_OPENS: Double = 5.0

        fun fromBuckets(buckets: DoubleArray): MavUsagePattern =
            if (buckets.size == HOURS) MavUsagePattern(buckets.copyOf()) else MavUsagePattern()

        /** Parse the persisted form; anything malformed is an empty pattern, never a crash. */
        fun parse(encoded: String?): MavUsagePattern {
            if (encoded.isNullOrBlank()) return MavUsagePattern()
            val parts = encoded.split(',')
            if (parts.size != HOURS) return MavUsagePattern()
            val buckets = DoubleArray(HOURS)
            for (index in parts.indices) {
                val value = parts[index].toDoubleOrNull() ?: return MavUsagePattern()
                if (!value.isFinite() || value < 0.0) return MavUsagePattern()
                buckets[index] = value
            }
            return MavUsagePattern(buckets)
        }
    }

    /**
     * The persisted form: one value per hour, comma separated.
     *
     * `Double.toString` rather than a formatted width, for two reasons. It round-trips exactly,
     * so a decayed bucket does not lose a little more of itself on every app launch. And it is
     * locale-independent: `String.format("%.4f", …)` without an explicit locale writes `0,9800`
     * in half of Europe, which would turn one bucket into two fields and silently reset the
     * wearer's pattern on every read.
     */
    fun encode(): String = counts.joinToString(",") { it.toString() }

    override fun equals(other: Any?): Boolean =
        other is MavUsagePattern && counts.indices.all { abs(counts[it] - other.counts[it]) < 1e-9 }

    override fun hashCode(): Int = counts.contentHashCode()
}
