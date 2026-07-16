package com.sennnen.mav.ui.aura

import com.sennnen.mav.ui.NoopPrefs

/** Master gate for optional wrist-buzz cues (timer ring etc.). */
object AuraHapticsPrefs {
    private const val KEY = "aura.strapHapticsEnabled"
    fun enabled(context: android.content.Context): Boolean =
        NoopPrefs.of(context).getBoolean(KEY, true)
    fun setEnabled(context: android.content.Context, on: Boolean) {
        NoopPrefs.of(context).edit().putBoolean(KEY, on).apply()
    }
}

/** Whether the user opted into system-health aggregation. Strap telemetry always outranks it. */
object AuraHealthSyncPrefs {
    private const val KEY = "aura.systemHealthSyncEnabled"
    fun enabled(context: android.content.Context): Boolean =
        NoopPrefs.of(context).getBoolean(KEY, false)
    fun setEnabled(context: android.content.Context, on: Boolean) {
        NoopPrefs.of(context).edit().putBoolean(KEY, on).apply()
    }
}
