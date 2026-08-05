package com.sennnen.mav

import com.sennnen.mav.ml.MavModelEviction
import com.sennnen.mav.ml.MavModelEviction.Candidate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The eviction policy, tested where a device is not needed to know the answer.
 *
 * Every case here is one the runner cannot survive getting wrong and no benchmark would
 * report: closing an interpreter mid-inference is a native crash, and evicting the model just
 * asked for turns the cache into a treadmill that still looks fast per call.
 *
 * The sizes are the ones measured on a Pixel 7, so the last test is the real zoo rather than a
 * shape of it.
 */
class ModelEvictionTest {
    private val budget = 192L * 1024L * 1024L

    private fun mb(count: Long) = count * 1024L * 1024L

    @Test
    fun nothingIsEvictedWhileTheResidentSetFits() {
        val evicting = MavModelEviction.choose(
            listOf(Candidate("a", mb(10), false), Candidate("b", mb(20), false)),
            budget,
            keeping = "b",
        )
        assertTrue("evicted $evicting while under budget", evicting.isEmpty())
    }

    @Test
    fun theLeastRecentlyUsedGoesFirst() {
        val evicting = MavModelEviction.choose(
            listOf(
                Candidate("oldest", mb(100), false),
                Candidate("middle", mb(100), false),
                Candidate("newest", mb(100), false),
            ),
            budget,
            keeping = "newest",
        )
        // 300 MB against a 192 MB budget: dropping the oldest alone leaves 200 MB, still over,
        // so the middle goes too and the newest is kept.
        assertEquals(listOf("oldest", "middle"), evicting)
    }

    @Test
    fun aModelMidInferenceIsNeverEvicted() {
        val evicting = MavModelEviction.choose(
            listOf(
                Candidate("running", mb(300), inUse = true),
                Candidate("idle", mb(50), false),
            ),
            budget,
            keeping = "idle",
        )
        assertEquals(
            "a model being executed was chosen for closing",
            emptyList<String>(),
            evicting.filter { it == "running" },
        )
    }

    @Test
    fun theModelJustAskedForIsNeverEvicted() {
        val evicting = MavModelEviction.choose(
            listOf(Candidate("wanted", mb(400), false), Candidate("other", mb(50), false)),
            budget,
            keeping = "wanted",
        )
        assertEquals(listOf("other"), evicting)
    }

    @Test
    fun aModelLargerThanTheWholeBudgetStillRunsAlone() {
        // pulse_ppg is 436 MB against a 192 MB budget. It must not evict itself, and the
        // policy must not spin trying to reach a total it cannot reach.
        val evicting = MavModelEviction.choose(
            listOf(Candidate("pulse_ppg", mb(436), false)),
            budget,
            keeping = "pulse_ppg",
        )
        assertTrue("the only resident model evicted itself", evicting.isEmpty())
    }

    @Test
    fun everythingIdleGoesWhenOneGiantIsHeld() {
        val evicting = MavModelEviction.choose(
            listOf(
                Candidate("small_a", mb(20), false),
                Candidate("small_b", mb(20), false),
                Candidate("pulse_ppg", mb(436), false),
            ),
            budget,
            keeping = "pulse_ppg",
        )
        assertEquals(listOf("small_a", "small_b"), evicting)
    }

    @Test
    fun theMeasuredZooSettlesWithinBudget() {
        // Native heap per model as measured on a Pixel 7, largest first after the head.
        val measured = listOf(
            "whr_unet_head" to 165L,
            "sleepnet_moonstone" to 98L,
            "sleepnet_bdi" to 69L,
            "sleepnet_bdi_v3" to 59L,
            "awhr_imputation" to 46L,
            "cva_encoder" to 26L,
            "pulse_ppg" to 436L,
        )
        val candidates = measured.map { (slug, size) -> Candidate(slug, mb(size), false) }
        val evicting = MavModelEviction.choose(candidates, budget, keeping = "pulse_ppg")
        val remaining = candidates.filterNot { it.slug in evicting }.sumOf { it.nativeBytes }
        // Only the kept giant may exceed the budget; nothing idle is left hoarding beside it.
        assertEquals(mb(436), remaining)
    }
}
