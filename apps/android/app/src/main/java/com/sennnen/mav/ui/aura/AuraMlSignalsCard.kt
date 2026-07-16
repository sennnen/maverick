package com.sennnen.mav.ui.aura

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Air
import androidx.compose.material.icons.filled.DirectionsRun
import androidx.compose.material.icons.filled.Memory
import androidx.compose.material.icons.filled.Psychology
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.ui.AppViewModel
import kotlin.math.roundToInt

// Android twin of iOS Strand/UI/AuraMLSignalsCard.swift — the read-only surface for the on-device ML
// layer (MlEngine). Shows only the signals that have a value yet, so it stays quiet until the strap has
// streamed enough beats. Honestly labelled as an on-device estimate, never a medical reading. Surfaces
// stress, cardio fitness and respiration; excludes heart-rhythm (AFib) + blood pressure, matching the
// engine's published set. Pure Material 3.
@Composable
fun AuraMlSignalsCard(vm: AppViewModel) {
    val p = Aura.palette
    val engine = vm.mlEngine
    val backboneActive by engine.backboneActive.collectAsStateWithLifecycle()
    val stressLoad by engine.stressLoad.collectAsStateWithLifecycle()
    val vo2max by engine.vo2max.collectAsStateWithLifecycle()
    val respiration by engine.respirationRate.collectAsStateWithLifecycle()

    val hasAny = stressLoad != null || vo2max != null || respiration != null

    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        AuraSectionHeader(title = "On-device signals")

        if (backboneActive) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Icon(Icons.Filled.Memory, null, tint = p.good, modifier = Modifier.size(14.dp))
                Text(
                    "Pulse-PPG model active on device",
                    style = AuraType.caption, color = p.ink.copy(alpha = 0.6f),
                )
            }
        }

        if (!hasAny) {
            AuraDarkCard(padding = 18.dp) {
                Text(
                    "Wear your strap for a few minutes. Stress load, cardio fitness and respiration are " +
                        "estimated on device, privately.",
                    style = AuraType.sub, color = p.ink.copy(alpha = 0.6f),
                )
            }
        } else {
            AuraDarkCard(padding = 0.dp) {
                Column(Modifier.padding(vertical = 4.dp)) {
                    var shown = false
                    stressLoad?.let {
                        StressRow(it)
                        shown = true
                    }
                    vo2max?.let {
                        if (shown) androidx.compose.material3.HorizontalDivider(
                            color = p.hairline, modifier = Modifier.padding(start = 18.dp),
                        )
                        MlRow(Icons.Filled.DirectionsRun, "Cardio fitness", it.roundToInt().toString(), "VO₂max", AuraFamily.CHARGE.glow)
                        shown = true
                    }
                    respiration?.let {
                        if (shown) androidx.compose.material3.HorizontalDivider(
                            color = p.hairline, modifier = Modifier.padding(start = 18.dp),
                        )
                        MlRow(Icons.Filled.Air, "Respiration", it.roundToInt().toString(), "br/min", AuraFamily.HEART.glow)
                    }
                }
            }
            Text(
                "Estimated on your phone from the strap's optical signal. A screen, not a diagnosis.",
                style = AuraType.caption, color = p.ink.copy(alpha = 0.4f),
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
    }
}

// Two-line stress row (label + qualitative word) with a big load number on the right — mirrors the iOS
// stressRow. Distinct from MlRow because stress carries a subtitle and no trailing unit.
@Composable
private fun StressRow(load: Double) {
    val p = Aura.palette
    val word = if (load >= 66) "High" else if (load >= 40) "Moderate" else "Low"
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 13.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Icon(Icons.Filled.Psychology, null, tint = AuraFamily.EFFORT.glow, modifier = Modifier.size(20.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text("Stress load", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
            Text("$word · from HRV & heart-rate", style = AuraType.caption, color = p.ink.copy(alpha = 0.55f))
        }
        Text(load.roundToInt().toString(), style = AuraType.number(22.sp), color = p.ink, fontWeight = FontWeight.Normal)
    }
}

@Composable
private fun MlRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, value: String, unit: String, tint: androidx.compose.ui.graphics.Color) {
    val p = Aura.palette
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 13.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(20.dp))
        Text(title, style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
        Spacer(Modifier.weight(1f))
        Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(value, style = AuraType.number(22.sp), color = p.ink, fontWeight = FontWeight.Normal)
            Text(unit, style = AuraType.caption, color = p.ink.copy(alpha = 0.5f), modifier = Modifier.padding(bottom = 3.dp))
        }
    }
}
