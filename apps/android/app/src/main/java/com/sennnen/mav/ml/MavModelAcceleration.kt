package com.sennnen.mav.ml

import java.io.File
import org.tensorflow.lite.Interpreter
import org.tensorflow.lite.gpu.CompatibilityList
import org.tensorflow.lite.gpu.GpuDelegate

/**
 * Where a model runs on Android, and at what arithmetic precision.
 *
 * iOS hands this problem to Core ML: the app asks for `.all` and the OS assigns each operation
 * to the CPU, the GPU or the Neural Engine. Android has no equivalent. The app picks a
 * delegate, and that one choice fixes both the processor and the arithmetic width.
 *
 * This used to be decided by inheritance — Core ML admitted the model at half precision, so
 * Android's half-precision delegate was assumed equivalent — and a sweep across all
 * forty-one models on a Tensor G2 showed the assumption failing three separate ways:
 *
 *  * **Half precision is not one behaviour.** Apple's Neural Engine accumulates a half-width
 *    matmul into a wider register; this delegate accumulates in half width. From identical
 *    weights Pulse-PPG lands 3.9e-3 from its reference under Core ML and 2.7e-2 here.
 *  * **On some graphs it is not a precision effect at all.** `activity_detection` and
 *    `cva_encoder` come back 7.2e-1 and 1.4e+1 away at *either* width, and `step_head` returns
 *    a whole relative unit at half width. Those are wrong answers, not imprecise ones.
 *  * **It is usually slower.** Dispatching to the GPU costs a few hundred microseconds, and
 *    all but one of these graphs are too small to earn that back. `awhr_imputation` takes
 *    3.7 ms on the CPU and 40 ms on the GPU.
 *
 * So the CPU is the default and the path is measured, not inferred: `preferredPath` comes from
 * `artifacts/models/android_delegate.json`, which a device run writes and which records the
 * timing and deviation behind every entry. One model leaves the CPU, at full width.
 *
 * NNAPI is gone rather than merely unused. It was deprecated in Android 15, no device here
 * could measure it, and an accelerator that silently changes the answer with no measurement
 * behind it is the exact failure this sweep existed to remove.
 *
 * A delegate that fails to attach is not fatal: it is recorded on the [Choice] and the CPU
 * runs the model, because a model that runs slowly is a working model and one that fails to
 * load is not.
 */
internal object MavModelAcceleration {
    /** Which path a model was actually given, and why any other was not taken. */
    data class Choice(
        val path: Path,
        val delegate: AutoCloseable?,
        val rejected: List<String>,
    )

    enum class Path {
        /** The GPU delegate at half width — faster, and measurably less accurate on this zoo. */
        GPU,

        /**
         * The GPU delegate with precision loss refused, so it computes at float32.
         *
         * Still the accelerator; what it gives up is the arithmetic width, not the hardware.
         * This is the path the one model that leaves the CPU takes, because it was the half
         * width and not the GPU that was costing it accuracy.
         */
        GPU_FULL,

        /** XNNPACK on the CPU, float32 arithmetic over float16 weights. */
        CPU,
    }

    /**
     * Build interpreter options for [entry] on the path it was measured onto.
     *
     * [force] overrides that choice and is used only by the delegate sweep, which has to
     * measure each path under its own name — and which decided [MavModelCatalog.Entry.preferredPath]
     * in the first place, so it cannot consult it.
     *
     * The caller owns the returned delegate and must close it after the interpreter.
     */
    fun configure(
        options: Interpreter.Options,
        entry: MavModelCatalog.Entry,
        force: Path? = null,
        threads: Int = MavModelRunner.THREAD_COUNT,
        /**
         * Where the GPU delegate may cache its compiled kernels, or null to compile every time.
         * Measured on a Tensor G2, compiling them is 538 ms of the 540 ms it takes to bring
         * `whr_unet_encoder` up — by far the largest single cost in loading the whole zoo.
         */
        kernelCache: File? = null,
    ): Choice {
        val rejected = mutableListOf<String>()
        val wanted = force ?: entry.preferredPath

        if (wanted == Path.GPU || wanted == Path.GPU_FULL) {
            try {
                val compatibility = CompatibilityList()
                if (compatibility.isDelegateSupportedOnThisDevice) {
                    val delegate = GpuDelegate(
                        compatibility.bestOptionsForThisDevice.apply {
                            // Explicit rather than inherited: this line is the arithmetic
                            // width, and a future change to the delegate's default would
                            // otherwise silently move the model off what was measured.
                            setPrecisionLossAllowed(wanted == Path.GPU)
                            setQuantizedModelsAllowed(false)
                            if (kernelCache != null) {
                                // The token is the admitted hash, so the cache is keyed to the
                                // exact bytes it was compiled from: a model that changes gets a
                                // new token and the stale kernels are simply never asked for.
                                // A miss or a corrupt entry costs a recompile, not a wrong
                                // answer — the delegate falls back to compiling.
                                kernelCache.mkdirs()
                                setSerializationParams(kernelCache.absolutePath, entry.admittedSha256)
                            }
                        },
                    )
                    options.addDelegate(delegate)
                    return Choice(wanted, delegate, rejected)
                }
                rejected += "gpu: not supported on this device"
            } catch (error: Throwable) {
                // A missing native library or a driver that refuses to initialise both arrive
                // as throwables from the constructor, and neither should stop the model loading.
                rejected += "gpu: ${error.javaClass.simpleName}"
            }
        }

        options.setUseXNNPACK(true)
        options.setNumThreads(threads)
        return Choice(Path.CPU, null, rejected)
    }
}
