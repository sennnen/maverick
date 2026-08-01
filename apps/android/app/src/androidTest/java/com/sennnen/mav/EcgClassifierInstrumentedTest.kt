package com.sennnen.mav

import android.graphics.pdf.PdfRenderer
import android.os.ParcelFileDescriptor
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.sennnen.mav.ecg.MavEcgClassifier
import com.sennnen.mav.ecg.MavEcgPdfRenderer
import com.sennnen.mav.ecg.MavEcgReportContent
import java.io.File
import java.io.FileInputStream
import java.time.Instant
import kotlin.math.abs
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class EcgClassifierInstrumentedTest {
    @Test
    fun nineSyntheticCasesKeepTheirExpectedWinningClass() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val testAssets = instrumentation.context.assets
        val targetContext = instrumentation.targetContext
        val manifest = JSONObject(
            testAssets.open("manifest.json").bufferedReader().use { it.readText() },
        )
        assertEquals("mav/ecg-model-corpus/v1", manifest.getString("schema"))
        assertEquals(7_680, manifest.getInt("sample_count"))
        val classes = manifest.getJSONArray("classes")
        val cases = manifest.getJSONArray("cases")
        assertEquals(9, cases.length())

        MavEcgClassifier(targetContext).use { classifier ->
            for (caseIndex in 0 until cases.length()) {
                val fixture = cases.getJSONObject(caseIndex)
                val id = fixture.getString("id")
                val input = normalizedSignal(testAssets, id)
                val probabilities = classifier.predict(input)
                assertEquals(id, 3, probabilities.size)
                assertTrue(id, probabilities.all(Float::isFinite))
                assertEquals(id, 1f, probabilities.sum(), 0.001f)
                val winner = probabilities.indices.maxBy { probabilities[it] }
                assertEquals(id, fixture.getString("expected"), classes.getString(winner))
                val expected = fixture.getJSONArray("tflite_fp16")
                probabilities.indices.forEach { index ->
                    assertEquals(id, expected.getDouble(index), probabilities[index].toDouble(), 0.02)
                }
            }
        }
    }

    @Test
    fun batchPredictionsReturnInRequestOrder() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val testAssets = instrumentation.context.assets
        val ids = listOf("n_regular_55", "a_irregular_70", "o_tachy_120")
        MavEcgClassifier(instrumentation.targetContext).use { classifier ->
            val outputs = classifier.predictBatch(ids.map { normalizedSignal(testAssets, it) })
            assertEquals(ids.size, outputs.size)
            assertEquals(listOf(0, 1, 2), outputs.map { it.indices.maxBy { index -> it[index] } })
        }
    }

    @Test
    fun everyTfliteFixtureProducesAReadableOnePageNativePdf() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val testAssets = instrumentation.context.assets
        val targetContext = instrumentation.targetContext
        val manifest = JSONObject(
            testAssets.open("manifest.json").bufferedReader().use { it.readText() },
        )
        val classes = manifest.getJSONArray("classes")
        val cases = manifest.getJSONArray("cases")
        val directory = File(
            requireNotNull(targetContext.getExternalFilesDir(null)),
            "MaverickECGReports/TFLite",
        ).apply { mkdirs() }
        val exportDirectory = "/sdcard/Download/MaverickECGReports/TFLite"
        shell("mkdir -p $exportDirectory")

        MavEcgClassifier(targetContext).use { classifier ->
            for (caseIndex in 0 until cases.length()) {
                val fixture = cases.getJSONObject(caseIndex)
                val id = fixture.getString("id")
                val modelInput = normalizedSignal(testAssets, id)
                val waveform = millivoltSignal(testAssets, id)
                val probabilities = classifier.predict(modelInput)
                val winner = probabilities.indices.maxBy { probabilities[it] }
                val report = MavEcgReportContent(
                    captureId = (caseIndex + 1).toULong(),
                    recordedAt = Instant.ofEpochSecond(1_752_600_000L + caseIndex * 60L),
                    rhythm = rhythm(classes.getString(winner)),
                    probabilities = probabilities,
                    confidence = confidence(probabilities),
                    quality = 0.94f,
                    sampleRateHz = manifest.getInt("sample_rate_hz"),
                    sampleCount = waveform.size,
                    sourceUnit = "millivolts",
                    waveform = waveform,
                    explanation = explanation(modelInput),
                    modelSha256 = MavEcgClassifier.ADMITTED_MODEL_SHA256,
                    preprocessingSha256 =
                        "793dddb8f59e71d8a9b24cbd03e02efe0b361879027cf525a2a3dd6435edff24",
                    algorithmVersion = "2.0.0",
                    provisional = true,
                )
                val bytes = MavEcgPdfRenderer.render(report)
                assertTrue(id, bytes.copyOfRange(0, 4).contentEquals("%PDF".toByteArray()))
                val file = File(directory, "${id}_tflite.pdf")
                file.writeBytes(bytes)
                ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY).use { descriptor ->
                    PdfRenderer(descriptor).use { renderer ->
                        assertEquals(id, 1, renderer.pageCount)
                    }
                }
                shell("cp ${file.absolutePath} $exportDirectory/${file.name}")
            }
        }

        assertEquals(9, directory.listFiles { file -> file.extension == "pdf" }?.size)
    }

    private fun normalizedSignal(
        assets: android.content.res.AssetManager,
        id: String,
    ): FloatArray = signalColumn(assets, id, 2)

    private fun millivoltSignal(
        assets: android.content.res.AssetManager,
        id: String,
    ): FloatArray = signalColumn(assets, id, 1)

    private fun signalColumn(
        assets: android.content.res.AssetManager,
        id: String,
        column: Int,
    ): FloatArray = assets.open("$id.csv").bufferedReader().useLines { lines ->
        lines.drop(1)
            .map { line -> line.split(',')[column].toFloat() }
            .toList()
            .toFloatArray()
    }

    private fun shell(command: String) {
        InstrumentationRegistry.getInstrumentation().uiAutomation
            .executeShellCommand(command)
            .use { descriptor -> FileInputStream(descriptor.fileDescriptor).use { it.readBytes() } }
    }

    private fun rhythm(code: String): String = when (code) {
        "N" -> "sinus_rhythm"
        "A" -> "atrial_fibrillation"
        else -> "other_abnormal_rhythm"
    }

    private fun confidence(values: FloatArray): Float {
        val ordered = values.sortedDescending()
        return if (ordered.size < 2) 0f else ((ordered[0] - ordered[1]) / 0.2f).coerceIn(0f, 1f)
    }

    private fun explanation(waveform: FloatArray): List<MavEcgReportContent.Segment> {
        val segmentSize = waveform.size / 6
        val energies = List(6) { segment ->
            waveform.copyOfRange(segment * segmentSize, (segment + 1) * segmentSize)
                .sumOf { abs(it).toDouble() }
                .toFloat() / segmentSize
        }
        val maximum = energies.max()
        return energies.mapIndexed { index, energy ->
            MavEcgReportContent.Segment(
                startSecond = index * 5,
                endSecond = (index + 1) * 5,
                importance = if (maximum > 0f) energy / maximum else 0f,
            )
        }
    }
}
