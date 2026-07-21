package com.sennnen.mav.ui.aura

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.TrendingUp
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.Timer
import androidx.compose.material.icons.filled.WbTwilight
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.analytics.RestScorer
import com.sennnen.mav.ui.AppViewModel
import java.time.LocalDate
import java.time.LocalTime
import java.time.format.DateTimeFormatter
import kotlin.math.roundToInt

// Today — the overview home (Android port of Strand/UI/AuraTodayView.swift):
// three pillar rings with status colour + a plain-language day insight + live
// HR at a glance; then the morning-Journal nudge, the Coach entry, the vitals
// grid, and Reports + Trends links. Tapping a ring jumps to that pillar's hub.

@Composable
fun AuraTodayScreen(
    vm: AppViewModel,
    onOpenLive: () -> Unit,
    onOpenJournal: () -> Unit,
    onOpenCoach: () -> Unit,
    onOpenReports: () -> Unit,
    onOpenTrends: () -> Unit,
) {
    val p = Aura.palette
    val switchTab = LocalAuraSwitchTab.current
    val days by vm.recentDays.collectAsStateWithLifecycle()
    val live by vm.live.collectAsStateWithLifecycle()
    val altBpm by vm.bpm.collectAsStateWithLifecycle()

    val anchor = auraAnchorDay(days)
    val vitalsDay = auraLastVitalsDay(days, anchor)

    // Rest = the imported sleep_performance for the anchor day when present,
    // else the on-device composite (RestScorer) — mirrors the widget contract.
    var restPct by remember { mutableStateOf<Double?>(null) }
    LaunchedEffect(days) {
        val series = runCatching {
            vm.repo.metricSeries(vm.activeDeviceSource, "sleep_performance", "0000-00-00", "9999-99-99")
        }.getOrDefault(emptyList())
        val byDay = series.associate { it.day to it.value }
        restPct = anchor?.let { a ->
            byDay[a.day]
                ?: (if (a.day == LocalDate.now().toString()) series.lastOrNull()?.value else null)
                ?: RestScorer.restFromDaily(a)
        }
    }

    var revealed by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { revealed = true }
    var editing by remember { mutableStateOf(false) }
    var showTimer by remember { mutableStateOf(false) }
    val (hiddenCSV, setHiddenCSV) = rememberHubHiddenCards("today")

    val charge = anchor?.recovery
    val effort = anchor?.strain
    val chargeStatus = AuraStatus.recovery(charge)
    val restStatus = AuraStatus.sleep(restPct)
    val bpm = altBpm ?: live.heartRate

    AuraScreen(lead = AuraFamily.CHARGE) {
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .statusBarsPadding()
                .padding(horizontal = Aura.screenMargin)
                .padding(top = 8.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(Aura.sectionGap),
        ) {
            AuraHubHeader(
                title = greeting(),
                subtitle = LocalDate.now().format(DateTimeFormatter.ofPattern("EEEE, MMMM d")),
                editing = editing,
                onToggleEditing = { editing = !editing },
            )

            // MARK: Hero — the three pillar rings + day insight
            Column(
                Modifier
                    .fillMaxWidth()
                    .auraReveal(revealed, 1)
                    .background(p.card, RoundedCornerShape(Aura.cardRadius))
                    .border(1.dp, p.hairline, RoundedCornerShape(Aura.cardRadius))
                    .padding(vertical = 22.dp, horizontal = 18.dp),
                verticalArrangement = Arrangement.spacedBy(22.dp),
            ) {
                Row(Modifier.fillMaxWidth()) {
                    PillarRing(
                        label = "Charge",
                        text = charge?.roundToInt()?.toString() ?: "--",
                        value = charge, status = chargeStatus,
                        onTap = { switchTab(AuraTab.RECOVERY) },
                        modifier = Modifier.weight(1f),
                    )
                    PillarRing(
                        label = "Effort",
                        text = AuraEffort.text(effort),
                        value = effort,
                        status = if (effort == null) AuraStatus.NONE else AuraStatus.GOOD,
                        tint = AuraFamily.EFFORT.glow,
                        onTap = { switchTab(AuraTab.STRAIN) },
                        modifier = Modifier.weight(1f),
                    )
                    PillarRing(
                        label = "Rest",
                        text = restPct?.roundToInt()?.toString() ?: "--",
                        value = restPct, status = restStatus,
                        onTap = { switchTab(AuraTab.SLEEP) },
                        modifier = Modifier.weight(1f),
                    )
                }
                Text(
                    insight(chargeStatus, restStatus),
                    style = AuraType.sub, color = p.ink.copy(alpha = 0.78f),
                )
            }

            AuraLiveHRPill(
                bpm = bpm,
                deviceName = live.advertisingName ?: "Wearable",
                batteryPercent = live.batteryPct?.roundToInt(),
                bonded = live.bonded,
                onClick = onOpenLive,
            )

            AuraEditableCard("journal", hiddenCSV, setHiddenCSV, editing) {
                NudgeCard(
                    icon = Icons.Filled.WbTwilight,
                    iconTint = AuraFamily.ENERGY.glow,
                    title = "Morning journal",
                    body = "Log last night's behaviours to sharpen your recovery insights.",
                    onClick = onOpenJournal,
                )
            }

            AuraEditableCard("vitals", hiddenCSV, setHiddenCSV, editing) {
                Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    AuraSectionHeader(title = "Vitals")
                    val hrv = anchor?.avgHrv ?: vitalsDay?.avgHrv
                    val rhr = anchor?.restingHr ?: vitalsDay?.restingHr
                    val resp = anchor?.respRateBpm ?: vitalsDay?.respRateBpm
                    val spo2 = anchor?.spo2Pct ?: vitalsDay?.spo2Pct
                    val temp = anchor?.skinTempDevC ?: vitalsDay?.skinTempDevC
                    // Levels are 0 when the vital is absent — never a fabricated bar.
                    AuraDarkCard(padding = 20.dp) {
                        VitalsGridRow(
                            AuraMini(auraIntText(hrv), "ms", "HRV", (hrv ?: 0.0) / 140, AuraFamily.CHARGE.glow),
                            AuraMini(rhr?.toString() ?: "--", "bpm", "Resting HR", rhr?.let { 1 - it / 100.0 } ?: 0.0, AuraFamily.HEART.glow),
                        )
                        Spacer(Modifier.padding(top = 22.dp))
                        VitalsGridRow(
                            AuraMini(auraDecText(resp, 1), "rpm", "Respiratory", (resp ?: 0.0) / 25, AuraFamily.VITALS.glow),
                            AuraMini(auraIntText(spo2), "%", "Blood O₂", (spo2 ?: 0.0) / 100, AuraFamily.VITALS.glow),
                        )
                        Spacer(Modifier.padding(top = 22.dp))
                        VitalsGridRow(
                            AuraMini(auraSignedText(temp), "°C", "Skin Temp", temp?.let { 0.5 + it / 4 } ?: 0.0, AuraFamily.HEART.glow),
                            AuraMini(auraHmText(anchor?.totalSleepMin), "", "Slept", (anchor?.totalSleepMin ?: 0.0) / 540, AuraFamily.REST.glow),
                        )
                    }
                }
            }

            AuraEditableCard("coach", hiddenCSV, setHiddenCSV, editing) {
                NudgeCard(
                    icon = Icons.Filled.AutoAwesome,
                    iconTint = AuraFamily.EFFORT.glow,
                    title = "Coach",
                    body = "Ask anything about your data. Private: your key, your device.",
                    onClick = onOpenCoach,
                )
            }

            AuraEditableCard("links", hiddenCSV, setHiddenCSV, editing) {
                Row(horizontalArrangement = Arrangement.spacedBy(Aura.cardSpacing)) {
                    LinkTile(
                        "Reports", "Week · month", Icons.Filled.Description,
                        onClick = onOpenReports, modifier = Modifier.weight(1f),
                    )
                    LinkTile(
                        "Trends", "1w · 1m · 6m", Icons.AutoMirrored.Filled.TrendingUp,
                        onClick = onOpenTrends, modifier = Modifier.weight(1f),
                    )
                    LinkTile(
                        "Timer", timerSub(), Icons.Filled.Timer,
                        onClick = { showTimer = true }, modifier = Modifier.weight(1f),
                    )
                }
            }
        }
    }

    if (showTimer) {
        AuraTimerSheet(onDismiss = { showTimer = false }, strapBonded = live.bonded)
    }
}

/** Live countdown readout on the tile while the timer runs, so it's glanceable
 *  without opening the sheet (twin of iOS AuraTodayView.timerSub). */
@Composable
private fun timerSub(): String {
    @Suppress("UNUSED_EXPRESSION") AuraCountdown.heartbeat   // re-derive every engine tick
    if (AuraCountdown.isRinging) return "Time's up"
    val r = AuraCountdown.remaining ?: return "Wrist buzz"
    return String.format(java.util.Locale.US, "%d:%02d left", r / 60, r % 60)
}

private fun greeting(): String = when (LocalTime.now().hour) {
    in 5..11 -> "Good morning"
    in 12..16 -> "Good afternoon"
    in 17..21 -> "Good evening"
    else -> "Good night"
}

private fun insight(charge: AuraStatus, rest: AuraStatus): String = when {
    charge == AuraStatus.GOOD && rest == AuraStatus.GOOD ->
        "Recovered and rested. Today can take whatever you want to give it."
    charge == AuraStatus.GOOD -> "Your body recharged well even if sleep fell short. Green light, gently."
    charge == AuraStatus.FAIR -> "A middling recharge. Train, but keep something in reserve."
    charge == AuraStatus.LOW -> "Recovery is low. Today is for easy movement and an early night."
    else -> "Wear your strap tonight and tomorrow starts with a score."
}

@Composable
private fun PillarRing(
    label: String,
    text: String,
    value: Double?,
    status: AuraStatus,
    onTap: () -> Unit,
    modifier: Modifier = Modifier,
    tint: Color? = null,
) {
    Column(
        modifier.auraPressable(onClick = onTap),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        AuraScoreRing(
            value = value, text = text, label = label, status = status,
            maxValue = 100.0, tintOverride = tint, size = 100.dp, lineWidth = 6.dp,
        )
    }
}

private data class AuraMini(
    val value: String, val unit: String, val label: String, val level: Double, val tint: Color,
)

@Composable
private fun VitalsGridRow(a: AuraMini, b: AuraMini) {
    Row(horizontalArrangement = Arrangement.spacedBy(22.dp)) {
        AuraMiniStat(a.value, a.label, a.level, a.tint, unit = a.unit, modifier = Modifier.weight(1f))
        AuraMiniStat(b.value, b.label, b.level, b.tint, unit = b.unit, modifier = Modifier.weight(1f))
    }
}

/** A tappable dark row card with an icon, title, body and chevron (journal / coach). */
@Composable
private fun NudgeCard(
    icon: ImageVector,
    iconTint: Color,
    title: String,
    body: String,
    onClick: () -> Unit,
) {
    val p = Aura.palette
    AuraDarkCard(onClick = onClick) {
        Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, contentDescription = null, tint = iconTint, modifier = Modifier.width(28.dp).size(20.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(title, style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                Text(body, style = AuraType.caption, color = p.ink.copy(alpha = 0.6f))
            }
            Icon(
                Icons.Filled.ChevronRight, contentDescription = null,
                tint = p.ink.copy(alpha = 0.35f), modifier = Modifier.size(16.dp),
            )
        }
    }
}

@Composable
private fun LinkTile(
    title: String,
    sub: String,
    icon: ImageVector,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val p = Aura.palette
    AuraDarkCard(modifier = modifier, onClick = onClick) {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Icon(icon, contentDescription = null, tint = p.accentInk, modifier = Modifier.size(20.dp))
            Text(title, style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
            Text(sub, style = AuraType.caption, color = p.ink.copy(alpha = 0.55f))
        }
    }
}
