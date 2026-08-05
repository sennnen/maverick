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
import org.junit.Assert.assertNotNull
import org.junit.Test
import org.junit.runner.RunWith
import org.tensorflow.lite.gpu.CompatibilityList

/**
 * Each execution path measured under its own name, so the delegate choice can be evidence.
 *
 * `MavModelAcceleration` used to decide from a single inherited assumption: Core ML admitted
 * this model at half precision, therefore Android's half-precision delegate is equivalent. It
 * is not. Apple's Neural Engine accumulates a half-width matmul into a wider register and this
 * GPU delegate does not, so the assumption holds for a shallow graph and fails for a deep one
 * — and nothing on a Mac can tell you which.
 *
 * This runs every model on the CPU, on the GPU at half width, and on the GPU with precision
 * loss refused, and prints accuracy against the reference beside the latency of each. The
 * printed table is what `tools/ml/android_delegate.py` turns into the shipped flag.
 *
 * It asserts almost nothing on purpose. Which delegates exist is the handset's answer to give;
 * this test's job is to obtain the numbers, and a device with no GPU driver is a legitimate
 * result rather than a failure.
 */
@RunWith(AndroidJUnit4::class)
class DelegateSweepInstrumentedTest {
    private val targetContext get() = InstrumentationRegistry.getInstrumentation().targetContext
    private val testContext get() = InstrumentationRegistry.getInstrumentation().context

    private fun readProbes(slug: String, inputCount: Int, outputCount: Int):
        List<Pair<List<FloatArray>, List<FloatArray>>> {
        val bytes = testContext.assets.open("vectors/$slug.vec").use { it.readBytes() }
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        buffer.position(MAGIC.length)
        val probes = buffer.int
        fun group(count: Int) = List(count) { FloatArray(buffer.int) { buffer.float } }
        return List(probes) {
            val inputs = group(inputCount)
            val expected = group(outputCount)
            group(outputCount) // the host's answer, not needed here
            inputs to expected
        }
    }

    private fun relative(produced: FloatArray, reference: FloatArray): Double {
        var worst = 0.0
        var scale = 0.0
        for (index in reference.indices) {
            worst = maxOf(worst, abs(produced[index].toDouble() - reference[index].toDouble()))
            scale = maxOf(scale, abs(reference[index].toDouble()))
        }
        return if (scale > 1e-9) worst / scale else worst
    }

    /**
     * What this handset says about its own accelerator, printed rather than assumed.
     *
     * "The GPU delegate supports float16" was an inherited belief for a long time and the thing
     * it justified — routing thirteen models onto a half-width path — turned out to cost
     * accuracy on all of them. The vendor's own [CompatibilityList] answer and the options it
     * hands back belong in the record beside the numbers they explain.
     */
    @Test
    fun theDeviceReportsItsAcceleratorCapabilities() {
        val report = StringBuilder()
        report.append(
            "DEVICE model=${Build.MODEL} soc=${Build.SOC_MODEL} " +
                "manufacturer=${Build.SOC_MANUFACTURER} api=${Build.VERSION.SDK_INT} " +
                "abis=${Build.SUPPORTED_ABIS.joinToString("/")}\n",
        )
        val compatibility = runCatching { CompatibilityList() }.getOrNull()
        val supported = compatibility?.isDelegateSupportedOnThisDevice
        report.append("DEVICE gpu_delegate_supported=$supported\n")
        if (compatibility != null && supported == true) {
            val options = compatibility.bestOptionsForThisDevice
            report.append(
                "DEVICE gpu_precision_loss_allowed_by_default=${options.isPrecisionLossAllowed} " +
                    "quantized_allowed=${options.areQuantizedModelsAllowed()} " +
                    "inference_preference=${options.inferencePreference}\n",
            )
        }
        println(report)
        // The only assertion the hardware cannot legitimately fail: the list has to answer.
        assertNotNull("CompatibilityList could not be constructed", compatibility)
    }

    @Test
    fun everyPathIsMeasuredOnEveryModel() {
        val report = StringBuilder()
        for (path in listOf(
            MavModelAcceleration.Path.CPU,
            MavModelAcceleration.Path.GPU,
            MavModelAcceleration.Path.GPU_FULL,
        )) {
            for (slug in MavModelCatalog.slugs) {
                val entry = requireNotNull(MavModelCatalog.entries[slug])
                val probes = readProbes(slug, entry.inputs.size, entry.outputs.size)
                // A fresh runner per model and per path: an interpreter caches its delegate for
                // the life of the runner, so reusing one would measure the first path three
                // times under three names.
                MavModelRunner(targetContext, path).use { runner ->
                    val actual = runCatching { runner.executionPath(slug) }.getOrNull()
                    if (actual == null) {
                        report.append("SWEEP $slug path=$path attached=none\n")
                        return@use
                    }
                    var worst = 0.0
                    for ((inputs, expected) in probes) {
                        val feed = entry.inputs.mapIndexed { index, spec ->
                            spec.name to inputs[index]
                        }.toMap()
                        val produced = runner.run(slug, feed)
                        entry.outputs.forEachIndexed { index, spec ->
                            worst = maxOf(
                                worst,
                                relative(requireNotNull(produced[spec.name]), expected[index]),
                            )
                        }
                    }
                    val warm = entry.inputs.associate { it.name to FloatArray(it.elementCount) }
                    repeat(WARMUP) { runner.run(slug, warm) }
                    val samples = LongArray(TIMED) {
                        val started = System.nanoTime()
                        runner.run(slug, warm)
                        System.nanoTime() - started
                    }
                    samples.sort()
                    report.append(
                        "SWEEP %s path=%s attached=%s rel=%.6e median_ms=%.3f\n".format(
                            slug, path, actual, worst, samples[TIMED / 2] / 1_000_000.0,
                        ),
                    )
                }
            }
        }
        println(report)
    }

    private companion object {
        const val MAGIC = "MAVVEC01"
        const val WARMUP = 2
        const val TIMED = 7
    }
}
