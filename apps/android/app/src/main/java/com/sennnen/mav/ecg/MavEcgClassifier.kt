package com.sennnen.mav.ecg

import android.content.Context
import java.io.FileInputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.MappedByteBuffer
import kotlin.math.exp
import kotlin.math.max
import org.tensorflow.lite.DataType
import org.tensorflow.lite.Interpreter

/**
 * Thin native inference boundary. Core owns preprocessing, labels, confidence and XAI policy.
 */
class MavEcgClassifier(
    context: Context,
) : AutoCloseable {
    private val interpreter = Interpreter(
        mapAsset(context, MODEL_ASSET),
        Interpreter.Options().setNumThreads(THREAD_COUNT),
    )

    init {
        val input = interpreter.getInputTensor(0)
        val output = interpreter.getOutputTensor(0)
        require(input.dataType() == DataType.FLOAT32 && input.shape().contentEquals(INPUT_SHAPE))
        require(output.dataType() == DataType.FLOAT32 && output.shape().contentEquals(OUTPUT_SHAPE))
    }

    @Synchronized
    fun predict(tensor: FloatArray): FloatArray {
        require(tensor.size == INPUT_SAMPLE_COUNT) {
            "ECG tensor has ${tensor.size} values; expected $INPUT_SAMPLE_COUNT"
        }
        val input = ByteBuffer.allocateDirect(INPUT_SAMPLE_COUNT * Float.SIZE_BYTES)
            .order(ByteOrder.nativeOrder())
        tensor.forEach(input::putFloat)
        input.rewind()

        val output = ByteBuffer.allocateDirect(OUTPUT_CLASS_COUNT * Float.SIZE_BYTES)
            .order(ByteOrder.nativeOrder())
        interpreter.run(input, output)
        output.rewind()
        val raw = FloatArray(OUTPUT_CLASS_COUNT) { output.float }
        return normalizeOutput(raw).also { probabilities ->
            require(probabilities.all(Float::isFinite)) { "ECG model returned a non-finite value" }
        }
    }

    /**
     * Occlusion-XAI supplies one baseline plus bounded masked tensors. Serial mapping preserves
     * core-request order and avoids concurrent access to one Interpreter.
     */
    @Synchronized
    fun predictBatch(tensors: List<FloatArray>): List<FloatArray> = tensors.map(::predict)

    override fun close() {
        interpreter.close()
    }

    companion object {
        const val MODEL_ASSET = "ecg/nao_full_ecg_model_fp16.tflite"
        const val ADMITTED_MODEL_SHA256 =
            "0be97329077d5d5d2791b8b7850baf8acaf8f12f96fdfad7bcdb4af37156ea21"
        const val INPUT_SAMPLE_COUNT = 7_680
        const val OUTPUT_CLASS_COUNT = 3
        const val THREAD_COUNT = 2
        val INPUT_SHAPE = intArrayOf(1, INPUT_SAMPLE_COUNT, 1)
        val OUTPUT_SHAPE = intArrayOf(1, OUTPUT_CLASS_COUNT)

        @JvmStatic
        internal fun normalizeOutput(values: FloatArray): FloatArray {
            require(values.size == OUTPUT_CLASS_COUNT)
            require(values.all(Float::isFinite))
            val sum = values.sum()
            val looksLikeProbabilities =
                values.all { it >= -0.0001f && it <= 1.0001f } &&
                    kotlin.math.abs(sum - 1f) < 0.001f
            val probabilities = if (looksLikeProbabilities) {
                val clipped = FloatArray(values.size) { values[it].coerceIn(0f, 1f) }
                val total = clipped.sum()
                if (total > 0f) FloatArray(values.size) { clipped[it] / total }
                else softmax(values)
            } else {
                softmax(values)
            }
            val calibrated = FloatArray(values.size) { max(probabilities[it], 0.000_000_1f) }
            val calibratedTotal = calibrated.sum()
            return FloatArray(values.size) { calibrated[it] / calibratedTotal }
        }

        private fun softmax(values: FloatArray): FloatArray {
            val maximum = values.max()
            val exponentials = DoubleArray(values.size) { index ->
                exp(values[index].toDouble() - maximum)
            }
            val total = exponentials.sum()
            return FloatArray(values.size) { index -> (exponentials[index] / total).toFloat() }
        }

        private fun mapAsset(context: Context, path: String): MappedByteBuffer =
            context.assets.openFd(path).use { descriptor ->
                FileInputStream(descriptor.fileDescriptor).channel.use { channel ->
                    channel.map(
                        java.nio.channels.FileChannel.MapMode.READ_ONLY,
                        descriptor.startOffset,
                        descriptor.declaredLength,
                    )
                }
            }
    }
}
