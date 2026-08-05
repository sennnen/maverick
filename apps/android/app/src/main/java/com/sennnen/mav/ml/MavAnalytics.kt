package com.sennnen.mav.ml

import android.content.Context
import com.sennnen.mav.data.MavCoreRuntime

/**
 * The one analytics engine, for the whole process.
 *
 * [MavAnalyticsEngine] enforces one pass at a time, and that guarantee is only worth anything if
 * everyone who can start a pass is holding the same engine. Two of them can: the view model, when
 * the wearer opens the app, and [MavAnalyticsWorker], when the OS grants a background window. They
 * can overlap — a window granted a second before the app is opened is exactly the case — so they
 * share one instance and the second caller is turned away rather than running the zoo twice.
 *
 * It is a holder rather than a static the activity fills in, because the activity is the one thing
 * that need not exist: WorkManager can start this process on its own, and a worker that found a
 * null static used to return success without running a model. It also keeps the engine — and the
 * runner's loaded models — alive across a rotation, which building it in the view model did not.
 */
object MavAnalytics {
    @Volatile
    private var engine: MavAnalyticsEngine? = null

    /** The engine, if one has been built. Null before the first pass is asked for. */
    fun opened(): MavAnalyticsEngine? = engine

    /**
     * The engine, building it over the process's core if this is the first ask.
     *
     * Throws if the core cannot be opened, which is the honest outcome: there is no analytics
     * without a store to read, and a caller that swallowed it would report a pass that never ran.
     */
    @Synchronized
    fun engine(context: Context): MavAnalyticsEngine {
        engine?.let { return it }
        val application = context.applicationContext
        return MavAnalyticsEngine(
            runtime = MavCoreAnalyticsRuntime(MavCoreRuntime.open(application)),
            runner = MavModelRunner(application),
        ).also { engine = it }
    }

    /** Replace the engine, for tests that drive the loop without a core. */
    @Synchronized
    internal fun install(replacement: MavAnalyticsEngine?) {
        engine = replacement
    }
}
