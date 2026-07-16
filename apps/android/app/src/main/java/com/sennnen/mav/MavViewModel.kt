package com.sennnen.mav

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
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

sealed interface MavAppState {
    data object Loading : MavAppState
    data class Ready(val snapshot: MavSnapshot) : MavAppState
    data class Failed(
        val code: String,
        val message: String,
    ) : MavAppState
}

class MavViewModel(application: Application) : AndroidViewModel(application) {
    private val mutableState = MutableStateFlow<MavAppState>(MavAppState.Loading)
    val state: StateFlow<MavAppState> = mutableState.asStateFlow()
    private var runtime: MavRuntime? = null
    private var refreshJob: Job? = null

    init {
        refresh()
    }

    fun refresh() {
        if (refreshJob?.isActive == true) return
        refreshJob = viewModelScope.launch(Dispatchers.IO) {
            mutableState.value = MavAppState.Loading
            mutableState.value = runCatching {
                val active = runtime ?: openRuntime().also { runtime = it }
                val result = active.hostSnapshot(System.currentTimeMillis())
                MavAppState.Ready(MavSnapshotDecoder.decode(result.json, result.hash))
            }.getOrElse(::failureState)
        }
    }

    override fun onCleared() {
        runtime?.close()
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
