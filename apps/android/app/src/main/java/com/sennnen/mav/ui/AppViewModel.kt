package com.sennnen.mav.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.sennnen.mav.BuildConfig
import com.sennnen.mav.MavAppState
import com.sennnen.mav.MavSnapshot
import com.sennnen.mav.MavSnapshotDecoder
import com.sennnen.mav.analytics.V5HealthSignals
import com.sennnen.mav.ble.LiveState
import com.sennnen.mav.data.DailyMetric
import com.sennnen.mav.data.MetricSeriesRow
import com.sennnen.mav.data.SleepSession
import com.sennnen.mav.data.WorkoutRow
import java.io.File
import java.util.TimeZone
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import uniffi.mav_ffi.FfiException
import uniffi.mav_ffi.MavRuntime
import uniffi.mav_ffi.RuntimeConfig

/**
 * The single app-wide view model behind the Aura UI. It exposes the member surface the
 * Aura screens read, backed by the Rust core's
 * from `host-snapshot/v1` instead of the legacy on-device Room store and BLE engine.
 *
 * Surfaces the core cannot fill yet stay HONESTLY empty (no fabricated rows, no cached scores):
 * `recentDays` / `workouts` emit empty lists and the repo facade returns empty series, so every
 * hub renders its designed empty state until connector ingestion lands in the core.
 */
class AppViewModel(application: Application) : AndroidViewModel(application) {

    private val mutableState = MutableStateFlow<MavAppState>(MavAppState.Loading)
    val state: StateFlow<MavAppState> = mutableState.asStateFlow()

    private val mutableLive = MutableStateFlow(LiveState())
    /** Live connection + biometric readout, derived from the core snapshot's connection block. */
    val live: StateFlow<LiveState> = mutableLive.asStateFlow()

    private val mutableBpm = MutableStateFlow<Int?>(null)
    val bpm: StateFlow<Int?> = mutableBpm.asStateFlow()

    private val mutableDays = MutableStateFlow<List<DailyMetric>>(emptyList())
    /** Daily metric history, oldest → newest. Empty until the core exposes day aggregates. */
    val recentDays: StateFlow<List<DailyMetric>> = mutableDays.asStateFlow()

    private val mutableWorkouts = MutableStateFlow<List<WorkoutRow>>(emptyList())
    val workouts: StateFlow<List<WorkoutRow>> = mutableWorkouts.asStateFlow()

    private val mutableActiveWorkout = MutableStateFlow<ActiveWorkout?>(null)
    val activeWorkout: StateFlow<ActiveWorkout?> = mutableActiveWorkout.asStateFlow()

    /** A manual workout in progress (strain-hub live banner). Never set until live sessions land. */
    data class ActiveWorkout(val startMs: Long, val liveStrain: Double = 0.0)

    private val mutableV5Signals = MutableStateFlow<V5HealthSignals.Snapshot?>(null)
    /** Nightly heads-up bundle (cycle / illness ward); published once those analytics are admitted. */
    val v5Signals: StateFlow<V5HealthSignals.Snapshot?> = mutableV5Signals.asStateFlow()

    val repo: MavRepo = MavRepo()
    val mlEngine: MavMlSignals = MavMlSignals()

    /** The canonical device id day-keyed reads use; mirrors the legacy `activeStrapId`. */
    val activeStrapId: String get() = "my-whoop"

    private var runtime: MavRuntime? = null
    private var refreshJob: Job? = null

    init {
        refresh()
    }

    fun refresh() {
        if (refreshJob?.isActive == true) return
        refreshJob = viewModelScope.launch(Dispatchers.IO) {
            // Keep the last good snapshot on screen during a re-read; Loading only before the first.
            if (mutableState.value !is MavAppState.Ready) mutableState.value = MavAppState.Loading
            mutableState.value = runCatching {
                val active = runtime ?: openRuntime().also { runtime = it }
                val result = active.hostSnapshot(System.currentTimeMillis())
                val snapshot = MavSnapshotDecoder.decode(result.json, result.hash)
                publishLive(snapshot)
                MavAppState.Ready(snapshot)
            }.getOrElse(::failureState)
        }
    }

    /** Zone minutes derived from raw HR over [from, to]; null until the core serves HR history. */
    suspend fun workoutZoneMinutes(from: Long, to: Long): List<Double>? = null

    fun loadWorkouts() {
        // No workout source in the core yet; the flow stays empty on purpose.
    }

    /** Wrist haptics — no transport wiring in the host yet, so these are inert. */
    fun buzz(loops: Int = 2) {}

    fun buzzStrapOnce() {}

    fun stopHaptics() {}

    override fun onCleared() {
        runtime?.close()
    }

    private fun publishLive(snapshot: MavSnapshot) {
        val connected = snapshot.connectionState == "connected"
        mutableBpm.value = snapshot.currentBpm
        mutableLive.value = LiveState(
            connected = connected,
            bonded = connected,
            heartRate = snapshot.currentBpm,
            advertisingName = snapshot.deviceName,
            scanning = snapshot.connectionState == "scanning",
            statusNote = snapshot.recoveryUnavailableReason,
        )
    }

    private fun openRuntime(): MavRuntime {
        val context = getApplication<Application>()
        val database = File(context.noBackupFilesDir, "mav.sqlite")
        return MavRuntime(
            RuntimeConfig(
                databasePath = database.absolutePath,
                timezoneId = TimeZone.getDefault().id,
                transportCapacity = 128u,
                appVersion = BuildConfig.VERSION_NAME,
                appBuild = BuildConfig.VERSION_CODE.toString(),
            ),
        )
    }

    private fun failureState(error: Throwable): MavAppState.Failed =
        when (error) {
            is FfiException.Core -> MavAppState.Failed(
                code = "MAV-${error.code}",
                message = error.safeMessage,
            )
            else -> MavAppState.Failed(
                code = "MAV-STARTUP",
                message = error.message ?: "Core startup failed",
            )
        }
}

/**
 * Read facade with legacy `WhoopRepository` signatures, returning empty results until the core
 * exposes the matching read models. Suspend so call-sites keep their coroutine shape.
 */
class MavRepo {
    suspend fun metricSeries(
        deviceId: String,
        key: String,
        from: String,
        to: String,
    ): List<MetricSeriesRow> = emptyList()

    suspend fun days(deviceId: String): List<DailyMetric> = emptyList()

    suspend fun workouts(deviceId: String, from: Long, to: Long, limit: Int = 400): List<WorkoutRow> =
        emptyList()

    suspend fun sleepSessionsUnion(deviceId: String, from: Long, to: Long, limit: Int = 400): List<SleepSession> =
        emptyList()

    suspend fun computedSleepSessionsUnion(deviceId: String, from: Long, to: Long, limit: Int = 400): List<SleepSession> =
        emptyList()
}

/** On-device ML signal surface (AuraMlSignalsCard). Inert until the native-inference lane lands. */
class MavMlSignals {
    val backboneActive: StateFlow<Boolean> = MutableStateFlow(false)
    val stressLoad: StateFlow<Double?> = MutableStateFlow(null)
    val vo2max: StateFlow<Double?> = MutableStateFlow(null)
    val respirationRate: StateFlow<Double?> = MutableStateFlow(null)
}
