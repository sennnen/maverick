package com.sennnen.mav.ui.aura

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.util.Locale
import kotlin.math.roundToInt

// The one metric flyout (Android port of Strand/UI/AuraMetricDetailView.swift):
// glow hero with status + baseline delta, an interactive scrubbable history
// graph with 1W/1M/6M ranges, range statistics, contributors, and provenance.

data class AuraDetailData(
    val family: AuraFamily,
    val title: String,
    val value: Double?,
    val unit: String,
    val decimals: Int = 0,
    /** Baseline (21-day) to show the delta against; null hides the delta. */
    val baseline: Double? = null,
    val status: AuraStatus = AuraStatus.NONE,
    val caption: String,
    /** Full day-keyed history (oldest → newest); the range picker clips it. */
    val points: List<AuraPoint> = emptyList(),
    val barStyle: Boolean = false,
    /** 0–1 for the hero slider; null hides it. */
    val heroFraction: Double? = null,
    val contributors: List<Contributor> = emptyList(),
    val provenance: String = "Computed on-device from your strap's raw signals.",
) {
    data class Contributor(val label: String, val value: String, val level: Double, val tint: Color)
}

@Composable
fun AuraMetricDetailSheet(data: AuraDetailData, onDismiss: () -> Unit) {
    val p = Aura.palette
    var range by remember { mutableStateOf(AuraTrendRange.MONTH) }
    val ranged = data.points.takeLast(range.days)
    val rangedValues = ranged.map { it.value }

    fun fmt(v: Double?): String {
        if (v == null) return "--"
        return if (data.decimals == 0) v.roundToInt().toString()
        else String.format(Locale.US, "%.${data.decimals}f", v)
    }

    AuraSheet(title = data.title, onDismiss = onDismiss, family = data.family) {
        // MARK: Hero
        AuraGlowTile(data.family, padding = 22.dp, radius = 34.dp) {
            Column(
                Modifier.heightIn(min = 210.dp),
                verticalArrangement = Arrangement.spacedBy(18.dp),
            ) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(data.title, style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                    Spacer(Modifier.weight(1f))
                    if (data.status != AuraStatus.NONE) {
                        AuraStatusChip(text = data.status.word, kind = data.status.chipKind)
                    } else {
                        delta(data)?.let { AuraDelta(value = it) }
                    }
                }
                Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(fmt(data.value), style = AuraType.mega(76.sp), color = p.ink, maxLines = 1)
                    if (data.value != null && data.unit.isNotEmpty()) {
                        Text(
                            data.unit, style = AuraType.number(26.sp),
                            color = p.ink.copy(alpha = 0.66f),
                            modifier = Modifier.padding(bottom = 10.dp),
                        )
                    }
                }
                if (data.status != AuraStatus.NONE) {
                    delta(data)?.let { AuraDelta(value = it) }
                }
                data.heroFraction?.let { AuraSlider(value = it, glow = data.family.glow) }
                Text(data.caption, style = AuraType.sub, color = p.ink.copy(alpha = 0.8f))
            }
        }

        // MARK: History
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text("History", style = AuraType.heading(19.sp), color = p.ink)
                Spacer(Modifier.weight(1f))
                AuraRangePicker(selection = range, onSelect = { range = it })
            }
            AuraDarkCard {
                AuraGraph(
                    points = ranged,
                    tint = data.family.glow,
                    unit = data.unit,
                    style = if (data.barStyle) AuraGraphStyle.BARS else AuraGraphStyle.LINE,
                    decimals = data.decimals,
                )
            }
        }

        // MARK: Range stats
        if (rangedValues.isNotEmpty()) {
            val avg = rangedValues.sum() / rangedValues.size
            AuraDarkCard(padding = 20.dp) {
                Row(horizontalArrangement = Arrangement.spacedBy(18.dp)) {
                    stat("Average", fmt(avg), Modifier.weight(1f))
                    stat("Lowest", fmt(rangedValues.min()), Modifier.weight(1f))
                    stat("Highest", fmt(rangedValues.max()), Modifier.weight(1f))
                    stat("Days", "${rangedValues.size}", Modifier.weight(1f))
                }
            }
        }

        // MARK: Contributors
        if (data.contributors.isNotEmpty()) {
            Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                AuraSectionHeader(title = "What feeds this")
                AuraDarkCard(padding = 20.dp) {
                    data.contributors.chunked(2).forEachIndexed { i, rowItems ->
                        if (i > 0) Spacer(Modifier.padding(top = 22.dp))
                        Row(horizontalArrangement = Arrangement.spacedBy(20.dp)) {
                            rowItems.forEach { c ->
                                AuraMiniStat(
                                    value = c.value, label = c.label, level = c.level,
                                    tint = c.tint, modifier = Modifier.weight(1f),
                                )
                            }
                            if (rowItems.size == 1) Spacer(Modifier.weight(1f))
                        }
                    }
                }
            }
        }

        Text(
            data.provenance, style = AuraType.caption, color = p.ink.copy(alpha = 0.45f),
            modifier = Modifier.padding(horizontal = 4.dp),
        )
    }
}

@Composable
private fun stat(label: String, value: String, modifier: Modifier = Modifier) {
    val p = Aura.palette
    Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(label, style = AuraType.caption, color = p.ink.copy(alpha = 0.55f))
        Text(value, style = AuraType.number(22.sp), color = p.ink, maxLines = 1)
    }
}

private fun delta(data: AuraDetailData): Double? {
    val v = data.value ?: return null
    val b = data.baseline ?: return null
    return v - b
}
