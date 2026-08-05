package com.sennnen.mav

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.sennnen.mav.ml.MavModelCatalog
import com.sennnen.mav.ml.MavModelRunner
import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * How many CPU threads XNNPACK should actually get, measured rather than picked.
 *
 * `THREAD_COUNT` was 2 with nothing behind it. The benchmark then showed process CPU time
 * almost exactly equal to wall time on every model — `pulse_ppg` spent 2,538 ms of CPU on a
 * 2,566 ms inference — which is the signature of one core doing the work whatever the option
 * says. Either the graphs do not parallelise, or the setting is not reaching the interpreter,
 * and the difference matters: this is a 2+2+4 Tensor G2 with eight cores idle.
 *
 * Only the models heavy enough for threading to matter are swept. A model that finishes in
 * 20 µs cannot be helped by another core and would only add heat to the measurement.
 *
 * More threads is not automatically better on a phone. The four A55s are much slower than the
 * two X1s, so a graph split across all eight can finish later than one split across two, and
 * every extra thread costs power the CPU governor pays for later. The chooser therefore wants
 * a real margin before it moves, exactly as the delegate sweep does.
 */
@RunWith(AndroidJUnit4::class)
class ThreadSweepInstrumentedTest {
    private val targetContext get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Test
    fun theHeavyModelsAreSweptAcrossThreadCounts() {
        val report = StringBuilder()
        val json = StringBuilder("{\n")
        var firstModel = true
        for (slug in HEAVY) {
            val entry = requireNotNull(MavModelCatalog.entries[slug]) { "$slug not in catalogue" }
            val inputs = entry.inputs.associate { it.name to FloatArray(it.elementCount) }
            json.append(if (firstModel) "" else ",\n")
            firstModel = false
            json.append("""  "$slug": {""")
            var firstCount = true
            for (threads in COUNTS) {
                Thread.sleep(SETTLE_MS)
                MavModelRunner(targetContext, null, threads).use { runner ->
                    repeat(WARMUP) { runner.run(slug, inputs) }
                    val samples = LongArray(SAMPLES) {
                        val started = System.nanoTime()
                        runner.run(slug, inputs)
                        System.nanoTime() - started
                    }
                    samples.sort()
                    val median = samples[SAMPLES / 2] / 1_000_000.0
                    report.append("THREADS %s n=%d median_ms=%.3f\n".format(slug, threads, median))
                    json.append(if (firstCount) "" else ", ")
                    firstCount = false
                    json.append(""""$threads": ${"%.4f".format(median)}""")
                    assertTrue("$slug produced no timing at $threads threads", median > 0.0)
                }
            }
            json.append("}")
        }
        json.append("\n}\n")
        File(targetContext.getExternalFilesDir(null), "threads.json").writeText(json.toString())
        println(report)
    }

    private companion object {
        val COUNTS = intArrayOf(1, 2, 4, 8)
        const val WARMUP = 2
        const val SAMPLES = 5
        const val SETTLE_MS = 500L

        /**
         * The models whose warm median cleared a millisecond in the baseline benchmark. Below
         * that, dispatch dominates and a second core has nothing to do.
         */
        val HEAVY = listOf(
            "pulse_ppg",
            "sleepnet_moonstone",
            "sleepnet_bdi",
            "sleepnet_bdi_v3",
            "cva_encoder",
            "whr_unet_head",
            "activity_detection",
            "pulsenet_foundation",
            "awhr_imputation",
            "activity_history_transformer",
            "activity_primary_segments",
            "activity_context_embedding",
            "activity_ensemble",
            "activity_secondary_segments",
            "popsicle_ovulation_detection",
            "whr_unet_encoder",
        )
    }
}
