package com.sennnen.mav.ui.aura

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * The noop:// deep-link router (ANDROID_PARITY_ROADMAP.md C2). MainActivity
 * publishes the requested host here (a process singleton, so it survives the
 * activity re-creation a notification tap causes); the Aura shell collects it
 * and lands on the right hub / flyout, then clears it.
 *
 * Hosts mirror the iOS notification router: `journal` (morning check-in),
 * `recovery` (illness heads-up → vitals), `workouts` (auto-detect "Label it"),
 * `live` (active session).
 */
object AuraDeepLink {
    private val _requested = MutableStateFlow<String?>(null)
    val requested: StateFlow<String?> = _requested.asStateFlow()

    /** Publish a request from an incoming Intent; ignores non-noop schemes. */
    fun offer(intent: Intent?) {
        val uri = intent?.data ?: return
        if (uri.scheme != "noop") return
        _requested.value = uri.host ?: return
    }

    /** The shell consumed the request. */
    fun clear() {
        _requested.value = null
    }

    /** A PendingIntent that opens the app at [host] — used by notification actions. */
    fun pendingIntent(context: Context, host: String, requestCode: Int): PendingIntent =
        PendingIntent.getActivity(
            context,
            requestCode,
            Intent(Intent.ACTION_VIEW, Uri.parse("noop://$host"))
                .setPackage(context.packageName)
                .addFlags(
                    Intent.FLAG_ACTIVITY_NEW_TASK or
                        Intent.FLAG_ACTIVITY_SINGLE_TOP or
                        Intent.FLAG_ACTIVITY_CLEAR_TOP,
                ),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
}
