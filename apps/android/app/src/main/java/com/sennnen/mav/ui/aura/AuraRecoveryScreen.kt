package com.sennnen.mav.ui.aura

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.MonitorHeart
import androidx.compose.material.icons.outlined.Circle
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.draw.scale
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.analytics.CyclePhaseEngine
import com.sennnen.mav.analytics.IllnessSignalEngine
import com.sennnen.mav.data.DailyMetric
import com.sennnen.mav.ui.AppViewModel
import java.util.Locale
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

// Recovery hub (Android port of Strand/UI/AuraRecoveryView.swift) — the Charge
// deep-dive AND the Health Monitor: recovery score with contributors, the five
// vitals with status vs baseline (tap → full interactive history), and
// illness / cycle signals.

private data class AuraVital(
    val id: String,
    val label: String,
    val family: AuraFamily,
    val value: Double?,
    val baseline: Double?,
    val display: (Double) -> String,
    val unit: String,
    val decimals: Int,
    val status: AuraStatus,
    val points: List<AuraPoint>,
    val caption: String,
    val level: Double,
)

@Composable
fun AuraRecoveryScreen(vm: AppViewModel) {
    val p = Aura.palette
    val days by vm.recentDays.collectAsStateWithLifecycle()
    val signals by vm.v5Signals.collectAsStateWithLifecycle()
    val recoveryReason = "Recovery is unavailable until the core admits a stored read model."

    val anchor = auraAnchorDay(days)
    val vitalsDay = auraLastVitalsDay(days, anchor)

    var revealed by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { revealed = true }
    var editing by remember { mutableStateOf(false) }
    val (hiddenCSV, setHiddenCSV) = rememberHubHiddenCards("recovery")
    var selected by remember { mutableStateOf<AuraDetailData?>(null) }

    val recovery = anchor?.recovery
    val status = AuraStatus.recovery(recovery)
    val vitals = buildVitals(days, anchor, vitalsDay)
    val recoveryPoints = auraPoints(days) { it.recovery }

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
                title = "Recovery",
                subtitle = "How ready your body is today",
                editing = editing,
                onToggleEditing = { editing = !editing },
            )

            // MARK: Hero
            AuraGlowTile(
                AuraFamily.CHARGE,
                modifier = Modifier.auraReveal(revealed, 1),
                padding = 22.dp, radius = 34.dp,
                onClick = {
                    selected = AuraDetailData(
                        family = AuraFamily.CHARGE, title = "Charge",
                        value = recovery, unit = "%",
                        baseline = auraBaselineOf(recoveryPoints),
                        status = status,
                        caption = "How recovered you are, led by overnight HRV against your own baseline.",
                        points = recoveryPoints,
                        heroFraction = (recovery ?: 0.0) / 100,
                        contributors = vitals.map {
                            AuraDetailData.Contributor(
                                label = it.label,
                                value = it.value?.let(it.display) ?: "--",
                                level = it.level,
                                tint = it.family.glow(p.dark),
                            )
                        },
                    )
                },
            ) {
                Column(
                    Modifier.heightIn(min = 280.dp),
                    verticalArrangement = Arrangement.spacedBy(18.dp),
                ) {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Text("Charge", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                        Spacer(Modifier.weight(1f))
                        // A low score pulses — the one state that should catch the eye.
                        AuraStatusChip(
                            text = statusLine(status), kind = status.chipKind,
                            pulsing = status == AuraStatus.LOW,
                        )
                    }
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                        AuraScoreRing(
                            value = recovery,
                            text = recovery?.roundToInt()?.toString() ?: "--",
                            unit = "%", label = "recovered", status = status,
                        )
                    }
                    Text(insight(status, recoveryReason), style = AuraType.sub, color = p.ink.copy(alpha = 0.8f))
                }
            }

            // MARK: Health Monitor
            AuraEditableCard("monitor", hiddenCSV, setHiddenCSV, editing) {
                Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    AuraSectionHeader(title = "Health Monitor")
                    AuraDarkCard(padding = 0.dp) {
                        Spacer(Modifier.padding(top = 4.dp))
                        vitals.forEachIndexed { i, v ->
                            VitalRow(v) {
                                selected = AuraDetailData(
                                    family = v.family, title = v.label, value = v.value,
                                    unit = v.unit, decimals = v.decimals, baseline = v.baseline,
                                    status = v.status, caption = v.caption, points = v.points,
                                    provenance = "Measured overnight by your strap; baseline is your own trailing 21 days.",
                                )
                            }
                            if (i < vitals.size - 1) {
                                HorizontalDivider(
                                    color = p.hairline, thickness = 1.dp,
                                    modifier = Modifier.padding(start = 18.dp),
                                )
                            }
                        }
                        Spacer(Modifier.padding(top = 4.dp))
                    }
                }
            }

            // MARK: Signals
            val illness = signals?.illness
            val cycle = signals?.cycle
            val showIllness = illness != null && illness.level != IllnessSignalEngine.Level.QUIET
            val showCycle = cycle != null &&
                cycle.phase != CyclePhaseEngine.Phase.UNKNOWN &&
                cycle.phase != CyclePhaseEngine.Phase.LEARNING
            if (showIllness || showCycle) {
                AuraEditableCard("signals", hiddenCSV, setHiddenCSV, editing) {
                    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                        AuraSectionHeader(title = "Signals")
                        Column(verticalArrangement = Arrangement.spacedBy(Aura.cardSpacing)) {
                            if (showIllness) IllnessCard(illness!!)
                            if (showCycle) CycleCard(cycle!!)
                        }
                    }
                }
            }

            // MARK: On-device ML signals (StrandML twin of iOS AuraMLSignalsCard)
            AuraEditableCard("mlsignals", hiddenCSV, setHiddenCSV, editing) {
                AuraMlSignalsCard(vm)
            }

            // MARK: Trend
            AuraEditableCard("trend", hiddenCSV, setHiddenCSV, editing) {
                Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    AuraSectionHeader(title = "Last month")
                    AuraDarkCard {
                        AuraGraph(
                            points = recoveryPoints.takeLast(30),
                            tint = AuraFamily.CHARGE.glow,
                            unit = "%", style = AuraGraphStyle.BARS,
                        )
                    }
                }
            }
        }
    }

    selected?.let { AuraMetricDetailSheet(data = it, onDismiss = { selected = null }) }
}

private fun statusLine(status: AuraStatus): String = when (status) {
    AuraStatus.GOOD -> "Recovered"
    AuraStatus.FAIR -> "Adequate"
    AuraStatus.LOW -> "Run down"
    AuraStatus.NONE -> "No data"
}

private fun insight(status: AuraStatus, unavailableReason: String?): String = when (status) {
    AuraStatus.GOOD -> "Your body absorbed yesterday's load. A big day is on the table."
    AuraStatus.FAIR -> "Partial recharge. Train, but leave something in reserve."
    AuraStatus.LOW -> "Your body is asking for rest. Keep intensity low today."
    // The core's structured reason, when it gave one — never a platform-invented explanation.
    AuraStatus.NONE -> unavailableReason ?: "No recovery data yet."
}

private fun buildVitals(
    days: List<DailyMetric>,
    anchor: DailyMetric?,
    vitalsDay: DailyMetric?,
): List<AuraVital> {
    val hrvPts = auraPoints(days) { it.avgHrv }
    val rhrPts = auraPoints(days) { it.restingHr?.toDouble() }
    val spo2Pts = auraPoints(days) { it.spo2Pct }
    val tempPts = auraPoints(days) { it.skinTempDevC }
    val respPts = auraPoints(days) { it.respRateBpm }

    val hrv = anchor?.avgHrv ?: vitalsDay?.avgHrv
    val rhr = (anchor?.restingHr ?: vitalsDay?.restingHr)?.toDouble()
    val spo2 = anchor?.spo2Pct ?: vitalsDay?.spo2Pct
    val temp = anchor?.skinTempDevC ?: vitalsDay?.skinTempDevC
    val resp = anchor?.respRateBpm ?: vitalsDay?.respRateBpm
    val hrvB = auraBaselineOf(hrvPts)
    val rhrB = auraBaselineOf(rhrPts)
    val respB = auraBaselineOf(respPts)

    fun intDisplay(v: Double): String = v.roundToInt().toString()
    fun decDisplay(v: Double): String = String.format(Locale.US, "%.1f", v)

    return listOf(
        AuraVital(
            "hrv", "HRV", AuraFamily.CHARGE, hrv, hrvB, ::intDisplay, "ms", 0,
            // A DROP below baseline is the warning direction.
            if (hrv == null) AuraStatus.NONE
            else AuraStatus.deviation(auraFrac(hrv, hrvB)?.let { min(it, 0.0) }, tolerance = 0.12),
            hrvPts,
            "Beat-to-beat variability while you sleep, recovery's leading input. Higher than your baseline is good.",
            (hrv ?: 0.0) / 140,
        ),
        AuraVital(
            "rhr", "Resting HR", AuraFamily.HEART, rhr, rhrB, ::intDisplay, "bpm", 0,
            // A RISE above baseline is the warning direction.
            if (rhr == null) AuraStatus.NONE
            else AuraStatus.deviation(auraFrac(rhr, rhrB)?.let { max(it, 0.0) }, tolerance = 0.08),
            rhrPts,
            "Your lowest sustained overnight heart-rate. Lower than your baseline is good.",
            rhr?.let { 1 - it / 100 } ?: 0.0,
        ),
        AuraVital(
            "spo2", "Blood O₂", AuraFamily.VITALS, spo2, auraBaselineOf(spo2Pts), ::intDisplay, "%", 0,
            when {
                spo2 == null -> AuraStatus.NONE
                spo2 >= 95 -> AuraStatus.GOOD
                spo2 >= 92 -> AuraStatus.FAIR
                else -> AuraStatus.LOW
            },
            spo2Pts,
            "Mean blood-oxygen saturation during sleep. 95%+ is typical.",
            (spo2 ?: 0.0) / 100,
        ),
        AuraVital(
            "temp", "Skin Temp", AuraFamily.HEART, temp, null,
            { v -> if (v > 0) "+${decDisplay(v)}" else decDisplay(v) }, "°C", 1,
            when {
                temp == null -> AuraStatus.NONE
                abs(temp) <= 0.4 -> AuraStatus.GOOD
                abs(temp) <= 0.8 -> AuraStatus.FAIR
                else -> AuraStatus.LOW
            },
            tempPts,
            "Deviation from your own overnight skin-temperature baseline. Spikes often precede illness.",
            temp?.let { 0.5 + it / 4 } ?: 0.0,
        ),
        AuraVital(
            "resp", "Respiratory", AuraFamily.VITALS, resp, respB, ::decDisplay, "rpm", 1,
            if (resp == null) AuraStatus.NONE
            else AuraStatus.deviation(auraFrac(resp, respB)?.let { max(it, 0.0) }, tolerance = 0.08),
            respPts,
            "Breaths per minute during sleep. Steady for you is healthy.",
            (resp ?: 0.0) / 25,
        ),
    )
}

@Composable
private fun VitalRow(v: AuraVital, onClick: () -> Unit) {
    val p = Aura.palette
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 18.dp, vertical = 13.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Off-baseline vitals pulse their status dot; in-range ones sit still.
        val pulse = rememberInfiniteTransition(label = "vitalPulse")
        val dotScale by if (v.status == AuraStatus.LOW || v.status == AuraStatus.FAIR) {
            pulse.animateFloat(
                initialValue = 1f, targetValue = 1.35f,
                animationSpec = infiniteRepeatable(tween(900), RepeatMode.Reverse),
                label = "vitalPulseScale",
            )
        } else remember { mutableStateOf(1f) }
        Box(Modifier.size(8.dp).scale(dotScale).background(v.status.color, CircleShape))
        Text(v.label, style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
        Spacer(Modifier.weight(1f))
        // A fortnight's shape inline, so drift is visible without opening the detail.
        AuraSparkline(
            values = v.points.takeLast(14).map { it.value },
            tint = v.family.glow(p.dark),
        )
        Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(
                    v.value?.let(v.display) ?: "--",
                    style = AuraType.number(22.sp), color = p.ink,
                )
                Text(
                    v.unit, style = AuraType.caption, color = p.ink.copy(alpha = 0.55f),
                    modifier = Modifier.padding(bottom = 3.dp),
                )
            }
            v.baseline?.let { b ->
                Text(
                    "baseline ${if (v.decimals == 0) b.roundToInt().toString() else String.format(Locale.US, "%.1f", b)}",
                    style = AuraType.caption, color = p.ink.copy(alpha = 0.45f),
                )
            }
        }
        Icon(
            Icons.Filled.ChevronRight, contentDescription = null,
            tint = p.ink.copy(alpha = 0.35f), modifier = Modifier.size(16.dp),
        )
    }
}

@Composable
private fun IllnessCard(r: IllnessSignalEngine.Result) {
    val p = Aura.palette
    val (title, body, kind) = when (r.level) {
        IllnessSignalEngine.Level.RAISED -> Triple(
            "Heads up", "Several vitals are running away from baseline. Consider taking it easy.",
            AuraChipKind.NEGATIVE,
        )
        IllnessSignalEngine.Level.MILD -> Triple(
            "Watching", "Some vitals are slightly off baseline. Nothing conclusive yet.",
            AuraChipKind.CAUTION,
        )
        IllnessSignalEngine.Level.SUPPRESSED -> Triple(
            "Explained shift", "Vitals are off baseline, but your journal explains it.",
            AuraChipKind.NEUTRAL,
        )
        IllnessSignalEngine.Level.ALREADY_UNWELL -> Triple(
            "Rest up", "You logged feeling unwell, so recovery weighting is adjusted.",
            AuraChipKind.CAUTION,
        )
        IllnessSignalEngine.Level.QUIET -> Triple("Quiet", "", AuraChipKind.NEUTRAL)
    }
    val tint = when (kind) {
        AuraChipKind.NEGATIVE -> p.bad
        AuraChipKind.CAUTION -> p.fair
        AuraChipKind.POSITIVE -> p.good
        AuraChipKind.NEUTRAL -> p.ink.copy(alpha = 0.7f)
    }
    AuraDarkCard {
        Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.Top) {
            Icon(
                Icons.Filled.MonitorHeart, contentDescription = null,
                tint = tint, modifier = Modifier.width(26.dp).size(20.dp),
            )
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                Text(title, style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                Text(body, style = AuraType.sub, color = p.ink.copy(alpha = 0.7f))
                if (r.firedSignals.isNotEmpty()) {
                    Text(
                        r.firedSignals.joinToString(" · "),
                        style = AuraType.caption, color = p.ink.copy(alpha = 0.5f),
                    )
                }
            }
        }
    }
}

@Composable
private fun CycleCard(c: CyclePhaseEngine.Result) {
    val p = Aura.palette
    val phase = when (c.phase) {
        CyclePhaseEngine.Phase.FOLLICULAR -> "Follicular phase"
        CyclePhaseEngine.Phase.PERI_OVULATORY -> "Peri-ovulatory"
        CyclePhaseEngine.Phase.LUTEAL -> "Luteal phase"
        else -> ""
    }
    AuraDarkCard {
        Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.Top) {
            Icon(
                Icons.Outlined.Circle, contentDescription = null,
                tint = AuraFamily.EFFORT.glow, modifier = Modifier.width(26.dp).size(20.dp),
            )
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                Text(phase, style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                Text(
                    "Estimated from your overnight temperature rhythm. Baselines adapt with your cycle.",
                    style = AuraType.sub, color = p.ink.copy(alpha = 0.7f),
                )
            }
        }
    }
}
