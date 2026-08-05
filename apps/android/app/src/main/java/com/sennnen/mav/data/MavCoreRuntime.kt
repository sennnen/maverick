package com.sennnen.mav.data

import android.content.Context
import com.sennnen.mav.BuildConfig
import uniffi.mav_ffi.MavRuntime
import uniffi.mav_ffi.RuntimeConfig
import uniffi.mav_ffi.TimezoneSpan
import java.io.File
import java.util.TimeZone

/**
 * The one open core, for the whole process.
 *
 * There is exactly one SQLite file and there must be exactly one `MavRuntime` over it. Before this
 * existed the connector manager built it and published the handle onto two static fields, which
 * was fine while the only way into the app was the activity — and stopped being fine the moment a
 * `WorkManager` worker could start the process on its own. A worker waking a cold process found
 * both fields null and quietly did nothing, so the background analytics windows the app asks the
 * OS for were spent returning success without running a model.
 *
 * So opening moved here. It is idempotent, it is synchronised, and both callers go through it,
 * which also removes the way a second `MavRuntime` could have been opened over a database the
 * first one still held.
 *
 * What is *not* here is anything a particular caller wants done after opening — installing the
 * bundled connector, restoring caches. Those stay with the caller that needs them.
 */
object MavCoreRuntime {
    @Volatile
    private var runtime: MavRuntime? = null

    /** The open core, or null when nothing has opened it yet. */
    fun opened(): MavRuntime? = runtime

    /**
     * Open the core, or return the one already open.
     *
     * [onFirstOpen] runs once, inside the lock, only for the caller that actually opened it. It is
     * where a caller puts the work that must not happen twice — installing the bundled connector
     * is the one that exists.
     */
    @Synchronized
    fun open(context: Context, onFirstOpen: (MavRuntime) -> Unit = {}): MavRuntime {
        runtime?.let { return it }
        val application = context.applicationContext
        val database = File(application.noBackupFilesDir, DATABASE)
        val zone = TimeZone.getDefault()
        val opened = MavRuntime(
            RuntimeConfig(
                databasePath = database.absolutePath,
                timezoneId = zone.id,
                appVersion = "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})",
            ),
        )
        opened.setTimezoneSpans(zone.id, offsetSpans(zone))
        runtime = opened
        onFirstOpen(opened)
        return opened
    }

    /**
     * Every offset change in the window a recompute can reach.
     *
     * Two years back and one forward: the store holds no older evidence, and a day boundary a
     * year out is as far ahead as any scheduled window lands. Sampled daily rather than read from
     * the zone's transition table, which the JDK does not expose portably.
     */
    fun offsetSpans(zone: TimeZone): List<TimezoneSpan> {
        val day = 86_400L
        val now = System.currentTimeMillis() / 1000L
        var cursor = now - 730 * day
        val end = now + 365 * day
        val spans = mutableListOf<TimezoneSpan>()
        var last: Int? = null
        while (cursor <= end) {
            val offset = zone.getOffset(cursor * 1000L) / 1000
            if (offset != last) {
                spans.add(TimezoneSpan(startUnixSeconds = cursor, offsetSeconds = offset))
                last = offset
            }
            cursor += day
        }
        if (spans.isEmpty()) {
            spans.add(TimezoneSpan(startUnixSeconds = 0L, offsetSeconds = zone.rawOffset / 1000))
        }
        return spans
    }

    private const val DATABASE = "mav.sqlite"
}
