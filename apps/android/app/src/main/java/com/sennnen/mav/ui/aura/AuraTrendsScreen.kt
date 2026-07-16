package com.sennnen.mav.ui.aura

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.data.DailyMetric
import com.sennnen.mav.ui.AppViewModel
import kotlin.math.roundToInt

// Multi-horizon trends (Android port of Strand/UI/AuraTrendsView.swift):
// 1w / 1m / 6m range switching, one labelled dark-card chart per metric.
// Surfaced from Today.

private data class TrendMetric(
    val id: String,
    val title: String,
    val family: AuraFamily,
    val unit: String,
    val points: List<AuraPoint>,
)

@Composable
fun AuraTrendsScreen(vm: AppViewModel, onClose: () -> Unit) {
    val p = Aura.palette
    val days by vm.recentDays.collectAsStateWithLifecycle()
    var range by remember { mutableStateOf(AuraTrendRange.MONTH) }
    var restSeries by remember { mutableStateOf<List<AuraPoint>>(emptyList()) }
    val factor = AuraEffort.displayFactor()

    LaunchedEffect(days) {
        restSeries = runCatching {
            vm.repo.metricSeries("my-whoop", "sleep_performance", "0000-00-00", "9999-99-99")
        }.getOrDefault(emptyList()).map { AuraPoint(it.day, it.value) }
    }

    val metrics = buildMetrics(days, restSeries, range.days, factor)

    AuraScreen(lead = AuraFamily.CHARGE) {
        Column(Modifier.fillMaxSize().statusBarsPadding()) {
            AuraSheetBar(title = "Trends", onClose = onClose)
            Column(
                Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = Aura.screenMargin)
                    .padding(bottom = 48.dp),
                verticalArrangement = Arrangement.spacedBy(20.dp),
            ) {
                Row { AuraRangePicker(selection = range, onSelect = { range = it }) }

                metrics.forEach { m ->
                    AuraDarkCard {
                        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                Text(m.title, style = AuraType.heading(17.sp), color = p.ink)
                                Spacer(Modifier.weight(1f))
                                Text(
                                    m.points.lastOrNull()?.value?.roundToInt()?.toString() ?: "--",
                                    style = AuraType.mega(40.sp), color = m.family.glow,
                                )
                                if (m.unit.isNotEmpty()) {
                                    Text(
                                        m.unit, style = AuraType.number(16.sp),
                                        color = p.ink.copy(alpha = 0.55f),
                                        modifier = Modifier.padding(bottom = 6.dp),
                                    )
                                }
                            }
                            AuraGraph(
                                points = m.points,
                                tint = m.family.glow,
                                unit = m.unit,
                                style = if (m.id == "str") AuraGraphStyle.BARS else AuraGraphStyle.LINE,
                            )
                        }
                    }
                }
            }
        }
    }
}

private fun buildMetrics(
    days: List<DailyMetric>,
    restSeries: List<AuraPoint>,
    n: Int,
    effortFactor: Double,
): List<TrendMetric> {
    val window = days.takeLast(n)
    fun pts(selector: (DailyMetric) -> Double?): List<AuraPoint> =
        window.mapNotNull { d -> selector(d)?.let { AuraPoint(d.day, it) } }
    return listOf(
        TrendMetric("rec", "Charge", AuraFamily.CHARGE, "%", pts { it.recovery }),
        TrendMetric("rest", "Rest", AuraFamily.REST, "%", restSeries.takeLast(n)),
        TrendMetric(
            "str", "Effort", AuraFamily.EFFORT, "",
            pts { it.strain }.map { AuraPoint(it.day, it.value * effortFactor) },
        ),
        TrendMetric("hrv", "HRV", AuraFamily.CHARGE, "ms", pts { it.avgHrv }),
        TrendMetric("rhr", "Resting HR", AuraFamily.HEART, "bpm", pts { it.restingHr?.toDouble() }),
    )
}
