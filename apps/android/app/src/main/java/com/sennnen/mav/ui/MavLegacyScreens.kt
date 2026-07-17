package com.sennnen.mav.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Sensors
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.MavAppState
import com.sennnen.mav.ui.aura.Aura
import com.sennnen.mav.ui.aura.AuraChipKind
import com.sennnen.mav.ui.aura.AuraDarkCard
import com.sennnen.mav.ui.aura.AuraFamily
import com.sennnen.mav.ui.aura.AuraGlowTile
import com.sennnen.mav.ui.aura.AuraHubHeader
import com.sennnen.mav.ui.aura.AuraNavRow
import com.sennnen.mav.ui.aura.AuraScreen
import com.sennnen.mav.ui.aura.AuraSectionHeader
import com.sennnen.mav.ui.aura.AuraStatusChip
import com.sennnen.mav.ui.aura.AuraType
import java.util.Locale
import kotlin.math.roundToInt

// The destinations AuraRoot routes to that Maverick backs with its own on-device subsystems
// (Room store, BLE engine, importers, AI coach, ML). Mav's core owns those lanes, so until
// each lands in the Rust core these destinations render an honest Aura-styled empty state —
// same signatures as Maverick's screens, so AuraRoot.kt stays byte-identical to the source.

/** Live console — real data: the snapshot's live HR, connection state and strap identity. */
@Composable
fun LiveScreen(vm: AppViewModel, onManageDevices: () -> Unit) {
    val live by vm.live.collectAsStateWithLifecycle()
    val bpm by vm.bpm.collectAsStateWithLifecycle()
    val appState by vm.state.collectAsStateWithLifecycle()
    val snapshot = (appState as? MavAppState.Ready)?.snapshot
    val p = Aura.palette
    AuraScreen(lead = AuraFamily.HEART) {
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .statusBarsPadding()
                .padding(horizontal = Aura.screenMargin)
                .padding(top = 8.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(Aura.sectionGap),
        ) {
            AuraHubHeader(title = "Live", subtitle = "Realtime strap readout")
            AuraGlowTile(AuraFamily.HEART) {
                Row {
                    AuraStatusChip(
                        text = if (live.bonded) "Connected" else "Searching",
                        kind = if (live.bonded) AuraChipKind.POSITIVE else AuraChipKind.NEUTRAL,
                        pulsing = !live.bonded,
                    )
                }
                Spacer(Modifier.height(20.dp))
                Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                    Icon(
                        Icons.Filled.Favorite, contentDescription = null,
                        tint = AuraFamily.HEART.glow, modifier = Modifier.size(46.dp),
                    )
                    Spacer(Modifier.height(12.dp))
                    Row(verticalAlignment = Alignment.Bottom) {
                        Text((bpm ?: live.heartRate)?.toString() ?: "--", style = AuraType.mega(88.sp), color = p.ink)
                        Spacer(Modifier.width(6.dp))
                        Text("bpm", style = AuraType.number(24.sp), color = p.ink.copy(alpha = 0.5f))
                    }
                }
                Spacer(Modifier.height(8.dp))
            }
            AuraDarkCard(padding = 0.dp) {
                Column(Modifier.padding(vertical = 4.dp)) {
                    InfoRow("Strap", live.advertisingName ?: "WHOOP")
                    InfoRow("Battery", live.batteryPct?.let { "${it.roundToInt()}%" } ?: "--")
                    val prv = snapshot?.prv
                    if (prv != null) {
                        InfoRow("PRV · RMSSD", String.format(Locale.getDefault(), "%.1f ms", prv.rmssdMicros / 1000.0))
                        InfoRow("PRV intervals", "${prv.intervalCount} used · ${prv.excludedIntervalCount} excluded")
                    } else {
                        // The core's structured reason; the platform never invents availability.
                        InfoRow("PRV", snapshot?.prvUnavailableReason ?: "--")
                    }
                }
            }
            if (snapshot?.prv != null) {
                Text(
                    "PRV is optical pulse-rate variability, not ECG HRV.",
                    style = AuraType.sub, color = p.ink.copy(alpha = 0.55f),
                )
            }
            AuraDarkCard {
                AuraNavRow(Icons.Filled.Sensors, "Manage devices", "Pairing & connectors", onClick = onManageDevices)
            }
        }
    }
}

@Composable
private fun InfoRow(label: String, value: String) {
    val p = Aura.palette
    Row(Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 15.dp)) {
        Text(label, style = AuraType.label, color = p.ink.copy(alpha = 0.9f))
        Spacer(Modifier.weight(1f))
        Text(value, style = AuraType.label, color = p.ink.copy(alpha = 0.78f))
    }
}

/** Shared scaffold for the not-yet-in-Mav destinations. */
@Composable
private fun MavPendingScreen(title: String, subtitle: String, body: String, extra: @Composable () -> Unit = {}) {
    AuraScreen {
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .statusBarsPadding()
                .padding(horizontal = Aura.screenMargin)
                .padding(top = 8.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(Aura.sectionGap),
        ) {
            AuraHubHeader(title = title, subtitle = subtitle)
            AuraDarkCard {
                Text(body, style = AuraType.sub, color = Aura.palette.ink.copy(alpha = 0.68f))
            }
            extra()
        }
    }
}

@Composable
fun DevicesScreen(vm: AppViewModel, onUseFileImport: () -> Unit) {
    val state by vm.state.collectAsStateWithLifecycle()
    MavPendingScreen(
        "Devices", "Straps & connectors",
        "Mav loads device connectors from their own signed packages; none are bundled yet. " +
            "Pairing appears here once a connector is installed.",
    ) {
        val snapshot = (state as? MavAppState.Ready)?.snapshot
        AuraDarkCard(padding = 0.dp) {
            Column(Modifier.padding(vertical = 4.dp)) {
                InfoRow("Connection", snapshot?.connectionState ?: "runtime unavailable")
                InfoRow("Device", snapshot?.deviceName ?: "--")
            }
        }
        AuraDarkCard {
            AuraNavRow(Icons.Filled.Storage, "Data sources", "Imports & provenance", onClick = onUseFileImport)
        }
    }
}

@Composable
fun DataSourcesScreen(vm: AppViewModel) {
    MavPendingScreen(
        "Data sources", "Imports & provenance",
        "Every stored sample carries its origin. File import lands with the connector lane; " +
            "nothing here is estimated or backfilled silently.",
    )
}

@Composable
fun WorkoutsScreen(vm: AppViewModel) {
    MavPendingScreen(
        "Workouts", "Sessions & zones",
        "Workouts appear once the core's timeline stores activity sessions from a connector.",
    )
}

@Composable
fun StrengthScreen(onClose: () -> Unit) {
    MavPendingScreen(
        "Strength", "Sets & progression",
        "The strength log arrives with the workout lane.",
    )
}

@Composable
fun InsightsScreen(vm: AppViewModel, onOpenInsightsHub: () -> Unit) {
    MavPendingScreen(
        "Journal", "Behaviours & effects",
        "The morning journal and its effect ranking need day aggregates from the core first.",
    )
}

@Composable
fun InsightsHubScreen(vm: AppViewModel) {
    MavPendingScreen("Insights", "Patterns & effects", "Insights build on stored day history.")
}

@Composable
fun AppleHealthScreen(vm: AppViewModel) {
    MavPendingScreen(
        "Health Connect", "System health data",
        "Health Connect exchange is planned after the core's ingest lane; Mav never uploads anything.",
    )
}

@Composable
fun CoachScreen() {
    MavPendingScreen(
        "Coach", "Your key, your device",
        "The private coach reads your stored days. It unlocks once day aggregates exist.",
    )
}

@Composable
fun SmartAlarmScreen(vm: AppViewModel) {
    MavPendingScreen(
        "Smart alarm", "Wake windows",
        "Alarm scheduling needs the strap transport lane (wrist haptics).",
    )
}

@Composable
fun TrendsScreen(vm: AppViewModel) {
    MavPendingScreen("Trends", "Long-range history", "Trends read stored day history from the core.")
}

@Composable
fun SettingsScreen(
    vm: AppViewModel,
    onOpenTestCentre: () -> Unit,
    onOpenBackupSync: () -> Unit,
    onOpenAutomations: () -> Unit,
    onOpenNotifications: () -> Unit,
) {
    val state by vm.state.collectAsStateWithLifecycle()
    val snapshot = (state as? MavAppState.Ready)?.snapshot
    MavPendingScreen(
        "All settings", "Runtime & storage",
        "Core ${snapshot?.coreVersion ?: "--"} · storage schema ${snapshot?.storageSchema ?: "--"} · " +
            "revision ${snapshot?.revision ?: "--"}",
    ) {
        AuraSectionHeader(title = "Advanced")
        AuraDarkCard(padding = 0.dp) {
            Column(Modifier.padding(vertical = 4.dp)) {
                AuraNavRow(Icons.Filled.Sensors, "Test centre", "Replay & fixtures", onClick = onOpenTestCentre)
                AuraNavRow(Icons.Filled.Storage, "Backup & sync", "Local exports", onClick = onOpenBackupSync)
                AuraNavRow(Icons.Filled.Refresh, "Automations", "Background behaviour", onClick = onOpenAutomations)
                AuraNavRow(Icons.Filled.Refresh, "Notifications", "Alerts & nudges", onClick = onOpenNotifications)
            }
        }
    }
}

@Composable
fun TestCentreScreen(vm: AppViewModel) {
    MavPendingScreen(
        "Test centre", "Replay & diagnostics",
        "Capture replay runs in the core (mav-replay); a host surface for it is planned.",
    )
}

@Composable
fun BackupSyncScreen() {
    MavPendingScreen(
        "Backup & sync", "Local, encrypted",
        "Whole-store backup lands after the storage lane freezes its export format.",
    )
}

@Composable
fun AutomationsScreen(vm: AppViewModel) {
    MavPendingScreen("Automations", "Background behaviour", "Background connection policies arrive with the transport lane.")
}

@Composable
fun NotificationsSettingsScreen(vm: AppViewModel) {
    MavPendingScreen("Notifications", "Alerts & nudges", "Notification routing arrives with the transport lane.")
}
