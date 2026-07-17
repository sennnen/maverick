package com.sennnen.mav.ui.aura

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.sennnen.mav.MainActivity
import com.sennnen.mav.ui.MavPrefs
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.util.Locale
import kotlin.math.max
import kotlin.math.min

// General-purpose countdown timer (Android twin of Strand/App/CountdownTimer.swift +
// Strand/UI/AuraTimerView.swift): sauna, cold plunge, cooking, stretching — set a
// duration, the strap buzzes at zero and keeps insisting until acknowledged.
//
// Firing model: the end instant persists across relaunches; a foreground 1 s tick
// drives the on-screen count and, at zero, the RING state — the strap re-buzzes
// every few seconds (capped) until acknowledged. A local notification is posted at
// fire so a backgrounded (still-alive) app lands "time's up"; the strap buzz is the
// primary signal, so no AlarmManager exactness is claimed.

object AuraCountdown {
    var endAtMs by mutableStateOf<Long?>(null)
        private set
    var pausedRemaining by mutableStateOf<Int?>(null)
        private set
    var isRinging by mutableStateOf(false)
        private set

    /** UI tick — bumped every second while running so readouts re-derive [remaining]. */
    var heartbeat by mutableIntStateOf(0)
        private set

    var lastDurationSeconds by mutableIntStateOf(10 * 60)
        private set

    /** One-shot strap buzz — wired by the shell to the BLE client. */
    var buzz: () -> Unit = {}

    /** Best-effort strap haptic clear — wired by the shell. */
    var stopBuzz: () -> Unit = {}

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var tick: Job? = null
    private var ringBuzzCount = 0
    private var appContext: Context? = null
    private var loaded = false

    private const val END_KEY = "countdownTimer.endDate"
    private const val DURATION_KEY = "countdownTimer.lastDuration"
    private const val CHANNEL_ID = "countdown_timer"
    private const val NOTIFICATION_ID = 7207

    /** Re-buzz cadence while ringing + cap (≈30 s of insistence, mirrors iOS). */
    private const val RING_REBUZZ_SECONDS = 4
    private const val RING_MAX_BUZZES = 8

    val isRunning: Boolean get() = endAtMs != null

    /** Seconds to zero for display: live countdown, paused bank, or null when idle. */
    val remaining: Int?
        get() {
            pausedRemaining?.let { return it }
            val end = endAtMs ?: return null
            return max(0L, (end - System.currentTimeMillis() + 500) / 1000).toInt()
        }

    /** Relaunch mid-run: revive a future end instant; a past one is cleared quietly. */
    fun ensureLoaded(context: Context) {
        if (loaded) return
        loaded = true
        appContext = context.applicationContext
        val prefs = MavPrefs.of(context)
        lastDurationSeconds = prefs.getInt(DURATION_KEY, 10 * 60)
        val end = prefs.getLong(END_KEY, 0L)
        if (end > System.currentTimeMillis()) resume(end)
        else if (end != 0L) prefs.edit().remove(END_KEY).apply()
    }

    fun start(seconds: Int) {
        if (seconds <= 0) return
        acknowledge()
        lastDurationSeconds = seconds
        appContext?.let { MavPrefs.of(it).edit().putInt(DURATION_KEY, seconds).apply() }
        pausedRemaining = null
        resume(System.currentTimeMillis() + seconds * 1000L)
    }

    fun pause() {
        val end = endAtMs ?: return
        pausedRemaining = max(1L, (end - System.currentTimeMillis()) / 1000).toInt()
        endAtMs = null
        stopTick()
        persistEnd(null)
    }

    fun resumePaused() {
        val banked = pausedRemaining ?: return
        pausedRemaining = null
        resume(System.currentTimeMillis() + banked * 1000L)
    }

    fun reset() {
        endAtMs = null
        pausedRemaining = null
        stopTick()
        persistEnd(null)
        acknowledge()
    }

    /** Stop the ring: strap haptics cleared, notification dismissed. Safe when idle. */
    fun acknowledge() {
        if (!isRinging) return
        isRinging = false
        ringBuzzCount = 0
        stopBuzz()
        appContext?.let { ctx ->
            runCatching {
                (ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager)
                    .cancel(NOTIFICATION_ID)
            }
        }
    }

    // MARK: Internals

    private fun resume(endMs: Long) {
        endAtMs = endMs
        persistEnd(endMs)
        startTick()
    }

    private fun persistEnd(endMs: Long?) {
        val ctx = appContext ?: return
        val e = MavPrefs.of(ctx).edit()
        if (endMs == null) e.remove(END_KEY) else e.putLong(END_KEY, endMs)
        e.apply()
    }

    private fun startTick() {
        stopTick()
        tick = scope.launch {
            while (isActive) {
                delay(1000)
                onTick()
            }
        }
    }

    private fun stopTick() {
        tick?.cancel()
        tick = null
    }

    private fun onTick() {
        if (isRinging) {
            // Insist until acknowledged, spaced + capped.
            ringBuzzCount += 1
            if (ringBuzzCount % RING_REBUZZ_SECONDS == 0 &&
                ringBuzzCount / RING_REBUZZ_SECONDS < RING_MAX_BUZZES
            ) buzz()
            return
        }
        val end = endAtMs ?: run { stopTick(); return }
        heartbeat += 1
        if (end <= System.currentTimeMillis()) fire()
    }

    private fun fire() {
        endAtMs = null
        persistEnd(null)
        isRinging = true
        ringBuzzCount = 0
        buzz()
        postNotification()
        // Tick keeps running to drive the re-buzz cadence until acknowledged.
    }

    private fun postNotification() {
        val ctx = appContext ?: return
        // Android 13+: POST_NOTIFICATIONS is a runtime permission; without it the ring
        // stays in-app (the sheet + wrist buzz still fire).
        if (android.os.Build.VERSION.SDK_INT >= 33 &&
            ctx.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        runCatching {
            val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "Timer", NotificationManager.IMPORTANCE_HIGH),
            )
            val open = PendingIntent.getActivity(
                ctx, 0, Intent(ctx, MainActivity::class.java),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            val n = android.app.Notification.Builder(ctx, CHANNEL_ID)
                .setSmallIcon(android.R.drawable.ic_lock_idle_alarm)
                .setContentTitle("Timer done")
                .setContentText("Your countdown just finished.")
                .setContentIntent(open)
                .setAutoCancel(true)
                .build()
            nm.notify(NOTIFICATION_ID, n)
        }
    }
}

// MARK: - The Timer sheet

@Composable
fun AuraTimerSheet(onDismiss: () -> Unit, strapBonded: Boolean) {
    val p = Aura.palette
    var hours by rememberSaveable { mutableIntStateOf(min(AuraCountdown.lastDurationSeconds / 3600, 23)) }
    var minutes by rememberSaveable { mutableIntStateOf((AuraCountdown.lastDurationSeconds % 3600) / 60) }
    var seconds by rememberSaveable { mutableIntStateOf(AuraCountdown.lastDurationSeconds % 60) }

    // 500 ms UI clock keeps the big number moving while running.
    var now by remember { mutableStateOf(0L) }
    LaunchedEffect(AuraCountdown.isRunning, AuraCountdown.isRinging) {
        while (true) {
            now = System.currentTimeMillis()
            delay(500)
        }
    }

    val pickedSeconds = hours * 3600 + minutes * 60 + seconds
    val remaining = AuraCountdown.remaining
    val running = AuraCountdown.isRunning
    val paused = AuraCountdown.pausedRemaining != null
    val ringing = AuraCountdown.isRinging

    AuraSheet(title = "Timer", onDismiss = onDismiss, family = AuraFamily.ENERGY) {
        // MARK: Hero — ring + big time
        AuraGlowTile(AuraFamily.ENERGY, padding = 22.dp, radius = 34.dp) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text("Countdown", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                Spacer(Modifier.weight(1f))
                when {
                    ringing -> AuraStatusChip("Time's up", AuraChipKind.NEGATIVE, pulsing = true)
                    running -> AuraStatusChip("Running", AuraChipKind.POSITIVE, pulsing = true)
                    paused -> AuraStatusChip("Paused", AuraChipKind.CAUTION)
                }
            }
            Spacer(Modifier.size(18.dp))
            @Suppress("UNUSED_EXPRESSION") now  // re-derive the readout each clock tick
            val shown = if (ringing) 0 else remaining ?: pickedSeconds
            val display =
                if (shown >= 3600) String.format(Locale.US, "%d:%02d:%02d", shown / 3600, (shown % 3600) / 60, shown % 60)
                else String.format(Locale.US, "%02d:%02d", shown / 60, shown % 60)
            if (running || paused || ringing) {
                val progress =
                    if (AuraCountdown.lastDurationSeconds > 0 && remaining != null)
                        1f - remaining.toFloat() / AuraCountdown.lastDurationSeconds
                    else if (ringing) 1f else 0f
                Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(
                        progress = { progress.coerceIn(0f, 1f) },
                        modifier = Modifier.size(230.dp),
                        color = AuraFamily.ENERGY.glow,
                        trackColor = MaterialTheme.colorScheme.surfaceVariant,
                        strokeWidth = 10.dp,
                        strokeCap = StrokeCap.Round,
                    )
                    Text(
                        display, style = AuraType.mega(if (shown >= 3600) 44.sp else 56.sp),
                        color = p.ink, maxLines = 1,
                    )
                }
            } else {
                Text(
                    display, style = AuraType.mega(72.sp), color = p.ink, maxLines = 1,
                    modifier = Modifier.fillMaxWidth(), textAlign = TextAlign.Center,
                )
                Spacer(Modifier.size(12.dp))
                Text(
                    "Buzzes your wrist when it hits zero. Double-tap the band to stop it.",
                    style = AuraType.sub, color = p.ink.copy(alpha = 0.7f),
                )
            }
        }

        // MARK: Idle → picker + start · Running → controls · Ringing → stop
        when {
            ringing -> {
                Column(verticalArrangement = Arrangement.spacedBy(14.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Button(
                        onClick = { AuraCountdown.acknowledge() },
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = p.bad, contentColor = Color.White,
                        ),
                        contentPadding = androidx.compose.foundation.layout.PaddingValues(vertical = 16.dp),
                    ) { Text("Stop", style = AuraType.label) }
                    Text(
                        "Or double-tap your band.",
                        style = AuraType.caption, color = p.ink.copy(alpha = 0.55f),
                    )
                }
            }
            running || paused -> {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    if (paused) {
                        Button(
                            onClick = { AuraCountdown.resumePaused() },
                            modifier = Modifier.weight(1f),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = p.accent, contentColor = Color.Black,
                            ),
                            contentPadding = androidx.compose.foundation.layout.PaddingValues(vertical = 14.dp),
                        ) { Text("Resume", style = AuraType.label) }
                    } else {
                        FilledTonalButton(
                            onClick = { AuraCountdown.pause() },
                            modifier = Modifier.weight(1f),
                            contentPadding = androidx.compose.foundation.layout.PaddingValues(vertical = 14.dp),
                        ) { Text("Pause", style = AuraType.label) }
                    }
                    FilledTonalButton(
                        onClick = { AuraCountdown.reset() },
                        modifier = Modifier.weight(1f),
                        contentPadding = androidx.compose.foundation.layout.PaddingValues(vertical = 14.dp),
                    ) { Text("Reset", style = AuraType.label) }
                }
            }
            else -> {
                AuraDarkCard(padding = 18.dp) {
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        listOf(1, 2, 5, 10, 15, 30).forEach { m ->
                            val active = hours == 0 && minutes == m && seconds == 0
                            FilterChip(
                                selected = active,
                                onClick = { hours = 0; minutes = m; seconds = 0 },
                                label = {
                                    Text(
                                        "${m}m", style = AuraType.caption, maxLines = 1, softWrap = false,
                                        modifier = Modifier.fillMaxWidth(), textAlign = TextAlign.Center,
                                    )
                                },
                                modifier = Modifier.weight(1f),
                                colors = FilterChipDefaults.filterChipColors(
                                    containerColor = p.ink.copy(alpha = 0.08f),
                                    labelColor = p.ink.copy(alpha = 0.7f),
                                    selectedContainerColor = p.accent,
                                    selectedLabelColor = Color.Black,
                                ),
                            )
                        }
                    }
                    Spacer(Modifier.size(16.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        DurationColumn("hours", hours, 23, { hours = it }, Modifier.weight(1f))
                        DurationColumn("min", minutes, 59, { minutes = it }, Modifier.weight(1f))
                        DurationColumn("sec", seconds, 59, { seconds = it }, Modifier.weight(1f))
                    }
                }
                Button(
                    onClick = { if (pickedSeconds > 0) AuraCountdown.start(pickedSeconds) },
                    enabled = pickedSeconds > 0,
                    modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = p.accent, contentColor = Color.Black,
                        disabledContainerColor = p.ink.copy(alpha = 0.08f),
                        disabledContentColor = p.ink.copy(alpha = 0.4f),
                    ),
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(vertical = 16.dp),
                ) { Text("Start", style = AuraType.label) }
            }
        }

        if (!strapBonded && !ringing) {
            Text(
                "No strap connected. The timer still counts here and notifies on this device; " +
                    "the wrist buzz joins when the strap connects.",
                style = AuraType.caption, color = p.ink.copy(alpha = 0.45f),
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
    }
}

/** One duration unit: big value + stepper (M3 IconButtons, the strength-editor idiom). */
@Composable
private fun DurationColumn(
    label: String,
    value: Int,
    maxValue: Int,
    onChange: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val p = Aura.palette
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(
            String.format(Locale.US, "%02d", value),
            style = AuraType.number(34.sp), color = p.ink,
        )
        Text(label, style = AuraType.caption, color = p.ink.copy(alpha = 0.55f))
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = { onChange(if (value <= 0) maxValue else value - 1) }, modifier = Modifier.size(38.dp)) {
                Icon(Icons.Filled.Remove, contentDescription = "Decrease $label", tint = p.ink.copy(alpha = 0.8f), modifier = Modifier.size(18.dp))
            }
            IconButton(onClick = { onChange(if (value >= maxValue) 0 else value + 1) }, modifier = Modifier.size(38.dp)) {
                Icon(Icons.Filled.Add, contentDescription = "Increase $label", tint = p.ink.copy(alpha = 0.8f), modifier = Modifier.size(18.dp))
            }
        }
    }
}

