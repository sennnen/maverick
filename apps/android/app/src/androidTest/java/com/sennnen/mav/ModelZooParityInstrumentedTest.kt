package com.sennnen.mav

import android.os.Build
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.sennnen.mav.ml.MavModelAcceleration
import com.sennnen.mav.ml.MavModelCatalog
import com.sennnen.mav.ml.MavModelRunner
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.abs
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import org.junit.runner.RunWith

/**
 * What the phone actually computes, against the reference the zoo is defined by.
 *
 * Every other parity number in this repository was measured on a Mac. The conversion pipeline
 * runs LiteRT's own interpreter on the shipped flatbuffer and reports how far it lands from
 * eager PyTorch, which is a real statement about the file and no statement whatsoever about
 * the handset. Android picks its delegate on the device, from the driver the device happens to
 * ship, and the delegate decides the arithmetic width; none of that exists on the host. This
 * is the only test in the tree that can fail because of the phone.
 *
 * `tools/ml/device_vectors.py` writes the vectors, carrying two references per probe:
 *
 *  * `expected` — eager PyTorch at full width, the ground truth. Device against it is total
 *    Android error and is directly comparable to the manifest's own number.
 *  * `host` — this flatbuffer through LiteRT on a Mac CPU. Device against it is what the
 *    handset added: the delegate, its driver, and half-width arithmetic.
 *
 * The pair is what makes a failure attributable. A model that drifts from `expected` but sits
 * on top of `host` was converted wrong, and the phone is faithfully reproducing the mistake; a
 * model that matches `expected` on the host and not here is the driver.
 */
@RunWith(AndroidJUnit4::class)
class ModelZooParityInstrumentedTest {
    private val targetContext get() = InstrumentationRegistry.getInstrumentation().targetContext
    private val testContext get() = InstrumentationRegistry.getInstrumentation().context

    private class Probe(
        val inputs: List<FloatArray>,
        val expected: List<FloatArray>,
        val host: List<FloatArray>,
    )

    private fun readVectors(slug: String, inputCount: Int, outputCount: Int): List<Probe> {
        val bytes = testContext.assets.open("vectors/$slug.vec").use { it.readBytes() }
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        val magic = ByteArray(MAGIC.length)
        buffer.get(magic)
        check(String(magic) == MAGIC) { "$slug vectors have magic ${String(magic)}" }
        val probeCount = buffer.int
        fun group(count: Int) = List(count) {
            FloatArray(buffer.int) { buffer.float }
        }
        return List(probeCount) {
            Probe(group(inputCount), group(outputCount), group(outputCount))
        }
    }

    /** Largest absolute difference, and that difference relative to the reference's own scale. */
    private fun deviation(produced: FloatArray, reference: FloatArray): Pair<Double, Double> {
        var worst = 0.0
        var scale = 0.0
        for (index in reference.indices) {
            worst = maxOf(worst, abs(produced[index].toDouble() - reference[index].toDouble()))
            scale = maxOf(scale, abs(reference[index].toDouble()))
        }
        return worst to if (scale > 1e-9) worst / scale else worst
    }

    /**
     * Keep what the device computed, so the other platform can be measured against it.
     *
     * Core ML and LiteRT are never compared on the same numbers otherwise: the manifest's
     * cross-platform figure runs both on this Mac, and the half of it that matters — what the
     * phone's delegate produced — has no counterpart there. Writing the tensors out lets
     * `tools/ml/device_compare.py` put the real handset output beside the real Core ML output
     * on identical inputs, which is the only version of that number neither side can fake.
     */
    private fun writeOutputs(slug: String, probes: List<List<FloatArray>>) {
        val directory = java.io.File(targetContext.getExternalFilesDir(null), "device_outputs")
        directory.mkdirs()
        java.io.DataOutputStream(
            java.io.BufferedOutputStream(java.io.File(directory, "$slug.bin").outputStream()),
        ).use { sink ->
            sink.write(MAGIC.toByteArray())
            sink.writeInt(Integer.reverseBytes(probes.size))
            for (outputs in probes) {
                for (values in outputs) {
                    sink.writeInt(Integer.reverseBytes(values.size))
                    for (value in values) {
                        sink.writeInt(Integer.reverseBytes(java.lang.Float.floatToIntBits(value)))
                    }
                }
            }
        }
    }

    @Test
    fun everyModelMatchesItsReferenceOnDevice() {
        val failures = mutableListOf<String>()
        val report = StringBuilder()
        report.append(
            "device=${Build.MODEL} soc=${Build.SOC_MODEL} api=${Build.VERSION.SDK_INT} " +
                "abi=${Build.SUPPORTED_ABIS.firstOrNull()}\n",
        )
        MavModelRunner(targetContext).use { runner ->
            for (slug in MavModelCatalog.slugs) {
                val entry = requireNotNull(MavModelCatalog.entries[slug])
                val probes = readVectors(slug, entry.inputs.size, entry.outputs.size)
                var worstExpectedRel = 0.0
                var worstExpectedAbs = 0.0
                var worstHostRel = 0.0
                var worstHostAbs = 0.0
                val kept = mutableListOf<List<FloatArray>>()
                for (probe in probes) {
                    val inputs = entry.inputs.mapIndexed { index, spec ->
                        spec.name to probe.inputs[index]
                    }.toMap()
                    val produced = runner.run(slug, inputs)
                    kept += entry.outputs.map { requireNotNull(produced[it.name]) }
                    entry.outputs.forEachIndexed { index, spec ->
                        val values = requireNotNull(produced[spec.name])
                        val (absExpected, relExpected) = deviation(values, probe.expected[index])
                        val (absHost, relHost) = deviation(values, probe.host[index])
                        worstExpectedAbs = maxOf(worstExpectedAbs, absExpected)
                        worstExpectedRel = maxOf(worstExpectedRel, relExpected)
                        worstHostAbs = maxOf(worstHostAbs, absHost)
                        worstHostRel = maxOf(worstHostRel, relHost)
                    }
                }
                writeOutputs(slug, kept)
                val path = runner.executionPath(slug)
                report.append(
                    "PARITY %s path=%s expected_rel=%.6e expected_abs=%.6e host_rel=%.6e host_abs=%.6e\n"
                        .format(slug, path, worstExpectedRel, worstExpectedAbs, worstHostRel, worstHostAbs),
                )
                val bar = DEVICE_BARS[slug] ?: DEFAULT_BAR
                // Relative error is meaningless where the reference is all but zero, so a model
                // clears on either measure: a tiny absolute difference is a tiny difference
                // whatever it is a fraction of.
                if (worstExpectedRel > bar && worstExpectedAbs > ABSOLUTE_FLOOR) {
                    failures += "$slug: %.3e relative to PyTorch, bar %.3e".format(worstExpectedRel, bar)
                }
                if (worstHostRel > DELEGATE_BAR && worstHostAbs > DELEGATE_ABSOLUTE_FLOOR) {
                    failures += "$slug: %.3e from the host's own answer for this file".format(worstHostRel)
                }
            }
        }
        println(report)
        if (failures.isNotEmpty()) {
            fail("on-device parity failed for ${failures.size} models:\n" + failures.joinToString("\n"))
        }
    }

    /**
     * What the device actually did, printed rather than asserted where the answer is the
     * hardware's to give.
     *
     * The delegate is a property of this handset's driver, so a fallback is a legitimate result
     * and not a defect; the assertion is that the runner reports a path at all and that a model
     * iOS keeps at full width is never quietly accelerated here. The timing is a median over
     * repeated runs after a warm-up, because the first call through a delegate builds its
     * kernels and measuring that would time the compiler.
     */
    @Test
    fun everyModelReportsItsDelegateAndLatency() {
        val report = StringBuilder()
        MavModelRunner(targetContext).use { runner ->
            for (slug in MavModelCatalog.slugs) {
                val entry = requireNotNull(MavModelCatalog.entries[slug])
                val inputs = entry.inputs.associate { it.name to FloatArray(it.elementCount) }
                repeat(WARMUP) { runner.run(slug, inputs) }
                val samples = LongArray(TIMED) {
                    val started = System.nanoTime()
                    runner.run(slug, inputs)
                    System.nanoTime() - started
                }
                samples.sort()
                val median = samples[TIMED / 2] / 1_000_000.0
                val path = runner.executionPath(slug)
                assertTrue(
                    "$slug was measured onto ${entry.preferredPath} and ran on $path",
                    path == entry.preferredPath || path == MavModelAcceleration.Path.CPU,
                )
                report.append(
                    "LATENCY %s path=%s preferred=%s median_ms=%.3f\n"
                        .format(slug, path, entry.preferredPath, median),
                )
            }
        }
        println(report)
    }

    private companion object {
        const val MAGIC = "MAVVEC01"
        const val WARMUP = 3
        const val TIMED = 9

        /**
         * How far the phone may sit from eager PyTorch.
         *
         * Matches the cross-platform bar the manifest is built against, because the same
         * flatbuffer measured on the same inputs should not become a different artefact for
         * having been copied onto a phone.
         */
        const val DEFAULT_BAR = 5e-3

        /**
         * How far the phone may sit from the *host's* answer for the very same file.
         *
         * Much tighter than the bar against PyTorch, because by this point the conversion
         * error has already been spent: what is left is the delegate, and a delegate that
         * changes the answer at all is not computing the graph that was admitted. Measured
         * across all forty-one on a Tensor G2 the worst is 5.4e-6, so this leaves an order of
         * magnitude for a different driver and still fails long before the 1.2e-2 that the
         * half-width GPU path used to introduce.
         */
        const val DELEGATE_BAR = 1e-4

        /**
         * Below this, a relative figure is noise about a number that is itself nearly zero.
         *
         * Mirrors `coreml_precision.ABSOLUTE_FLOOR`, which the conversion ladder has always
         * judged by, rather than a tighter number invented here. `illness_detection` is the
         * case it exists for: it emits one probability, and on a probe where that probability
         * is 0.063 an absolute deviation of 3.4e-4 reads as 5.4e-3 relative. The head reading
         * it cannot tell those two numbers apart, and on a probe where the probability is
         * near zero the same deviation would read as 1.0.
         */
        const val ABSOLUTE_FLOOR = 1e-3

        /**
         * The same idea for the delegate comparison, two orders tighter.
         *
         * There the two sides are running the *same file*, so the conversion's own error is
         * not in the number and there is far less of it to excuse. Measured worst across the
         * zoo is 9.2e-5 absolute, on `activity_detection`, whose outputs run to hundreds.
         */
        const val DELEGATE_ABSOLUTE_FLOOR = 1e-5

        /**
         * Models whose bar is not the default, each with the measured reason it is not.
         *
         * Empty, and that is the claim: with the delegate chosen by measurement rather than
         * inherited from iOS, all forty-one clear the same bar and none needs an exception
         * written for it. An entry here would be a model the zoo tolerates and should say so
         * out loud.
         */
        val DEVICE_BARS: Map<String, Double> = mapOf()
    }
}
