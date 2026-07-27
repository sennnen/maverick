package com.sennnen.mav.ui.aura

import com.sennnen.mav.ui.MavPrefs

/** Master gate for optional wrist-buzz cues (timer ring etc.). */
object AuraHapticsPrefs {
    private const val KEY = "aura.strapHapticsEnabled"
    fun enabled(context: android.content.Context): Boolean =
        MavPrefs.of(context).getBoolean(KEY, true)
    fun setEnabled(context: android.content.Context, on: Boolean) {
        MavPrefs.of(context).edit().putBoolean(KEY, on).apply()
    }
}

/** Whether the user opted into system-health aggregation. Strap telemetry always outranks it. */
/** Battery saver. Persisted so the choice survives a restart and is re-applied to every session. */
object AuraLowPowerPrefs {
    private const val KEY = "aura.lowPowerEnabled"
    fun enabled(context: android.content.Context): Boolean =
        MavPrefs.of(context).getBoolean(KEY, false)
    fun setEnabled(context: android.content.Context, on: Boolean) {
        MavPrefs.of(context).edit().putBoolean(KEY, on).apply()
    }
}

object AuraHealthSyncPrefs {
    private const val KEY = "aura.systemHealthSyncEnabled"
    fun enabled(context: android.content.Context): Boolean =
        MavPrefs.of(context).getBoolean(KEY, false)
    fun setEnabled(context: android.content.Context, on: Boolean) {
        MavPrefs.of(context).edit().putBoolean(KEY, on).apply()
    }
}
