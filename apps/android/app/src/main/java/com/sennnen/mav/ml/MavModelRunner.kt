package com.sennnen.mav.ml

import android.content.Context
import java.io.FileInputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer
import java.nio.IntBuffer
import java.nio.LongBuffer
import java.nio.MappedByteBuffer
import java.nio.channels.FileChannel
import java.security.MessageDigest
import org.tensorflow.lite.DataType
import org.tensorflow.lite.Interpreter

class MavModelException(message: String) : IllegalStateException(message)

/**
 * Runs one TensorFlow Lite model from the zoo, and nothing else.
 *
 * Every judgement — which model, what a tensor means, whether a prediction may be believed —
 * stays in the shared core. This class maps a bundled asset, asserts the shapes the generated
 * catalogue declares, binds named float buffers, and hands the numbers straight back.
 *
 * Unlike iOS, the shipped bytes are exactly the bytes the manifest recorded, so the runner hashes
 * the mapped buffer at load and refuses a model whose SHA-256 is not the admitted one. A swapped
 * asset fails at open rather than producing a number nothing can be attributed to.
 *
 * Everything an inference needs that does not depend on the input — which runtime tensor each
 * contract name binds to, how wide it is, and the direct buffer to stage it in — is worked out
 * once, at load, and kept on a [Loaded]. It used to be worked out on every call: two
 * `allocateDirect` per tensor per inference, a linear scan over tensor names (twice per output,
 * once to bind and once to read), and element-at-a-time buffer fills. On a model that answers
 * in twenty microseconds that overhead *was* the inference.
 */
class MavModelRunner internal constructor(
    private val context: Context,
    /**
     * Attach this delegate instead of the one the catalogue implies, or null for the normal
     * choice. Only the delegate sweep passes it: that test exists to measure each path under
     * its own name, and it decided the flag the normal choice now reads.
     */
    private val forcedPath: MavModelAcceleration.Path?,
    /**
     * How many CPU threads XNNPACK may use, or null for the measured default. Only the thread
     * sweep passes it; it exists so the default can be chosen from evidence on real hardware
     * rather than picked.
     */
    private val threadOverride: Int? = null,
) : AutoCloseable, MavModelBridge.Runner {
    constructor(context: Context) : this(context, null, null)

    private val threads = threadOverride ?: THREAD_COUNT

    /** Where the GPU delegate keeps compiled kernels between launches. */
    private val kernelCache: java.io.File by lazy {
        java.io.File(context.cacheDir, "tflite-gpu-kernels")
    }

    /**
     * One model, loaded, with everything its inferences will need.
     *
     * `lock` is per model rather than per runner so a four-minute Pulse-PPG window does not
     * hold every other model still behind it. A TensorFlow Lite interpreter is not thread-safe,
     * and the staging buffers below are shared between calls, so the *same* model still runs
     * one inference at a time.
     */
    private class Loaded(
        val interpreter: Interpreter,
        val delegate: AutoCloseable?,
        val path: MavModelAcceleration.Path,
        val sha256: String,
        val plan: BindPlan,
        val timings: LoadTimings,
        /** Native bytes this interpreter took to build, measured as it was built. */
        val nativeBytes: Long,
    ) {
        val lock = Any()

        /**
         * How many callers are inside [run] for this model. Eviction skips anything above
         * zero: closing an interpreter that native code is executing is a crash, not a
         * reclaimed page.
         */
        var inUse: Int = 0
    }

    /**
     * Where the cold path's time went, in nanoseconds.
     *
     * Split because the three parts have unrelated fixes. Mapping is the filesystem; hashing is
     * the integrity check, which scales with the asset and is 57 MB for Pulse-PPG; building the
     * interpreter is the graph, and for a delegated model it also builds the delegate's
     * kernels, which was over a second for the one model on the GPU.
     */
    internal data class LoadTimings(
        val mapNanos: Long,
        val hashNanos: Long,
        val interpreterNanos: Long,
    )

    /**
     * Where each contract tensor lives at run time, and the buffer it is staged through.
     *
     * The buffers are allocated once and rewound per call. They are direct, because that is
     * what the interpreter can read without copying again, and direct allocation is a native
     * malloc — doing it per inference is the one cost here that grows with how often the app
     * infers rather than with how big the model is.
     */
    private class BindPlan(
        val inputIndex: IntArray,
        val inputType: Array<DataType>,
        val inputBuffer: Array<ByteBuffer>,
        val outputIndex: IntArray,
        val outputType: Array<DataType>,
        val outputBuffer: Array<ByteBuffer>,
    ) {
        /** Typed views over the same memory, so a fill is one bulk copy rather than a loop. */
        val inputFloats: Array<FloatBuffer?> = Array(inputBuffer.size) {
            if (inputType[it] == DataType.FLOAT32) inputBuffer[it].asFloatBuffer() else null
        }
        val inputInts: Array<IntBuffer?> = Array(inputBuffer.size) {
            if (inputType[it] == DataType.INT32) inputBuffer[it].asIntBuffer() else null
        }
        val inputLongs: Array<LongBuffer?> = Array(inputBuffer.size) {
            if (inputType[it] == DataType.INT64) inputBuffer[it].asLongBuffer() else null
        }
        val outputFloats: Array<FloatBuffer?> = Array(outputBuffer.size) {
            if (outputType[it] == DataType.FLOAT32) outputBuffer[it].asFloatBuffer() else null
        }
        val outputInts: Array<IntBuffer?> = Array(outputBuffer.size) {
            if (outputType[it] == DataType.INT32) outputBuffer[it].asIntBuffer() else null
        }
        val outputLongs: Array<LongBuffer?> = Array(outputBuffer.size) {
            if (outputType[it] == DataType.INT64) outputBuffer[it].asLongBuffer() else null
        }

        /** Reused argument holders: `runForMultipleInputsOutputs` wants an array and a map. */
        val arguments: Array<Any> = Array(inputBuffer.size) { inputBuffer[it] }
        val sinks: MutableMap<Int, Any> = HashMap<Int, Any>(outputBuffer.size).apply {
            outputIndex.forEachIndexed { position, runtimeIndex -> put(runtimeIndex, outputBuffer[position]) }
        }
    }

    /** Guards [loaded] only. Never held across an inference. */
    private val registry = Any()

    /** Access-ordered, so iteration order is least-recently-used first for eviction. */
    private val loaded = LinkedHashMap<String, Loaded>(16, 0.75f, true)

    override fun run(slug: String, inputs: Map<String, FloatArray>): Map<String, FloatArray> {
        val entry = MavModelCatalog.entries[slug]
            ?: throw MavModelException("this build ships no model named $slug")
        val model = acquire(entry)
        val plan = model.plan

        try {
            synchronized(model.lock) {
            entry.inputs.forEachIndexed { position, spec ->
                val values = inputs[spec.name]
                    ?: throw MavModelException("${entry.slug} is missing its ${spec.name} tensor")
                if (values.size != spec.elementCount) {
                    throw MavModelException(
                        "${entry.slug} input ${spec.name} has ${values.size} values, " +
                            "expected ${spec.elementCount}",
                    )
                }
                // Integer inputs travel as whole-numbered floats across the FFI and are cast
                // here, rather than anywhere the value could be silently rounded.
                //
                // Both widths are real. A sequence length converts to INT32; the behaviour-id
                // lookup keeps the INT64 its `torch.int64` indices exported as. Writing the
                // wrong width is not a rounding problem — it puts half as many bytes as the
                // tensor expects into the buffer, and the interpreter reads whatever follows.
                when (plan.inputType[position]) {
                    DataType.INT32 -> plan.inputInts[position]!!.let { view ->
                        for (index in values.indices) view.put(index, values[index].toInt())
                    }
                    DataType.INT64 -> plan.inputLongs[position]!!.let { view ->
                        for (index in values.indices) view.put(index, values[index].toLong())
                    }
                    else -> plan.inputFloats[position]!!.let { view ->
                        view.clear()
                        view.put(values)
                    }
                }
                plan.inputBuffer[position].rewind()
            }
            plan.outputBuffer.forEach { it.rewind() }

            model.interpreter.runForMultipleInputsOutputs(plan.arguments, plan.sinks)

            val outputs = LinkedHashMap<String, FloatArray>(entry.outputs.size)
            entry.outputs.forEachIndexed { position, spec ->
                plan.outputBuffer[position].rewind()
                // Widened to Float on the way out, because that is the one type the FFI speaks.
                // An INT64 label is a small whole number and survives the widening exactly; the
                // core casts it back where it means a class.
                val values = FloatArray(spec.elementCount)
                when (plan.outputType[position]) {
                    DataType.INT32 -> plan.outputInts[position]!!.let { view ->
                        for (index in values.indices) values[index] = view.get(index).toFloat()
                    }
                    DataType.INT64 -> plan.outputLongs[position]!!.let { view ->
                        for (index in values.indices) values[index] = view.get(index).toFloat()
                    }
                    else -> plan.outputFloats[position]!!.let { view ->
                        view.clear()
                        view.get(values)
                    }
                }
                for (value in values) {
                    if (!value.isFinite()) {
                        throw MavModelException("${entry.slug} returned a non-finite ${spec.name}")
                    }
                }
                outputs[spec.name] = values
            }
            return outputs
            }
        } finally {
            release(model)
        }
    }

    /** The SHA-256 of the asset actually mapped for this model. */
    override fun loadedSha256(slug: String): String {
        val entry = MavModelCatalog.entries[slug]
            ?: throw MavModelException("this build ships no model named $slug")
        return loadedFor(entry).sha256
    }

    /**
     * Close every interpreter. Called when the app backgrounds: the zoo's assets total well over
     * a hundred megabytes, and a mapped interpreter holds its buffer resident.
     */
    override fun close() {
        val closing = synchronized(registry) {
            val snapshot = loaded.values.toList()
            loaded.clear()
            snapshot
        }
        // Interpreters first: a delegate closed while an interpreter still holds it is a
        // use-after-free in native code rather than an exception here.
        closing.forEach { it.interpreter.close() }
        closing.forEach { it.delegate?.close() }
    }

    private fun loadedFor(entry: MavModelCatalog.Entry): Loaded = synchronized(registry) {
        loaded.getOrPut(entry.slug) { load(entry) }.also { evictIdle(entry.slug) }
    }

    /** Take a reference that eviction must respect for as long as an inference is running. */
    private fun acquire(entry: MavModelCatalog.Entry): Loaded = synchronized(registry) {
        loaded.getOrPut(entry.slug) { load(entry) }.also {
            it.inUse++
            evictIdle(entry.slug)
        }
    }

    private fun release(model: Loaded) = synchronized(registry) {
        model.inUse--
    }

    /**
     * Close least-recently-used interpreters until the resident total is back under budget.
     *
     * The cache was unbounded, and on the assumption that a hundred-megabyte bundle costs
     * about a hundred megabytes to hold. It does not. Loading all forty-one on a Pixel 7 took
     * the process to **1.05 GB** of native heap from an 86.8 MB bundle, because the
     * interpreter builds tensor arenas and XNNPACK repacks the float16 weights to float32 —
     * `pulse_ppg` alone is 436 MB from a 57 MB asset, and `whr_unet_head` 165 MB from 2 MB.
     * A process that holds that gets killed, and the model that was loaded to answer a
     * question is lost along with it.
     *
     * [keeping] and anything mid-inference are never evicted, so the budget is a bound on what
     * is held *idle*: a single model larger than the whole budget still runs, it just does not
     * get to keep company.
     */
    private fun evictIdle(keeping: String) {
        val evicting = MavModelEviction.choose(
            loaded.map { (slug, model) ->
                MavModelEviction.Candidate(slug, model.nativeBytes, model.inUse > 0)
            },
            RESIDENT_BUDGET_BYTES,
            keeping,
        )
        for (slug in evicting) {
            val model = loaded.remove(slug) ?: continue
            model.interpreter.close()
            model.delegate?.close()
        }
    }

    private fun load(entry: MavModelCatalog.Entry): Loaded {
        val nativeBefore = android.os.Debug.getNativeHeapAllocatedSize()
        val mapStarted = System.nanoTime()
        val buffer = mapAsset(entry.assetPath)
        val hashStarted = System.nanoTime()
        val digest = sha256(buffer)
        val interpreterStarted = System.nanoTime()
        if (!digest.equals(entry.admittedSha256, ignoreCase = true)) {
            throw MavModelException(
                "${entry.slug} asset hashes to $digest, which this build does not admit",
            )
        }
        val options = Interpreter.Options().setNumThreads(threads)
        val choice = MavModelAcceleration.configure(options, entry, forcedPath, threads, kernelCache)
        var path = choice.path
        var delegate = choice.delegate
        val interpreter = try {
            Interpreter(buffer, options)
        } catch (error: IllegalArgumentException) {
            // A delegate the device advertised but cannot actually run *this graph* on
            // fails here rather than when it was attached. The CPU path is always
            // available, so fall back to it: a model that runs slowly is a working model.
            choice.delegate?.close()
            delegate = null
            path = MavModelAcceleration.Path.CPU
            Interpreter(buffer, Interpreter.Options().setNumThreads(threads))
        }
        assertContract(interpreter, entry)
        return Loaded(
            interpreter,
            delegate,
            path,
            digest,
            planFor(interpreter, entry),
            LoadTimings(
                mapNanos = hashStarted - mapStarted,
                hashNanos = interpreterStarted - hashStarted,
                interpreterNanos = System.nanoTime() - interpreterStarted,
            ),
            // Measured rather than estimated from the asset size, which understates it by
            // between three and eighty times depending on the graph.
            nativeBytes = (android.os.Debug.getNativeHeapAllocatedSize() - nativeBefore)
                .coerceAtLeast(0L),
        )
    }

    /** How long this model's cold path spent mapping, hashing and building. */
    internal fun loadTimings(slug: String): LoadTimings {
        val entry = MavModelCatalog.entries[slug]
            ?: throw MavModelException("this build ships no model named $slug")
        return loadedFor(entry).timings
    }

    /**
     * Resolve every contract tensor to a runtime index and a staging buffer, once.
     *
     * Sized from the tensor rather than assumed. Not every output is a float: the risk tree
     * returns its chosen class as an INT64 label beside its float probabilities, and a buffer
     * sized for floats is half the bytes the interpreter is about to write. It refuses the copy
     * rather than overrunning, so the model simply could not be run on Android at all.
     */
    private fun planFor(interpreter: Interpreter, entry: MavModelCatalog.Entry): BindPlan {
        val inputIndex = IntArray(entry.inputs.size)
        val inputType = ArrayList<DataType>(entry.inputs.size)
        val inputBuffer = ArrayList<ByteBuffer>(entry.inputs.size)
        entry.inputs.forEachIndexed { position, spec ->
            val runtimeIndex = interpreterInputIndex(interpreter, entry, spec.name, position)
            val tensor = interpreter.getInputTensor(runtimeIndex)
            inputIndex[position] = runtimeIndex
            inputType += tensor.dataType()
            inputBuffer += ByteBuffer.allocateDirect(tensor.numBytes()).order(ByteOrder.nativeOrder())
        }
        val outputIndex = IntArray(entry.outputs.size)
        val outputType = ArrayList<DataType>(entry.outputs.size)
        val outputBuffer = ArrayList<ByteBuffer>(entry.outputs.size)
        entry.outputs.forEachIndexed { position, spec ->
            val runtimeIndex = interpreterOutputIndex(interpreter, entry, spec.name, position)
            val tensor = interpreter.getOutputTensor(runtimeIndex)
            outputIndex[position] = runtimeIndex
            outputType += tensor.dataType()
            outputBuffer += ByteBuffer.allocateDirect(tensor.numBytes()).order(ByteOrder.nativeOrder())
        }
        // The interpreter takes inputs positionally by runtime index, so the argument array has
        // to be in runtime order even though the contract may name them in another.
        val plan = BindPlan(
            inputIndex,
            inputType.toTypedArray(),
            inputBuffer.toTypedArray(),
            outputIndex,
            outputType.toTypedArray(),
            outputBuffer.toTypedArray(),
        )
        inputIndex.forEachIndexed { position, runtimeIndex ->
            plan.arguments[runtimeIndex] = plan.inputBuffer[position]
        }
        return plan
    }

    /**
     * Which execution path this model was actually given. Read by the diagnostics report.
     *
     * Internal because [MavModelAcceleration.Path] is: which delegate attached is a fact about
     * this layer, not part of the surface the app screens are allowed to reason about. The
     * instrumented tests are an associated compilation and so can still see it.
     */
    internal fun executionPath(slug: String): MavModelAcceleration.Path {
        val entry = MavModelCatalog.entries[slug]
            ?: throw MavModelException("this build ships no model named $slug")
        return loadedFor(entry).path
    }

    private fun assertContract(interpreter: Interpreter, entry: MavModelCatalog.Entry) {
        if (interpreter.inputTensorCount != entry.inputs.size) {
            throw MavModelException(
                "${entry.slug} takes ${interpreter.inputTensorCount} tensors, " +
                    "the contract declares ${entry.inputs.size}",
            )
        }
        if (interpreter.outputTensorCount != entry.outputs.size) {
            throw MavModelException(
                "${entry.slug} returns ${interpreter.outputTensorCount} tensors, " +
                    "the contract declares ${entry.outputs.size}",
            )
        }
        entry.inputs.forEachIndexed { index, spec ->
            val runtimeIndex = interpreterInputIndex(interpreter, entry, spec.name, index)
            val shape = interpreter.getInputTensor(runtimeIndex).shape()
            if (shape.fold(1) { total, side -> total * side } != spec.elementCount) {
                throw MavModelException(
                    "${entry.slug} input ${spec.name} is ${shape.toList()}, " +
                        "the contract declares ${spec.shape.toList()}",
                )
            }
        }
        entry.outputs.forEachIndexed { index, spec ->
            val runtimeIndex = interpreterOutputIndex(interpreter, entry, spec.name, index)
            val shape = interpreter.getOutputTensor(runtimeIndex).shape()
            if (shape.fold(1) { total, side -> total * side } != spec.elementCount) {
                throw MavModelException(
                    "${entry.slug} output ${spec.name} is ${shape.toList()}, " +
                        "the contract declares ${spec.shape.toList()}",
                )
            }
        }
    }

    /**
     * Bind by name where the converter preserved one, and fall back to contract order.
     *
     * LiteRT keeps the exported signature names for most graphs but decorates some
     * (`serving_default_ppg:0`), so the match is on the trailing component. Order is the
     * documented fallback because the contract lists tensors in the order they were exported.
     */
    private fun interpreterInputIndex(
        interpreter: Interpreter,
        entry: MavModelCatalog.Entry,
        name: String,
        fallback: Int,
    ): Int {
        for (index in 0 until interpreter.inputTensorCount) {
            if (matches(interpreter.getInputTensor(index).name(), name)) return index
        }
        return fallback.also {
            if (it >= interpreter.inputTensorCount) {
                throw MavModelException("${entry.slug} has no input tensor for $name")
            }
        }
    }

    private fun interpreterOutputIndex(
        interpreter: Interpreter,
        entry: MavModelCatalog.Entry,
        name: String,
        fallback: Int,
    ): Int {
        for (index in 0 until interpreter.outputTensorCount) {
            if (matches(interpreter.getOutputTensor(index).name(), name)) return index
        }
        return fallback.also {
            if (it >= interpreter.outputTensorCount) {
                throw MavModelException("${entry.slug} has no output tensor for $name")
            }
        }
    }

    private fun mapAsset(path: String): MappedByteBuffer =
        context.assets.openFd(path).use { descriptor ->
            FileInputStream(descriptor.fileDescriptor).channel.use { channel ->
                channel.map(
                    FileChannel.MapMode.READ_ONLY,
                    descriptor.startOffset,
                    descriptor.declaredLength,
                )
            }
        }

    companion object {
        /**
         * How many CPU threads XNNPACK gets.
         *
         * Two, and now for a measured reason rather than none. Swept across 1, 2, 4 and 8 on a
         * Tensor G2 over the sixteen models heavy enough for it to matter, the whole zoo is
         * flat: `pulse_ppg` moves from 2553 ms to 2545 ms between one thread and two and is
         * unchanged at eight, because XNNPACK does not parallelise its 1-D convolutions. Eight
         * is actively harmful — `activity_detection` doubles, 11.1 ms to 22.1 ms, thrashing the
         * four little cores. Two is within noise of the best on every model and never the worst.
         */
        const val THREAD_COUNT = 2

        /**
         * How many bytes of loaded interpreters may sit idle before the oldest are closed.
         *
         * Measured on a Pixel 7: the forty-one models cost 1.05 GB of native heap held all at
         * once, and the distribution is long-tailed — `pulse_ppg` 436 MB, `whr_unet_head`
         * 165 MB, the three sleep networks 60-98 MB each, and everything else under 26 MB.
         * 192 MB holds the entire small end of that distribution, which is what a session
         * actually cycles through, and refuses to keep two of the giants resident at once.
         *
         * It is a budget for *idle* models. Whatever is being inferred is exempt, so raising or
         * lowering this changes how often a model is rebuilt, never whether one can run.
         */
        const val RESIDENT_BUDGET_BYTES = 192L * 1024L * 1024L

        @JvmStatic
        internal fun matches(runtimeName: String, contractName: String): Boolean {
            // Path first, then the output index. The other order turns
            // "StatefulPartitionedCall:0/embeddings" into "StatefulPartitionedCall", because
            // cutting at the colon throws away the slash that carried the real name.
            val trimmed = runtimeName.substringAfterLast('/').substringBefore(':')
            return trimmed == contractName ||
                trimmed.removePrefix("serving_default_") == contractName ||
                runtimeName == contractName
        }

        @JvmStatic
        internal fun sha256(buffer: ByteBuffer): String {
            val digest = MessageDigest.getInstance("SHA-256")
            val duplicate = buffer.duplicate()
            duplicate.rewind()
            digest.update(duplicate)
            return digest.digest().joinToString("") { byte -> "%02x".format(byte) }
        }
    }
}
