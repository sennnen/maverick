package com.sennnen.mav

import com.sennnen.mav.ml.MavModelBridge
import com.sennnen.mav.ml.MavModelCatalog
import com.sennnen.mav.ml.MavModelRunner
import java.io.File
import java.nio.ByteBuffer
import java.security.MessageDigest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.mav_ffi.ModelInferenceRequest
import uniffi.mav_ffi.ModelInferenceResult
import uniffi.mav_ffi.ModelTensor

/**
 * The JVM half of the model-zoo proof: the bundled assets are the admitted assets, the generated
 * catalogue agrees with the manifest, and the drain loop behaves when a model fails.
 *
 * Running a model needs a device, so that lives in the instrumented test. What can be checked
 * here is everything that does not need an interpreter, which is most of what can go wrong.
 */
class ModelZooContractTest {
    private fun repositoryRoot(): File {
        var candidate = File(System.getProperty("user.dir") ?: ".").absoluteFile
        while (!File(candidate, "artifacts/models/manifest.json").isFile) {
            candidate = candidate.parentFile ?: error("could not find the repository root")
        }
        return candidate
    }

    private fun manifestModels(): List<Map<String, Any?>> {
        val text = File(repositoryRoot(), "artifacts/models/manifest.json").readText()
        @Suppress("UNCHECKED_CAST")
        return parseJson(text)["models"] as List<Map<String, Any?>>
    }

    @Test
    fun everyAdmittedAssetIsBundledAndHashesToItsAdmittedValue() {
        val assets = File(repositoryRoot(), "apps/android/app/src/main/assets/models")
        assertTrue("no bundled models found", MavModelCatalog.entries.isNotEmpty())
        for ((slug, entry) in MavModelCatalog.entries) {
            val file = File(assets, "$slug.tflite")
            assertTrue("$slug is not bundled", file.isFile)
            val digest = MessageDigest.getInstance("SHA-256")
                .digest(file.readBytes())
                .joinToString("") { "%02x".format(it) }
            assertEquals("$slug hashes differently to the catalogue", entry.admittedSha256, digest)
        }
    }

    @Test
    fun theGeneratedCatalogueMatchesTheManifest() {
        val models = manifestModels()
        assertEquals(models.size, MavModelCatalog.entries.size)
        for (model in models) {
            val slug = model["model"] as String
            val entry = requireNotNull(MavModelCatalog.entries[slug]) { "$slug missing" }
            @Suppress("UNCHECKED_CAST")
            val tflite = model["tflite"] as Map<String, Any?>
            assertEquals(tflite["sha256"], entry.admittedSha256)
            @Suppress("UNCHECKED_CAST")
            val inputs = model["inputs"] as List<Map<String, Any?>>
            assertEquals(inputs.size, entry.inputs.size)
            inputs.forEachIndexed { index, spec ->
                assertEquals(spec["name"], entry.inputs[index].name)
                @Suppress("UNCHECKED_CAST")
                val shape = (spec["shape"] as List<Number>).map(Number::toInt)
                assertEquals(shape, entry.inputs[index].shape.toList())
            }
        }
    }

    @Test
    fun tensorNameMatchingSurvivesTheConvertersDecoration() {
        assertTrue(MavModelRunner.matches("ppg", "ppg"))
        assertTrue(MavModelRunner.matches("serving_default_ppg:0", "ppg"))
        assertTrue(MavModelRunner.matches("StatefulPartitionedCall:0/embeddings", "embeddings"))
        assertFalse(MavModelRunner.matches("serving_default_scalars:0", "ppg"))
    }

    @Test
    fun assetHashingReadsTheWholeBufferWithoutConsumingIt() {
        val buffer = ByteBuffer.wrap("admitted-bytes".toByteArray())
        val first = MavModelRunner.sha256(buffer)
        val second = MavModelRunner.sha256(buffer)
        assertEquals(first, second)
        assertEquals(
            MessageDigest.getInstance("SHA-256")
                .digest("admitted-bytes".toByteArray())
                .joinToString("") { "%02x".format(it) },
            first,
        )
    }

    @Test
    fun theDrainLoopCompletesWhatItCanAndCancelsWhatItCannot() {
        val queue = ArrayDeque(
            listOf(
                ModelInferenceRequest(1uL, "good_model", listOf(ModelTensor("ppg", listOf(0f, 1f)))),
                ModelInferenceRequest(2uL, "bad_model", listOf(ModelTensor("ppg", listOf(0f, 1f)))),
            ),
        )
        val cancelled = mutableListOf<ULong>()
        val submitted = mutableListOf<ULong>()
        val host = object : MavModelBridge.Host {
            override fun nextModelInference(): ModelInferenceRequest? = queue.removeFirstOrNull()

            override fun submitModelInference(
                requestId: ULong,
                outputs: List<ModelTensor>,
                modelSha256: String,
                completedAtMs: Long,
            ): ModelInferenceResult {
                submitted += requestId
                return ModelInferenceResult(requestId, "good_model", outputs, modelSha256)
            }

            override fun cancelModelInference(requestId: ULong): Boolean {
                cancelled += requestId
                return true
            }
        }
        val runner = object : MavModelBridge.Runner {
            override fun run(slug: String, inputs: Map<String, FloatArray>): Map<String, FloatArray> {
                if (slug == "bad_model") error("not bundled")
                return mapOf("embeddings" to floatArrayOf(0.5f))
            }

            override fun loadedSha256(slug: String) = "a".repeat(64)

            override fun releaseCache() = Unit
        }

        val outcome = MavModelBridge(host, runner).drain(limit = 8)
        assertEquals(1, outcome.completed)
        assertEquals(1, outcome.failed)
        assertEquals(listOf(1uL), submitted)
        assertEquals(listOf(2uL), cancelled)
    }

    @Test
    fun theDrainLoopStopsAtItsLimit() {
        val queue = ArrayDeque(
            (1..10).map {
                ModelInferenceRequest(it.toULong(), "good_model", listOf(ModelTensor("ppg", listOf(0f))))
            },
        )
        val host = object : MavModelBridge.Host {
            override fun nextModelInference(): ModelInferenceRequest? = queue.removeFirstOrNull()

            override fun submitModelInference(
                requestId: ULong,
                outputs: List<ModelTensor>,
                modelSha256: String,
                completedAtMs: Long,
            ) = ModelInferenceResult(requestId, "good_model", outputs, modelSha256)

            override fun cancelModelInference(requestId: ULong) = true
        }
        val runner = object : MavModelBridge.Runner {
            override fun run(slug: String, inputs: Map<String, FloatArray>) =
                mapOf("embeddings" to floatArrayOf(1f))

            override fun loadedSha256(slug: String) = "b".repeat(64)

            override fun releaseCache() = Unit
        }

        val outcome = MavModelBridge(host, runner).drain(limit = 3)
        assertEquals(3, outcome.completed)
        assertEquals(7, queue.size)
    }

    /**
     * A five-line JSON reader, so the test can read the manifest without adding a parser
     * dependency to the app's unit-test classpath.
     */
    private fun parseJson(text: String): Map<String, Any?> {
        val reader = JsonReader(text)
        val value = reader.readValue()
        @Suppress("UNCHECKED_CAST")
        return value as Map<String, Any?>
    }

    private class JsonReader(private val text: String) {
        private var index = 0

        fun readValue(): Any? {
            skipWhitespace()
            return when (text[index]) {
                '{' -> readObject()
                '[' -> readArray()
                '"' -> readString()
                't' -> readLiteral("true", true)
                'f' -> readLiteral("false", false)
                'n' -> readLiteral("null", null)
                else -> readNumber()
            }
        }

        private fun readObject(): Map<String, Any?> {
            val out = LinkedHashMap<String, Any?>()
            index += 1
            skipWhitespace()
            if (text[index] == '}') { index += 1; return out }
            while (true) {
                skipWhitespace()
                val key = readString()
                skipWhitespace()
                index += 1 // ':'
                out[key] = readValue()
                skipWhitespace()
                if (text[index] == ',') { index += 1 } else { index += 1; return out }
            }
        }

        private fun readArray(): List<Any?> {
            val out = mutableListOf<Any?>()
            index += 1
            skipWhitespace()
            if (text[index] == ']') { index += 1; return out }
            while (true) {
                out += readValue()
                skipWhitespace()
                if (text[index] == ',') { index += 1 } else { index += 1; return out }
            }
        }

        private fun readString(): String {
            index += 1
            val builder = StringBuilder()
            while (text[index] != '"') {
                if (text[index] == '\\') {
                    index += 1
                    builder.append(
                        when (text[index]) {
                            'n' -> '\n'
                            't' -> '\t'
                            'u' -> text.substring(index + 1, index + 5).toInt(16).toChar()
                                .also { index += 4 }
                            else -> text[index]
                        },
                    )
                } else {
                    builder.append(text[index])
                }
                index += 1
            }
            index += 1
            return builder.toString()
        }

        private fun <T> readLiteral(literal: String, value: T): T {
            index += literal.length
            return value
        }

        private fun readNumber(): Number {
            val start = index
            while (index < text.length && text[index] !in ",}] \n\t\r") index += 1
            val slice = text.substring(start, index)
            return if (slice.contains('.') || slice.contains('e') || slice.contains('E')) {
                slice.toDouble()
            } else {
                slice.toLong()
            }
        }

        private fun skipWhitespace() {
            while (index < text.length && text[index].isWhitespace()) index += 1
        }
    }
}
