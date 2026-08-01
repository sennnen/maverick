package com.sennnen.mav.ui.mav

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The live session's clock.
 *
 * Both platforms formatted elapsed time as `mm:ss` with no hour rollover, so a ninety-minute
 * session read "90:00" and a two-hour one read "120:00". The iOS twin is `MavElapsedTests.swift`
 * and asserts the same boundaries, because a clock that is right on one phone and wrong on the
 * other is worse than one that is wrong on both.
 */
class MavElapsedTest {

    @Test
    fun `under an hour stays mm ss`() {
        assertEquals("00:00", formatElapsed(0))
        assertEquals("00:09", formatElapsed(9))
        assertEquals("01:00", formatElapsed(60))
        assertEquals("59:59", formatElapsed(3_599))
    }

    @Test
    fun `the hour boundary rolls over instead of counting past sixty minutes`() {
        assertEquals("1:00:00", formatElapsed(3_600))
        assertEquals("1:30:00", formatElapsed(5_400))
        assertEquals("2:00:01", formatElapsed(7_201))
        // The regression itself: this used to render "90:00".
        assertEquals("1:30:00", formatElapsed(90 * 60))
    }

    @Test
    fun `minutes and seconds stay zero padded inside an hour reading`() {
        assertEquals("1:05:07", formatElapsed(3_600 + 5 * 60 + 7))
    }

    @Test
    fun `the spoken form is a duration and singularises correctly`() {
        assertEquals("0 seconds", spokenElapsed(0))
        assertEquals("1 second", spokenElapsed(1))
        assertEquals("2 seconds", spokenElapsed(2))
        assertEquals("1 minute 0 seconds", spokenElapsed(60))
        assertEquals("2 minutes 3 seconds", spokenElapsed(123))
        assertEquals("1 hour 0 seconds", spokenElapsed(3_600))
        assertEquals("1 hour 30 minutes 0 seconds", spokenElapsed(5_400))
        assertEquals("2 hours 1 minute 5 seconds", spokenElapsed(7_265))
    }
}
