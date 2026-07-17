package com.sennnen.mav.ui.aura

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Alarm
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.analytics.RestScorer
import com.sennnen.mav.analytics.StageSegment
import com.sennnen.mav.data.SleepSession
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.parsePersistedSegments
import java.text.DateFormat
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.max
import kotlin.math.roundToInt

// Sleep hub (Android port of Strand/UI/AuraSleepHubView.swift) — performance vs
// need, last night's hypnogram, stage breakdown, sleep bank, naps, and the
// haptic alarm (existing Smart-Alarm engine, pushed from the nav row).

private data class AuraSleepFigures(
    val needMin: Double? = null,
    val debtMin: Double? = null,
    val consistencyPct: Double? = null,
)

@Composable
fun AuraSleepScreen(vm: AppViewModel, onOpenAlarm: () -> Unit) {
    val p = Aura.palette
    val days by vm.recentDays.collectAsStateWithLifecycle()
    val anchor = auraAnchorDay(days)

    var restPct by remember { mutableStateOf<Double?>(null) }
    var figures by remember { mutableStateOf(AuraSleepFigures()) }
    var night by remember { mutableStateOf<List<StageSegment>>(emptyList()) }
    var naps by remember { mutableStateOf<List<SleepSession>>(emptyList()) }

    LaunchedEffect(days) {
        val a = anchor ?: return@LaunchedEffect
        suspend fun series(key: String) = runCatching {
            vm.repo.metricSeries("my-whoop", key, "0000-00-00", "9999-99-99")
        }.getOrDefault(emptyList()).associate { it.day to it.value }

        val perf = series("sleep_performance")
        restPct = perf[a.day]
            ?: (if (a.day == java.time.LocalDate.now().toString()) perf.entries.lastOrNull()?.value else null)
            ?: RestScorer.restFromDaily(a)
        figures = AuraSleepFigures(
            needMin = series("sleep_need_min")[a.day],
            debtMin = series("sleep_debt_min")[a.day],
            consistencyPct = series("sleep_consistency")[a.day],
        )

        // Last night's sessions: any session ENDING on the anchor day (local
        // wake-day). Longest block is the night; short extras are naps.
        val now = System.currentTimeMillis() / 1000L
        val fmt = SimpleDateFormat("yyyy-MM-dd", Locale.US)
        fun dayKey(ts: Long) = fmt.format(Date(ts * 1000L))
        val all = runCatching {
            val imported = vm.repo.sleepSessionsUnion(vm.activeStrapId, 0L, now)
            val computed = vm.repo.computedSleepSessionsUnion(vm.activeStrapId, 0L, now)
            // Exact-duplicate blocks recorded under both id families: prefer the
            // one that carries a stage timeline.
            (imported + computed)
                .groupBy { it.startTs to it.endTs }
                .map { (_, dupes) -> dupes.firstOrNull { it.stagesJSON != null } ?: dupes.first() }
        }.getOrDefault(emptyList())
        val ofDay = all.filter { dayKey(it.endTs) == a.day }
        val main = ofDay.maxByOrNull { it.endTs - it.effectiveStartTs }
        if (main != null) {
            night = parsePersistedSegments(main.stagesJSON)
                .orEmpty()
                .map { StageSegment(it.start, it.end, it.stage) }
                .sortedBy { it.start }
            naps = ofDay.filter {
                it.startTs != main.startTs && (it.endTs - it.effectiveStartTs) < 3 * 3600
            }
        } else {
            night = emptyList()
            naps = emptyList()
        }
    }

    var revealed by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { revealed = true }
    var editing by remember { mutableStateOf(false) }
    val (hiddenCSV, setHiddenCSV) = rememberHubHiddenCards("sleep")

    val status = AuraStatus.sleep(restPct)

    AuraScreen(lead = AuraFamily.REST) {
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
                title = "Sleep",
                subtitle = "Last night, and what it bought you",
                editing = editing,
                onToggleEditing = { editing = !editing },
            )

            // MARK: Hero
            AuraGlowTile(
                AuraFamily.REST,
                modifier = Modifier.auraReveal(revealed, 1),
                padding = 22.dp, radius = 34.dp,
            ) {
                Column(
                    Modifier.heightIn(min = 240.dp),
                    verticalArrangement = Arrangement.spacedBy(20.dp),
                ) {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Text("Rest", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                        Spacer(Modifier.weight(1f))
                        AuraStatusChip(text = status.word, kind = status.chipKind)
                    }
                    Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(
                            restPct?.roundToInt()?.toString() ?: "--",
                            style = AuraType.mega(88.sp), color = p.ink, maxLines = 1,
                        )
                        if (restPct != null) {
                            Text(
                                "%", style = AuraType.number(30.sp),
                                color = p.ink.copy(alpha = 0.66f),
                                modifier = Modifier.padding(bottom = 12.dp),
                            )
                        }
                    }
                    AuraSlider(value = (restPct ?: 0.0) / 100, glow = AuraFamily.REST.glow)
                    Text(
                        needLine(anchor?.totalSleepMin, figures.needMin),
                        style = AuraType.sub, color = p.ink.copy(alpha = 0.8f),
                    )
                }
            }

            // MARK: Stages
            AuraEditableCard("stages", hiddenCSV, setHiddenCSV, editing) {
                Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    AuraSectionHeader(title = "Last night")
                    AuraDarkCard {
                        if (night.isEmpty()) {
                            FallbackStageBar(
                                deepMin = anchor?.deepMin,
                                remMin = anchor?.remMin,
                                lightMin = anchor?.lightMin,
                            )
                        } else {
                            AuraHypnogram(segments = night)
                        }
                    }
                }
            }

            // MARK: Breakdown
            AuraEditableCard("breakdown", hiddenCSV, setHiddenCSV, editing) {
                AuraDarkCard(padding = 20.dp) {
                    val deepTint = if (p.dark) Color(0xFF3E7BFF) else Color(0xFF2F5FD0)
                    val lightTint = if (p.dark) Color(0xFF6E9BFF) else Color(0xFF5B82D8)
                    BreakdownRow(
                        Triple(auraHmText(anchor?.totalSleepMin), "Asleep", (anchor?.totalSleepMin ?: 0.0) / 540) to AuraFamily.REST.glow(p.dark),
                        Triple(anchor?.efficiency?.roundToInt()?.toString() ?: "--", "Efficiency", (anchor?.efficiency ?: 0.0) / 100) to AuraFamily.CHARGE.glow(p.dark),
                        unitA = "", unitB = "%",
                    )
                    Spacer(Modifier.padding(top = 22.dp))
                    BreakdownRow(
                        Triple(auraHmText(anchor?.deepMin), "Deep", (anchor?.deepMin ?: 0.0) / 150) to deepTint,
                        Triple(auraHmText(anchor?.remMin), "REM", (anchor?.remMin ?: 0.0) / 150) to AuraFamily.VITALS.glow(p.dark),
                    )
                    Spacer(Modifier.padding(top = 22.dp))
                    BreakdownRow(
                        Triple(auraHmText(anchor?.lightMin), "Light", (anchor?.lightMin ?: 0.0) / 300) to lightTint,
                        Triple(anchor?.disturbances?.toString() ?: "--", "Disturbances", (anchor?.disturbances ?: 0) / 20.0) to AuraFamily.HEART.glow(p.dark),
                    )
                }
            }

            // MARK: Planner — slider → tonight's bedtime for a full recharge
            AuraEditableCard("planner", hiddenCSV, setHiddenCSV, editing) {
                Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    AuraSectionHeader(title = "Sleep planner")
                    SleepPlannerCard(needMin = figures.needMin, debtMin = figures.debtMin)
                }
            }

            // MARK: Sleep bank
            if (figures.needMin != null || figures.debtMin != null || figures.consistencyPct != null) {
                AuraEditableCard("debt", hiddenCSV, setHiddenCSV, editing) {
                    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                        AuraSectionHeader(title = "Sleep bank")
                        AuraDarkCard(padding = 20.dp) {
                            Row(horizontalArrangement = Arrangement.spacedBy(18.dp)) {
                                figures.needMin?.let {
                                    BankStat("Need", auraHmText(it), AuraStatus.NONE, Modifier.weight(1f))
                                }
                                figures.debtMin?.let { debt ->
                                    BankStat(
                                        "Debt", auraHmText(debt),
                                        when {
                                            debt <= 30 -> AuraStatus.GOOD
                                            debt <= 90 -> AuraStatus.FAIR
                                            else -> AuraStatus.LOW
                                        },
                                        Modifier.weight(1f),
                                    )
                                }
                                figures.consistencyPct?.let { cons ->
                                    BankStat(
                                        "Consistency", "${cons.roundToInt()}%",
                                        when {
                                            cons >= 80 -> AuraStatus.GOOD
                                            cons >= 60 -> AuraStatus.FAIR
                                            else -> AuraStatus.LOW
                                        },
                                        Modifier.weight(1f),
                                    )
                                }
                            }
                        }
                    }
                }
            }

            // MARK: Naps
            if (naps.isNotEmpty()) {
                AuraEditableCard("naps", hiddenCSV, setHiddenCSV, editing) {
                    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                        AuraSectionHeader(title = "Naps")
                        AuraDarkCard(padding = 0.dp) {
                            Spacer(Modifier.padding(top = 4.dp))
                            naps.forEachIndexed { i, n ->
                                Row(
                                    Modifier
                                        .fillMaxWidth()
                                        .padding(horizontal = 18.dp, vertical = 15.dp),
                                ) {
                                    Text(
                                        DateFormat.getTimeInstance(DateFormat.SHORT)
                                            .format(Date(n.effectiveStartTs * 1000L)),
                                        style = AuraType.label, color = p.ink.copy(alpha = 0.9f),
                                    )
                                    Spacer(Modifier.weight(1f))
                                    Text(
                                        auraHmText((n.endTs - n.effectiveStartTs) / 60.0),
                                        style = AuraType.label, color = p.ink.copy(alpha = 0.78f),
                                    )
                                }
                                if (i < naps.size - 1) {
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
            }

            // MARK: Alarm
            AuraDarkCard(padding = 0.dp) {
                Spacer(Modifier.padding(top = 4.dp))
                AuraNavRow(
                    icon = Icons.Filled.Alarm,
                    title = "Haptic alarm",
                    detail = "Wake by wrist buzz",
                    tint = p.accentInk,
                    onClick = onOpenAlarm,
                )
                Spacer(Modifier.padding(top = 4.dp))
            }
        }
    }
}

/** Target-wake slider → the bedtime that covers tonight's need (need + a settle
 *  allowance; the plan's "slider for target calc"). Wake choice persists. */
@Composable
private fun SleepPlannerCard(needMin: Double?, debtMin: Double?) {
    val p = Aura.palette
    val context = androidx.compose.ui.platform.LocalContext.current
    var wakeMin by androidx.compose.runtime.saveable.rememberSaveable {
        androidx.compose.runtime.mutableIntStateOf(
            com.sennnen.mav.ui.MavPrefs.of(context).getInt("aura.sleep.targetWakeMin", 7 * 60),
        )
    }
    val need = needMin ?: 480.0
    val settle = 20                                   // minutes to fall asleep
    val bed = (((wakeMin - need - settle).roundToInt() % (24 * 60)) + 24 * 60) % (24 * 60)
    fun clock(m: Int): String {
        val t = java.time.LocalTime.of(m / 60, m % 60)
        return t.format(java.time.format.DateTimeFormatter.ofPattern("HH:mm"))
    }
    AuraDarkCard(padding = 20.dp) {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("In bed by", style = AuraType.sub, color = p.ink.copy(alpha = 0.7f), modifier = Modifier.padding(bottom = 4.dp))
                Text(clock(bed), style = AuraType.number(34.sp), color = AuraFamily.REST.glow(p.dark))
            }
            Text(
                buildString {
                    append("Wake at ${clock(wakeMin)} · covers your ${auraHmText(need)} need + ${settle}m to drift off")
                    if ((debtMin ?: 0.0) > 30) append(" · tonight's need already carries your debt")
                },
                style = AuraType.caption, color = p.ink.copy(alpha = 0.6f),
            )
            androidx.compose.material3.Slider(
                value = wakeMin.toFloat(),
                onValueChange = {
                    wakeMin = (it / 15f).roundToInt() * 15
                    com.sennnen.mav.ui.MavPrefs.of(context).edit().putInt("aura.sleep.targetWakeMin", wakeMin).apply()
                },
                valueRange = 240f..660f,   // 04:00 – 11:00
                steps = 27,
            )
        }
    }
}

private fun needLine(sleptMin: Double?, needMin: Double?): String {
    if (sleptMin != null && needMin != null && needMin > 0) {
        return "You slept ${auraHmText(sleptMin)} of the ${auraHmText(needMin)} your body needed."
    }
    return "How restorative last night was: duration, efficiency, deep and REM."
}

@Composable
private fun BankStat(label: String, value: String, status: AuraStatus, modifier: Modifier = Modifier) {
    val p = Aura.palette
    Column(modifier, verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            if (status != AuraStatus.NONE) {
                Box(Modifier.size(7.dp).background(status.color, CircleShape))
            }
            Text(label, style = AuraType.caption, color = p.ink.copy(alpha = 0.6f))
        }
        Text(value, style = AuraType.number(26.sp), color = p.ink, maxLines = 1)
    }
}

@Composable
private fun BreakdownRow(
    a: Pair<Triple<String, String, Double>, Color>,
    b: Pair<Triple<String, String, Double>, Color>,
    unitA: String = "",
    unitB: String = "",
) {
    Row(horizontalArrangement = Arrangement.spacedBy(20.dp)) {
        AuraMiniStat(
            value = a.first.first, label = a.first.second, level = a.first.third,
            tint = a.second, unit = unitA, modifier = Modifier.weight(1f),
        )
        AuraMiniStat(
            value = b.first.first, label = b.first.second, level = b.first.third,
            tint = b.second, unit = unitB, modifier = Modifier.weight(1f),
        )
    }
}

/** Proportional stacked bar for nights without a stage timeline (imports). */
@Composable
private fun FallbackStageBar(deepMin: Double?, remMin: Double?, lightMin: Double?) {
    val p = Aura.palette
    val parts = listOf(
        Triple("Deep", deepMin ?: 0.0, if (p.dark) Color(0xFF3E7BFF) else Color(0xFF2F5FD0)),
        Triple("REM", remMin ?: 0.0, if (p.dark) Color(0xFF12AEBE) else Color(0xFF0F93A1)),
        Triple("Light", lightMin ?: 0.0, if (p.dark) Color(0xFF6E9BFF) else Color(0xFF5B82D8)),
    )
    val total = parts.sumOf { it.second }
    if (total > 0) {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(
                Modifier.fillMaxWidth().height(14.dp),
                horizontalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                parts.forEach { (_, min, tint) ->
                    if (min > 0) {
                        Box(
                            Modifier
                                .weight(max(min / total, 0.02).toFloat())
                                .fillMaxWidth()
                                .height(14.dp)
                                .background(tint, RoundedCornerShape(3.dp))
                        )
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                parts.forEach { (label, min, tint) ->
                    Row(horizontalArrangement = Arrangement.spacedBy(5.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(7.dp).background(tint, CircleShape))
                        Text(
                            "$label ${auraHmText(min)}",
                            style = AuraType.caption, color = p.ink.copy(alpha = 0.7f),
                        )
                    }
                }
            }
        }
    } else {
        Text(
            "No staged sleep recorded",
            style = AuraType.caption, color = p.ink.copy(alpha = 0.55f),
        )
    }
}
