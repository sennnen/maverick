package com.sennnen.mav

import android.os.Build
import android.os.Debug
import android.os.PowerManager
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.sennnen.mav.ml.MavModelCatalog
import com.sennnen.mav.ml.MavModelRunner
import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What the zoo actually costs on this phone: cold, warm, and held under load.
 *
 * Kept apart from the parity tests on purpose. Those read fourteen megabytes of reference
 * vectors, run every model three times against them and write their outputs back to disk —
 * all of which is work the app will never do, and all of which heats the device before the
 * next thing measured. A timing taken in that shadow is a timing of the test that came before
 * it: `activity_context_embedding` read 10.34 ms and 11.83 ms on the CPU in sweeps taken
 * straight after a parity pass, and 2.13 ms on a cool device. So this test loads no vectors,
 * asserts no numbers about accuracy, and idles between models.
 *
 * Three separate things get measured, because they have separate causes:
 *
 *  * **Cold** — mapping the asset, hashing it, building the interpreter, attaching the
 *    delegate. Paid once per model per process, and dominated by whichever of those is slow.
 *  * **Warm** — the steady-state inference, median and p90 over repeats after a warm-up. This
 *    is what a queued batch of windows pays.
 *  * **Sustained** — the same inference repeated for a fixed wall-clock span, reported as
 *    early against late, with the thermal status at both ends. A phone that is fast for one
 *    call and throttles at ten seconds is not fast.
 *
 * Nothing here asserts a latency bound. Timings are the hardware's answer and vary with
 * thermal state; the numbers are written out for `tools/ml/device_bench.py` to compare across
 * builds, and the only assertion is that every model ran.
 */
@RunWith(AndroidJUnit4::class)
class ModelBenchmarkInstrumentedTest {
    private val targetContext get() = InstrumentationRegistry.getInstrumentation().targetContext

    /** Process CPU time, so a wall-clock win that is really a core count can be told apart. */
    private fun processCpuMillis(): Long {
        val fields = File("/proc/self/stat").readText().substringAfterLast(") ").split(" ")
        // utime and stime are fields 14 and 15 of the full line; the split above drops the
        // first two, so they land at 11 and 12. Ticks are 100/s on every Android arm64 build.
        val ticks = fields[11].toLong() + fields[12].toLong()
        return ticks * 10
    }

    private fun thermalStatus(): Int {
        val power = targetContext.getSystemService(PowerManager::class.java)
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) power.currentThermalStatus else -1
    }

    private fun residentKb(): Long {
        val info = Debug.MemoryInfo()
        Debug.getMemoryInfo(info)
        return info.totalPss.toLong()
    }

    private fun percentile(sorted: LongArray, fraction: Double): Double {
        val index = ((sorted.size - 1) * fraction).toInt()
        return sorted[index] / 1_000_000.0
    }

    @Test
    fun everyModelIsBenchmarkedColdWarmAndSustained() {
        val report = StringBuilder()
        val json = StringBuilder("{\n")
        json.append(
            """ "device": {"model": "${Build.MODEL}", "soc": "${Build.SOC_MODEL}", """ +
                """"api": ${Build.VERSION.SDK_INT}, "abi": "${Build.SUPPORTED_ABIS.first()}"},""",
        )
        json.append("\n \"models\": {\n")

        var first = true
        val baseline = residentKb()
        for (slug in MavModelCatalog.slugs) {
            val entry = requireNotNull(MavModelCatalog.entries[slug])
            val inputs = entry.inputs.associate { it.name to FloatArray(it.elementCount) }

            // Let the previous model's heat dissipate. Cheap insurance against measuring the
            // test order instead of the model.
            Thread.sleep(SETTLE_MS)
            val thermalBefore = thermalStatus()
            val pssBefore = residentKb()

            // A fresh runner per model so the cold path is genuinely cold: a shared runner
            // would have this model's interpreter already built by an earlier phase.
            val coldStarted = System.nanoTime()
            var firstInference: Long
            var warmSorted: LongArray
            var sustainedEarly: Double
            var sustainedLate: Double
            var iterations: Int
            var cpuUsed: Long
            var pssAfter: Long
            var samplesTaken = 0
            MavModelRunner(targetContext).use { runner ->
                // Touch the model without inferring: this is map + hash + interpreter + delegate.
                runner.loadedSha256(slug)
                val coldNanos = System.nanoTime() - coldStarted
                val split = runner.loadTimings(slug)

                val firstStarted = System.nanoTime()
                runner.run(slug, inputs)
                firstInference = System.nanoTime() - firstStarted

                // How many repeats this model gets is set by how long it takes, not by a
                // constant. Fifteen iterations of `pulse_ppg` is thirty-seven seconds of
                // continuous load, which measures the thermal governor rather than the model
                // and heats the phone for everything after it.
                val budgeted = (WARM_BUDGET_NANOS / maxOf(firstInference, 1L)).toInt()
                val timed = budgeted.coerceIn(MIN_SAMPLES, MAX_SAMPLES)
                repeat(WARMUP) { runner.run(slug, inputs) }
                val cpuStart = processCpuMillis()
                warmSorted = LongArray(timed) {
                    val started = System.nanoTime()
                    runner.run(slug, inputs)
                    System.nanoTime() - started
                }
                cpuUsed = processCpuMillis() - cpuStart
                warmSorted.sort()
                samplesTaken = timed

                // Sustained: run for a fixed span and compare the first tenth with the last.
                val samples = ArrayList<Long>()
                val deadline = System.nanoTime() + SUSTAIN_NANOS
                while (System.nanoTime() < deadline) {
                    val started = System.nanoTime()
                    runner.run(slug, inputs)
                    samples += System.nanoTime() - started
                }
                iterations = samples.size
                val slice = maxOf(1, samples.size / 10)
                sustainedEarly = samples.take(slice).average() / 1_000_000.0
                sustainedLate = samples.takeLast(slice).average() / 1_000_000.0
                pssAfter = residentKb()

                report.append(
                    "BENCH %s path=%s cold_ms=%.2f first_ms=%.2f warm_p50_ms=%.3f warm_p90_ms=%.3f ".format(
                        slug,
                        runner.executionPath(slug),
                        coldNanos / 1_000_000.0,
                        firstInference / 1_000_000.0,
                        percentile(warmSorted, 0.5),
                        percentile(warmSorted, 0.9),
                    ),
                )
                json.append(if (first) "" else ",\n")
                first = false
                json.append(
                    """  "$slug": {"path": "${runner.executionPath(slug)}", """ +
                        """"cold_ms": ${"%.3f".format(coldNanos / 1_000_000.0)}, """ +
                        """"cold_map_ms": ${"%.3f".format(split.mapNanos / 1_000_000.0)}, """ +
                        """"cold_hash_ms": ${"%.3f".format(split.hashNanos / 1_000_000.0)}, """ +
                        """"cold_build_ms": ${"%.3f".format(split.interpreterNanos / 1_000_000.0)}, """ +
                        """"first_ms": ${"%.3f".format(firstInference / 1_000_000.0)}, """ +
                        """"warm_p50_ms": ${"%.4f".format(percentile(warmSorted, 0.5))}, """ +
                        """"warm_p90_ms": ${"%.4f".format(percentile(warmSorted, 0.9))}, """ +
                        """"sustained_early_ms": ${"%.4f".format(sustainedEarly)}, """ +
                        """"sustained_late_ms": ${"%.4f".format(sustainedLate)}, """ +
                        """"sustained_iterations": $iterations, """ +
                        """"cpu_ms_per_inference": ${"%.3f".format(cpuUsed.toDouble() / maxOf(samplesTaken, 1))}, """ +
                        """"warm_samples": $samplesTaken, """ +
                        """"pss_delta_kb": ${pssAfter - pssBefore}, """ +
                        """"thermal_before": $thermalBefore, "thermal_after": ${thermalStatus()}}""",
                )
            }
            report.append(
                "sustain_early_ms=%.3f sustain_late_ms=%.3f iters=%d cpu_ms=%.2f pss_kb=%d thermal=%d/%d\n".format(
                    sustainedEarly,
                    sustainedLate,
                    iterations,
                    cpuUsed.toDouble() / maxOf(samplesTaken, 1),
                    pssAfter - pssBefore,
                    thermalBefore,
                    thermalStatus(),
                ),
            )
            assertTrue("$slug produced no warm samples", warmSorted.isNotEmpty())
        }
        json.append("\n }\n}\n")

        val out = File(targetContext.getExternalFilesDir(null), "benchmark.json")
        out.writeText(json.toString())
        println(report)
        println("BENCH wrote ${out.absolutePath}; baseline pss ${baseline} kB")
    }

    /**
     * What keeping every interpreter resident actually costs.
     *
     * The runner caches an interpreter per model and never evicts one, which is right if the
     * total is small and wrong if it is not — so it gets measured rather than assumed. Asking
     * per model does not work: process PSS moves with garbage collection and with whatever the
     * test harness is doing, and sampling it around one model attributed 60 MB to `step_head`,
     * which has five parameters. The honest measurement is the whole zoo at once, against the
     * same process before it loaded anything and after it let go.
     */
    @Test
    fun holdingEveryInterpreterResidentIsMeasured() {
        Runtime.getRuntime().gc()
        Thread.sleep(SETTLE_MS)
        val before = residentKb()
        val nativeBefore = Debug.getNativeHeapAllocatedSize() / 1024

        val runner = MavModelRunner(targetContext)
        val perModel = StringBuilder()
        for (slug in MavModelCatalog.slugs) {
            val was = Debug.getNativeHeapAllocatedSize()
            runner.loadedSha256(slug)
            // Native heap rather than PSS: the interpreter's arenas and XNNPACK's repacked
            // weights are native allocations, and this counter does not wander with the
            // garbage collector the way process PSS does.
            perModel.append(
                "RESIDENT %s native_kb=%d\n".format(
                    slug, (Debug.getNativeHeapAllocatedSize() - was) / 1024,
                ),
            )
        }
        println(perModel)
        val loadedPss = residentKb()
        val loadedNative = Debug.getNativeHeapAllocatedSize() / 1024

        runner.close()
        Runtime.getRuntime().gc()
        Thread.sleep(SETTLE_MS)
        val after = residentKb()
        val nativeAfter = Debug.getNativeHeapAllocatedSize() / 1024

        println(
            "RESIDENCY models=%d pss_before_kb=%d pss_loaded_kb=%d pss_after_close_kb=%d ".format(
                MavModelCatalog.slugs.size, before, loadedPss, after,
            ) +
                "native_before_kb=%d native_loaded_kb=%d native_after_close_kb=%d".format(
                    nativeBefore, loadedNative, nativeAfter,
                ),
        )
        assertTrue("loading the zoo freed memory, which cannot be right", loadedPss > before)
    }

    private companion object {
        const val WARMUP = 3
        const val MIN_SAMPLES = 5
        const val MAX_SAMPLES = 15
        const val SETTLE_MS = 600L

        /** Roughly how long the warm phase may spend on any one model. */
        const val WARM_BUDGET_NANOS = 1_500_000_000L

        /** Long enough for a small model to show throttling, short enough not to cook the phone. */
        const val SUSTAIN_NANOS = 3_000_000_000L
    }
}
