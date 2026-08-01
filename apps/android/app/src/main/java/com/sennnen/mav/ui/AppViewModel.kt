package com.sennnen.mav.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.sennnen.mav.MavAppState
import com.sennnen.mav.MavSnapshot
import com.sennnen.mav.ble.LiveState
import com.sennnen.mav.connector.AndroidConnectorManager
import com.sennnen.mav.data.MetricSeriesRow
import com.sennnen.mav.data.SleepSession
import com.sennnen.mav.data.WorkoutRow
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import com.sennnen.mav.data.DailyMetric
import uniffi.mav_ffi.DailySnapshotReport
import uniffi.mav_ffi.FfiException
import uniffi.mav_ffi.MavRuntime

/**
 * The single app-wide view model behind the Aura UI. It exposes the member surface the
 * Aura screens read, backed by the Rust core instead of the legacy Room store and BLE engine.
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
    /**
     * A manual workout in progress. `sport` is carried rather than looked up because the live
     * screen has to name the activity, and the session does not exist in the store until it ends.
     */
    data class ActiveWorkout(
        val startMs: Long,
        val sport: String,
        val liveStrain: Double = 0.0,
    )

    private val mutableSyncNote = MutableStateFlow<String?>(null)
    /** The core's `historical-status/v1` progress as a display line; null when idle or unknown. */
    val syncNote: StateFlow<String?> = mutableSyncNote.asStateFlow()

    private val mutableSnapshot = MutableStateFlow<DailySnapshotReport?>(null)
    /**
     * Today's analytics as the shared core computed them, with the availability list that says why
     * anything absent is absent. The only source of an analytic number in this app: a screen that
     * cannot find a value here renders the core's reason, never a locally computed substitute.
     */
    val dailySnapshot: StateFlow<DailySnapshotReport?> = mutableSnapshot.asStateFlow()

    val repo: MavRepo = MavRepo()
    val mlEngine: MavMlSignals = MavMlSignals()
    val connectors = AndroidConnectorManager(application, viewModelScope)

    /** Generic source id used until stored read models carry the active connector identity. */
    val activeDeviceSource: String get() = "active-device"

    companion object {
        /** The device the connector host writes under; one strap at a time until multi-device. */
        const val ACTIVE_DEVICE_ID: ULong = 1uL
    }

    private var refreshJob: Job? = null

    init {
        viewModelScope.launch {
            connectors.connection.collect { connection ->
                mutableLive.value = liveStateOf(connection)
                mutableBpm.value = connection.heartRateBpm
            }
        }
        refresh()
    }

    fun refresh() {
        if (refreshJob?.isActive == true) return
        refreshJob = viewModelScope.launch(Dispatchers.IO) {
            // Keep the last good snapshot on screen during a re-read; Loading only before the first.
            if (mutableState.value != MavAppState.Ready) mutableState.value = MavAppState.Loading
            mutableState.value = runCatching {
                connectors.start()
                mutableSyncNote.value = null
                mutableSnapshot.value =
                    connectors.dailySnapshot(ACTIVE_DEVICE_ID, System.currentTimeMillis())
                MavAppState.Ready
            }.getOrElse(::failureState)
        }
    }

    /** Zone minutes derived from raw HR over [from, to]; null until the core serves HR history. */
    @Suppress("UNUSED_PARAMETER")
    suspend fun workoutZoneMinutes(from: Long, to: Long): List<Double>? = null

    fun loadWorkouts() {
        // No workout source in the core yet; the flow stays empty on purpose.
    }

    /** Starts the local live-session surface. Persistence waits for the frozen workout contract. */
    fun startWorkout(sport: String) {
        if (mutableActiveWorkout.value == null) {
            mutableActiveWorkout.value =
                ActiveWorkout(startMs = System.currentTimeMillis(), sport = sport)
        }
    }

    /** Ends the local live-session surface without inventing a stored workout row. */
    fun stopWorkout() {
        mutableActiveWorkout.value = null
    }

    /**
     * Trade data density for battery on both the phone and the strap (ADR-030). The core keeps the
     * setting across sessions, so a reconnect does not quietly return to full power.
     *
     * The runtime handle lives on `MavRepo.sharedRuntime`, not on this class; reaching for a bare
     * `runtime` here is what left the tree uncompilable after the battery-saver commit.
     */
    fun setLowPower(on: Boolean) {
        MavRepo.sharedRuntime?.setLowPower(on, System.currentTimeMillis())
    }

    /** Wrist haptics — no transport wiring in the host yet, so these are inert. */
    @Suppress("UNUSED_PARAMETER")
    fun buzz(loops: Int = 2) {}

    fun buzzStrapOnce() {}

    fun stopHaptics() {}

    override fun onCleared() {
        connectors.close()
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
 * The core connection block rendered as the live readout. The runtime's link-up states are
 * `subscribing` and `streaming` (there is no `connected` state in `host-snapshot/v1`), and a stored
 * heart rate or battery figure never outlives the link — a disconnected strap shows no live vitals.
 */
internal fun liveStateOf(snapshot: MavSnapshot): LiveState {
    val connected =
        snapshot.connectionState == "subscribing" || snapshot.connectionState == "streaming"
    return LiveState(
        connected = connected,
        bonded = connected,
        heartRate = if (connected) snapshot.currentBpm else null,
        batteryPct = if (connected) snapshot.batteryPercent?.toDouble() else null,
        charging = if (connected) snapshot.charging else null,
        // Assume worn until the strap says otherwise (macOS/Android LiveState parity).
        worn = if (connected) snapshot.onWrist ?: true else true,
        advertisingName = snapshot.deviceName,
        scanning = snapshot.connectionState == "scanning",
        statusNote = snapshot.recoveryUnavailableReason,
    )
}

internal fun liveStateOf(connection: com.sennnen.mav.connector.ConnectorConnectionState): LiveState =
    LiveState(
        connected = connection.connected,
        bonded = connection.connected,
        heartRate = connection.heartRateBpm,
        batteryPct = connection.batteryPercent?.toDouble(),
        worn = connection.onWrist ?: true,
        advertisingName = connection.connectorId,
        scanning = connection.lifecycle == uniffi.mav_ffi.ConnectorLifecycleState.SCANNING,
        statusNote = connection.errorMessage ?: connection.label,
    )

/**
 * Read facade over the core. The history surfaces read whole day ranges through
 * [dailySnapshots]; everything still returning empty is a surface the core has no read model for
 * yet, and returns nothing rather than inventing it.
 */
@Suppress("UNUSED_PARAMETER")
class MavRepo {
    companion object {
        /** Set once by the connector manager when the runtime opens. */
        @Volatile
        @JvmStatic
        var sharedRuntime: MavRuntime? = null
    }

    private val runtime: MavRuntime? get() = sharedRuntime

    suspend fun metricSeries(
        deviceId: String,
        key: String,
        from: String,
        to: String,
    ): List<MetricSeriesRow> = emptyList()

    /**
     * One snapshot per local day in the window, oldest first, straight from the core. Days with no
     * evidence come back too, carrying the reason each analytic is unavailable — a gap in the
     * history is a fact about the recording, not a row to omit.
     */
    suspend fun dailySnapshots(deviceId: String, fromMs: Long, toMs: Long): List<DailySnapshotReport> =
        runtime?.dailySnapshots(deviceId.toULongOrNull() ?: 0uL, fromMs, toMs) ?: emptyList()

    /**
     * The same window in the shape the trend and vitals surfaces read. Only the fields an admitted
     * analytic produces are filled; the rest stay null, which is what makes the empty states honest
     * rather than zeroed.
     */
    suspend fun days(deviceId: String, fromMs: Long, toMs: Long): List<DailyMetric> =
        dailySnapshots(deviceId, fromMs, toMs).map { snapshot ->
            DailyMetric(
                deviceId = deviceId,
                day = snapshot.day,
                restingHr = null,
                avgHrv = snapshot.hrv?.rmssdMs,
                hrvLabel = snapshot.hrv?.label,
            )
        }

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
