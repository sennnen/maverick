package com.sennnen.mav

import com.sennnen.mav.ml.MavAnalyticsWorker
import com.sennnen.mav.ml.MavUsagePattern
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * When to precompute, tested without waiting a week for a habit to form.
 *
 * The failure this guards against is scheduling background work at a time nobody opens the app:
 * the battery is spent and the result is stale by the time it is seen, which is worse than not
 * having run at all.
 */
class UsagePatternTest {

    @Test
    fun a_new_install_has_no_opinion_and_schedules_nothing() {
        val pattern = MavUsagePattern()
        assertEquals(0.0, pattern.confidence(), 0.0)
        assertTrue(pattern.likelyHours().isEmpty())
        assertNull(MavAnalyticsWorker.precomputeDelayMinutes(pattern, hourNow = 9))
    }

    @Test
    fun a_habit_shows_up_as_the_hour_it_happens_in() {
        var pattern = MavUsagePattern()
        repeat(10) { pattern = pattern.record(7) }
        repeat(3) { pattern = pattern.record(22) }
        assertEquals(listOf(7, 22), pattern.likelyHours(2))
    }

    @Test
    fun the_pattern_says_nothing_until_enough_opens_have_been_seen() {
        var pattern = MavUsagePattern()
        repeat(4) { pattern = pattern.record(7) }
        assertEquals("four opens is not a routine", 0.0, pattern.confidence(), 0.0)
        repeat(2) { pattern = pattern.record(7) }
        assertTrue(pattern.confidence() > 0.0)
    }

    @Test
    fun a_changed_routine_eventually_wins() {
        var pattern = MavUsagePattern()
        repeat(20) { pattern = pattern.record(7) }
        repeat(60) { pattern = pattern.record(18) }
        assertEquals(18, pattern.likelyHours(1).single())
    }

    @Test
    fun precompute_is_aimed_ahead_of_the_open_not_at_it() {
        var pattern = MavUsagePattern()
        repeat(10) { pattern = pattern.record(7) }
        // Two hours of lead, so at 03:00 the work is asked for at 05:00.
        val delay = MavAnalyticsWorker.precomputeDelayMinutes(pattern, hourNow = 3)
        assertEquals(2 * 60L, delay)
    }

    @Test
    fun a_target_that_has_already_passed_aims_at_tomorrow_rather_than_now() {
        var pattern = MavUsagePattern()
        repeat(10) { pattern = pattern.record(7) }
        // 05:00 is the lead hour; asking at 05:00 must not schedule a zero-delay pass.
        val delay = MavAnalyticsWorker.precomputeDelayMinutes(pattern, hourNow = 5)
        assertEquals(24 * 60L, delay)
    }

    @Test
    fun the_lead_window_wraps_around_midnight() {
        var pattern = MavUsagePattern()
        repeat(10) { pattern = pattern.record(1) }
        // 01:00 opens mean a 23:00 precompute, which is the previous day.
        assertTrue(pattern.shouldPrecomputeAt(23))
        assertTrue(!pattern.shouldPrecomputeAt(12))
    }

    @Test
    fun a_pattern_survives_being_written_down_and_read_back() {
        var pattern = MavUsagePattern()
        repeat(7) { pattern = pattern.record(9) }
        repeat(2) { pattern = pattern.record(20) }
        assertEquals(pattern, MavUsagePattern.parse(pattern.encode()))
    }

    @Test
    fun a_corrupt_stored_pattern_is_an_empty_one_rather_than_a_crash() {
        assertEquals(MavUsagePattern(), MavUsagePattern.parse("not,a,pattern"))
        assertEquals(MavUsagePattern(), MavUsagePattern.parse(""))
        assertEquals(MavUsagePattern(), MavUsagePattern.parse(null))
        assertEquals(MavUsagePattern(), MavUsagePattern.parse("-1," + "0,".repeat(22) + "0"))
    }

    @Test
    fun an_hour_outside_the_day_is_refused_rather_than_wrapped() {
        val pattern = MavUsagePattern()
        listOf(-1, 24, 99).forEach { hour ->
            try {
                pattern.record(hour)
                throw AssertionError("$hour was accepted as an hour of the day")
            } catch (expected: IllegalArgumentException) {
                // The point: a bad hour is a bug in the caller, not a bucket to silently pick.
            }
        }
    }
}
