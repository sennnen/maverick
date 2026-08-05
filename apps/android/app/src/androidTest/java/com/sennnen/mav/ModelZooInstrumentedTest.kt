package com.sennnen.mav

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.sennnen.mav.ml.MavModelAcceleration
import com.sennnen.mav.ml.MavModelCatalog
import com.sennnen.mav.ml.MavModelException
import com.sennnen.mav.ml.MavModelRunner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The device half of the model-zoo proof: every bundled model opens, hashes to its admitted
 * value, and runs at its contracted shape through the real TensorFlow Lite interpreter.
 *
 * The unit test can check the assets and the catalogue; only a device can check that the graphs
 * the converter produced actually execute, which is the failure the conversion pipeline cannot
 * rule out on its own.
 */
@RunWith(AndroidJUnit4::class)
class ModelZooInstrumentedTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Test
    fun everyBundledModelRunsAtItsContractedShape() {
        MavModelRunner(context).use { runner ->
            assertTrue("the catalogue is empty", MavModelCatalog.slugs.isNotEmpty())
            for (slug in MavModelCatalog.slugs) {
                val entry = requireNotNull(MavModelCatalog.entries[slug])
                // Zero is a legitimate tensor for every contract here, so the assertion is about
                // shape and finiteness rather than about any particular signal.
                val inputs = entry.inputs.associate { it.name to FloatArray(it.elementCount) }
                val outputs = runner.run(slug, inputs)
                assertEquals("$slug returned the wrong tensor count", entry.outputs.size, outputs.size)
                for (spec in entry.outputs) {
                    val values = requireNotNull(outputs[spec.name]) { "$slug omitted ${spec.name}" }
                    assertEquals("$slug ${spec.name} length", spec.elementCount, values.size)
                    assertTrue("$slug returned a non-finite ${spec.name}", values.all(Float::isFinite))
                }
                assertEquals(entry.admittedSha256, runner.loadedSha256(slug))
            }
        }
    }

    /**
     * Every model that Core ML admitted at half-precision arithmetic must reach a
     * half-precision path here too, or the two platforms are computing different arithmetic
     * from the same weights and the manifest's cross-platform numbers describe nothing.
     *
     * This can only fail on hardware, which is why it lives here: the delegate is chosen from
     * the device's own driver support, so a device with no usable GPU legitimately falls back.
     * The assertion is therefore about *reporting* — the runner must say which path it took —
     * and the accompanying log line is what makes a fleet-wide fallback visible rather than
     * silent.
     */
    @Test
    fun everyAcceleratedModelReportsThePathItTook() {
        MavModelRunner(context).use { runner ->
            val fellBack = mutableListOf<String>()
            for (slug in MavModelCatalog.slugs) {
                val entry = requireNotNull(MavModelCatalog.entries[slug])
                val path = runner.executionPath(slug)
                if (entry.preferredPath != MavModelAcceleration.Path.CPU &&
                    path == MavModelAcceleration.Path.CPU
                ) {
                    fellBack += slug
                }
                if (entry.preferredPath == MavModelAcceleration.Path.CPU) {
                    assertEquals(
                        "$slug was measured onto the CPU and must not be accelerated here",
                        MavModelAcceleration.Path.CPU,
                        path,
                    )
                }
            }
            // Not an assertion: a device without a supported GPU driver is a valid device. The
            // list is printed so a run on real hardware says which models lost the accelerator.
            println("models that fell back to the CPU: $fellBack")
        }
    }

    @Test
    fun aShortInputTensorIsRefusedBeforeTheInterpreterRuns() {
        MavModelRunner(context).use { runner ->
            val slug = MavModelCatalog.slugs.first()
            val entry = requireNotNull(MavModelCatalog.entries[slug])
            val inputs = entry.inputs.associate { it.name to FloatArray(it.elementCount - 1) }
            val failure = runCatching { runner.run(slug, inputs) }.exceptionOrNull()
            assertTrue("expected a contract failure, got $failure", failure is MavModelException)
            assertTrue(
                "the message should name the tensor",
                failure?.message?.contains(entry.inputs.first().name) == true,
            )
        }
    }

    @Test
    fun anUnknownSlugIsRefused() {
        MavModelRunner(context).use { runner ->
            val failure = runCatching { runner.run("not_a_model", emptyMap()) }.exceptionOrNull()
            assertTrue(failure is MavModelException)
        }
    }

    @Test
    fun thePpgEncodersProduceDifferentEmbeddingsForDifferentSignals() {
        MavModelRunner(context).use { runner ->
            val entry = requireNotNull(MavModelCatalog.entries["pulse_ppg"])
            val length = entry.inputs.first().elementCount
            val flat = FloatArray(length)
            val pulsatile = FloatArray(length) { index ->
                val seconds = index / 50.0
                (Math.sin(2 * Math.PI * (68.0 / 60.0) * seconds)).toFloat()
            }
            val first = runner.run("pulse_ppg", mapOf("ppg" to flat)).getValue("embeddings")
            val second = runner.run("pulse_ppg", mapOf("ppg" to pulsatile)).getValue("embeddings")
            val difference = first.indices.maxOf { Math.abs(first[it] - second[it]) }
            assertTrue(
                "a flat signal and a pulse produced the same embedding",
                difference > 1e-4f,
            )
        }
    }
}
