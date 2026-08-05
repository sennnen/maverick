package com.sennnen.mav.ml

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.sennnen.mav.data.MavCoreRuntime
import java.util.concurrent.TimeUnit

/**
 * Analytics while the screen is off, the phone is locked, or the app is backgrounded.
 *
 * WorkManager rather than a service, because none of this is urgent and none of it is the
 * wearer's idea: nobody asked for an embedding at 4am, they asked for the answer to be there at
 * 7. The constraints below say the same thing — do this when the phone is idle and on a charger,
 * because a foundation encoder over a night of PPG is not work to spend a commute's battery on.
 *
 * What the OS actually grants is not knowable in advance and is deliberately not assumed. A
 * window may be minutes or hours late, may be skipped while the device is in a deep standby
 * bucket, and never happens at all while the phone is off. Every one of those degrades to the
 * same place: the next foreground open runs an interactive pass and the wearer waits a moment
 * instead of not waiting. That fallback is the contract; the background window is the
 * optimisation.
 */
class MavAnalyticsWorker(
    context: Context,
    parameters: WorkerParameters,
) : CoroutineWorker(context, parameters) {

    override suspend fun doWork(): Result {
        // The process's one engine, whether or not an activity exists. Sharing it is what makes a
        // background pass and a foreground pass contend for the same single-pass lock instead of
        // running the zoo twice — and reaching it through a holder rather than a static the
        // activity fills in is what makes a cold, UI-less process actually do the work. It used
        // to find a null provider here and return success without running a model.
        val engine = runCatching { MavAnalytics.engine(applicationContext) }
            .getOrElse { return Result.failure() }
        val deviceId = inputData.getLong(KEY_DEVICE_ID, DEFAULT_DEVICE_ID).toULong()
        return when (engine.runPass(deviceId, MavRunMode.DEFERRED)) {
            MavAnalyticsEngine.Outcome.COMPLETED -> Result.success()
            // Another pass held the lock — the app is open and doing this work in the
            // foreground, which is strictly better. Nothing to retry.
            MavAnalyticsEngine.Outcome.SKIPPED_BUSY -> Result.success()
            // Partial and failed both come back later. WorkManager's own exponential backoff is
            // what spaces them, so a strap that is producing nothing does not spin.
            MavAnalyticsEngine.Outcome.PARTIAL,
            MavAnalyticsEngine.Outcome.FAILED,
            -> Result.retry()
        }
    }

    companion object {
        const val PERIODIC_NAME = "mav-analytics-periodic"
        const val PRECOMPUTE_NAME = "mav-analytics-precompute"
        const val KEY_DEVICE_ID = "device_id"
        const val DEFAULT_DEVICE_ID = 1L

        /**
         * The constraints every background pass runs under.
         *
         * Battery-not-low and idle are the honest ones: this is speculative work for a wearer
         * who is asleep. Charging is *not* required — requiring it means a wearer who does not
         * charge overnight never gets a background pass at all — but it is preferred through the
         * periodic schedule landing at night. No network constraint, because nothing here
         * touches the network; asking for one would only add a reason to be skipped.
         */
        fun constraints(): Constraints =
            Constraints.Builder()
                .setRequiresBatteryNotLow(true)
                .setRequiresDeviceIdle(true)
                .setRequiredNetworkType(NetworkType.NOT_REQUIRED)
                .build()

        /** Keep a nightly pass scheduled. Idempotent: safe to call on every app start. */
        fun schedulePeriodic(context: Context) {
            val request = PeriodicWorkRequestBuilder<MavAnalyticsWorker>(6, TimeUnit.HOURS)
                .setConstraints(constraints())
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 15, TimeUnit.MINUTES)
                .build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(
                PERIODIC_NAME,
                // KEEP, not UPDATE: replacing the request on every launch resets its window and
                // an app opened daily would never reach one.
                ExistingPeriodicWorkPolicy.KEEP,
                request,
            )
        }

        /**
         * Ask for one pass [delayMinutes] from now, ahead of a likely app open.
         *
         * Best-effort by construction. The OS decides when — or whether — this runs, so nothing
         * downstream may assume it did.
         */
        fun schedulePrecompute(context: Context, delayMinutes: Long) {
            val request = OneTimeWorkRequestBuilder<MavAnalyticsWorker>()
                .setConstraints(constraints())
                .setInitialDelay(delayMinutes.coerceAtLeast(0), TimeUnit.MINUTES)
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 10, TimeUnit.MINUTES)
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork(
                PRECOMPUTE_NAME,
                ExistingWorkPolicy.REPLACE,
                request,
            )
        }

        /**
         * Minutes to wait before a precompute aimed at the wearer's next likely open.
         *
         * Null when the pattern has nothing to say — which is the case for a new install, and
         * scheduling a guess then would be work for nobody.
         */
        fun precomputeDelayMinutes(pattern: MavUsagePattern, hourNow: Int): Long? {
            if (pattern.confidence() <= 0.0) return null
            val target = pattern.likelyHours(1).firstOrNull() ?: return null
            val lead = (target - MavUsagePattern.LEAD_HOURS + MavUsagePattern.HOURS) % MavUsagePattern.HOURS
            val ahead = (lead - hourNow + MavUsagePattern.HOURS) % MavUsagePattern.HOURS
            // Landing exactly now is not a schedule; push a zero out a full day so the request
            // still aims at the same hour tomorrow.
            return if (ahead == 0) MavUsagePattern.HOURS * 60L else ahead * 60L
        }

        /** Stop everything. Called when analytics is switched off or the store is cleared. */
        fun cancelAll(context: Context) {
            val manager = WorkManager.getInstance(context)
            manager.cancelUniqueWork(PERIODIC_NAME)
            manager.cancelUniqueWork(PRECOMPUTE_NAME)
        }
    }
}
