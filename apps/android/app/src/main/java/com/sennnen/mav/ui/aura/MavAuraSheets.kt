package com.sennnen.mav.ui.aura

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Sensors
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.MavAppState
import com.sennnen.mav.ui.AppViewModel

// Mav stand-ins for NOOP's AuraToolSheets (pairing / migrate / diagnostics). NOOP's originals
// operate its Room store and BLE engine directly; those lanes belong to Mav's Rust core, so
// these sheets stay presentation-only: same names + signatures (AuraRoot.kt is byte-identical
// to the NOOP source), real snapshot facts where the core has them, honest text where not.

@Composable
fun AuraPairingSheet(vm: AppViewModel, onDismiss: () -> Unit, onOpenDevices: () -> Unit) {
    val live by vm.live.collectAsStateWithLifecycle()
    val p = Aura.palette
    AuraSheet("Pair a strap", onDismiss, family = AuraFamily.HEART) {
        Row {
            AuraStatusChip(
                text = if (live.bonded) "Connected" else "No strap",
                kind = if (live.bonded) AuraChipKind.POSITIVE else AuraChipKind.NEUTRAL,
            )
        }
        Spacer(Modifier.padding(top = 14.dp))
        AuraDarkCard {
            Text(
                "Mav pairs through device connectors — signed packages that carry each strap's " +
                    "protocol. None are bundled yet, so scanning is disabled.",
                style = AuraType.sub, color = p.ink.copy(alpha = 0.68f),
            )
        }
        Spacer(Modifier.padding(top = 10.dp))
        AuraDarkCard(padding = 0.dp) {
            Column(Modifier.padding(vertical = 4.dp)) {
                AuraNavRow(Icons.Filled.Sensors, "Devices", "Connectors & pairing", onClick = onOpenDevices)
            }
        }
    }
}

@Composable
fun AuraMigrateSheet(vm: AppViewModel, onDismiss: () -> Unit) {
    val p = Aura.palette
    AuraSheet("Import data", onDismiss, family = AuraFamily.VITALS) {
        AuraDarkCard {
            Text(
                "Historical import (WHOOP export, Health Connect) runs through the core's ingest " +
                    "lane so every row keeps its provenance. It isn't wired into this build yet.",
                style = AuraType.sub, color = p.ink.copy(alpha = 0.68f),
            )
        }
    }
}

@Composable
fun AuraDiagnosticsSheet(vm: AppViewModel, onDismiss: () -> Unit) {
    val state by vm.state.collectAsStateWithLifecycle()
    val snapshot = (state as? MavAppState.Ready)?.snapshot
    val p = Aura.palette
    AuraSheet("Diagnostics", onDismiss) {
        AuraDarkCard(padding = 0.dp) {
            Column(Modifier.padding(vertical = 4.dp)) {
                DiagRow("Core", snapshot?.coreVersion ?: "unavailable")
                DiagRow("Storage schema", snapshot?.storageSchema?.toString() ?: "--")
                DiagRow("Snapshot revision", snapshot?.revision?.toString() ?: "--")
                DiagRow("Connection", snapshot?.connectionState ?: "--")
                DiagRow("Snapshot hash", snapshot?.hash?.take(12) ?: "--")
            }
        }
        Spacer(Modifier.padding(top = 10.dp))
        (state as? MavAppState.Failed)?.let { failed ->
            AuraDarkCard {
                Text("${failed.code}: ${failed.message}", style = AuraType.sub, color = p.bad)
            }
            Spacer(Modifier.padding(top = 10.dp))
        }
        AuraDarkCard(padding = 0.dp) {
            Column(Modifier.padding(vertical = 4.dp)) {
                AuraNavRow(Icons.Filled.Refresh, "Refresh snapshot", "Re-read the core", onClick = { vm.refresh() })
                AuraNavRow(Icons.Filled.Storage, "Report bundle", "Planned: logs + error journal", onClick = {})
            }
        }
    }
}

@Composable
private fun DiagRow(label: String, value: String) {
    val p = Aura.palette
    Row(Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 13.dp)) {
        Text(label, style = AuraType.label, color = p.ink.copy(alpha = 0.9f))
        Spacer(Modifier.weight(1f))
        Text(value, style = AuraType.label, color = p.ink.copy(alpha = 0.72f))
    }
}
