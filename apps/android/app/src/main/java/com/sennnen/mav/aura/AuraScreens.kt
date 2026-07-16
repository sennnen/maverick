package com.sennnen.mav.aura

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material.icons.filled.Sensors
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.sennnen.mav.MavAppState
import com.sennnen.mav.MavSnapshot

@Composable
fun AuraTodayScreen(state: MavAppState) = MavHub("Today", "Private, local, inspectable", AuraFamily.HEART, state) { snapshot ->
    AuraGlowTile(AuraFamily.HEART) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Filled.Favorite, null, tint = AuraFamily.HEART.glow)
            Spacer(Modifier.width(12.dp))
            Column {
                Text(snapshot?.currentBpm?.toString() ?: "--", style = AuraType.mega(64.sp), color = Aura.palette.ink)
                Text("Live heart rate · bpm", style = AuraType.sub, color = Aura.palette.ink.copy(alpha = 0.68f))
            }
        }
        Spacer(Modifier.height(16.dp))
        AuraLiveHRPill(snapshot?.currentBpm, snapshot?.deviceName ?: "No device", null, snapshot?.connectionState == "connected")
    }
    AuraSectionHeader("Daily signals")
    AuraDarkCard {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            AuraMiniStat("--", "Recovery", 0.0, AuraFamily.CHARGE.glow, modifier = Modifier.weight(1f))
            AuraMiniStat("--", "Strain", 0.0, AuraFamily.EFFORT.glow, modifier = Modifier.weight(1f))
            AuraMiniStat("--", "Sleep", 0.0, AuraFamily.REST.glow, modifier = Modifier.weight(1f))
        }
        Text("Scores appear after admitted streams are stored and analysed.", style = AuraType.sub, color = Aura.palette.ink.copy(alpha = 0.62f))
    }
}

@Composable
fun AuraRecoveryScreen(state: MavAppState) = MavHub("Recovery", "Readiness from overnight data", AuraFamily.CHARGE, state) { snapshot ->
    AuraGlowTile(AuraFamily.CHARGE) {
        AuraScoreRing(null, "--", "Recovery", AuraStatus.NONE, tintOverride = AuraFamily.CHARGE.glow)
        Text(snapshot?.recoveryUnavailableReason ?: "No recovery result yet.", style = AuraType.sub, color = Aura.palette.ink.copy(alpha = 0.72f))
    }
    AuraSectionHeader("Vitals")
    AuraDarkCard {
        Text("No overnight vitals available", style = AuraType.title, color = Aura.palette.ink)
        Text("Mav shows raw provenance before presenting a score.", style = AuraType.sub, color = Aura.palette.ink.copy(alpha = 0.62f))
    }
}

@Composable
fun AuraStrainScreen(state: MavAppState) = MavHub("Strain", "Daily load and activity", AuraFamily.EFFORT, state) {
    AuraGlowTile(AuraFamily.EFFORT) {
        AuraScoreRing(null, "--", "Strain", AuraStatus.NONE, tintOverride = AuraFamily.EFFORT.glow)
        Text("No admitted strain algorithm in current core schema.", style = AuraType.sub, color = Aura.palette.ink.copy(alpha = 0.72f))
    }
    AuraDarkCard {
        Text("Activities", style = AuraType.title, color = Aura.palette.ink)
        Text("Connector ingestion will populate this view. Nothing is estimated here.", style = AuraType.sub, color = Aura.palette.ink.copy(alpha = 0.62f))
    }
}

@Composable
fun AuraSleepScreen(state: MavAppState) = MavHub("Sleep", "Overnight timing and stages", AuraFamily.REST, state) {
    AuraGlowTile(AuraFamily.REST) {
        AuraScoreRing(null, "--", "Sleep", AuraStatus.NONE, tintOverride = AuraFamily.REST.glow)
        Text("No admitted sleep result in current core schema.", style = AuraType.sub, color = Aura.palette.ink.copy(alpha = 0.72f))
    }
    AuraDarkCard {
        Text("Sleep detail", style = AuraType.title, color = Aura.palette.ink)
        Text("Stages and SpO₂ stay absent until a connector provides verified data.", style = AuraType.sub, color = Aura.palette.ink.copy(alpha = 0.62f))
    }
}

@Composable
private fun MavHub(title: String, subtitle: String, family: AuraFamily, state: MavAppState, content: @Composable (MavSnapshot?) -> Unit) {
    val snapshot = (state as? MavAppState.Ready)?.snapshot
    AuraScreen(lead = family) {
        Column(Modifier.statusBarsPadding().padding(horizontal = Aura.screenMargin, vertical = 14.dp), verticalArrangement = Arrangement.spacedBy(Aura.cardSpacing)) {
            AuraHubHeader(title, subtitle)
            content(snapshot)
            if (state is MavAppState.Failed) AuraDarkCard {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Filled.Refresh, null, tint = Aura.palette.bad)
                    Spacer(Modifier.width(10.dp))
                    Text("${state.code}: ${state.message}", style = AuraType.sub, color = Aura.palette.ink)
                }
            }
        }
    }
}

@Composable
fun MavSettingsSheet(state: MavAppState, onRefresh: () -> Unit, onDismiss: () -> Unit) {
    AuraSheet("Settings", onDismiss) {
        val snapshot = (state as? MavAppState.Ready)?.snapshot
        AuraNavRow(Icons.Filled.Sensors, "Core", "Local Mav runtime", AuraFamily.CHARGE.glow, onRefresh)
        AuraNavRow(Icons.Filled.Storage, "Storage", "Schema ${snapshot?.storageSchema ?: "--"}", AuraFamily.VITALS.glow) {}
        AuraNavRow(Icons.Filled.Sensors, "Connectors", "Installed separately; none bundled", AuraFamily.HEART.glow) {}
        AuraNavRow(Icons.Filled.Refresh, "Diagnostics", snapshot?.connectionState ?: "Runtime unavailable", AuraFamily.EFFORT.glow, onRefresh)
    }
}
