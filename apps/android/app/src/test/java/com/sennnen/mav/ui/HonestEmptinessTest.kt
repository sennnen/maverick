package com.sennnen.mav.ui

import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * PL-P7 leakage guard: a live session snapshot (HR, PRV) must never surface as Strain or Sleep
 * values. Those hubs read only these day/workout/sleep facades, which stay empty until the core
 * serves the matching read models.
 */
class HonestEmptinessTest {
    @Test
    fun strainAndSleepInputsStayEmptyUntilTheCoreServesThem() = runTest {
        val repo = MavRepo()
        assertTrue(repo.days("my-whoop").isEmpty())
        assertTrue(repo.workouts("my-whoop", 0, Long.MAX_VALUE).isEmpty())
        assertTrue(repo.sleepSessionsUnion("my-whoop", 0, Long.MAX_VALUE).isEmpty())
        assertTrue(repo.computedSleepSessionsUnion("my-whoop", 0, Long.MAX_VALUE).isEmpty())
        assertTrue(repo.metricSeries("my-whoop", "strain", "1970-01-01", "2100-01-01").isEmpty())
    }
}
