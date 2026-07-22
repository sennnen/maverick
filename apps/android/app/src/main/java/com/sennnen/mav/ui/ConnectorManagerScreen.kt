package com.sennnen.mav.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bluetooth
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.ErrorOutline
import androidx.compose.material.icons.filled.FileOpen
import androidx.compose.material.icons.filled.GppGood
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.RemoveCircleOutline
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.connector.ConnectorApprovalPhase
import com.sennnen.mav.connector.ConnectorApprovalSummary
import com.sennnen.mav.connector.ConnectorScanDevice
import com.sennnen.mav.ui.aura.Aura
import com.sennnen.mav.ui.aura.AuraDarkCard
import com.sennnen.mav.ui.aura.AuraFamily
import com.sennnen.mav.ui.aura.AuraHubHeader
import com.sennnen.mav.ui.aura.AuraScreen
import com.sennnen.mav.ui.aura.AuraSectionHeader
import com.sennnen.mav.ui.aura.AuraType
import uniffi.mav_ffi.InstalledConnectorRecord
import uniffi.mav_ffi.ConnectorRegistryEntry
import uniffi.mav_ffi.ConnectorLifecycleState

@Composable
fun DevicesScreen(vm: AppViewModel, onChooseConnectorFile: () -> Unit) {
    val manager = vm.connectors
    val phase by manager.phase.collectAsStateWithLifecycle()
    val installed by manager.installed.collectAsStateWithLifecycle()
    val registryEntries by manager.registryEntries.collectAsStateWithLifecycle()
    val registryError by manager.registryError.collectAsStateWithLifecycle()
    val connection by manager.connection.collectAsStateWithLifecycle()
    val discoveredDevices by manager.discoveredDevices.collectAsStateWithLifecycle()
    var remoteUrl by remember { mutableStateOf("") }
    val palette = Aura.palette

    AuraScreen(lead = AuraFamily.HEART) {
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = Aura.screenMargin)
                .padding(top = 16.dp, bottom = 40.dp),
            verticalArrangement = Arrangement.spacedBy(Aura.sectionGap),
        ) {
            AuraHubHeader(title = "Device connectors", subtitle = "Signed, local, replaceable")
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Icon(Icons.Filled.GppGood, contentDescription = null, tint = AuraFamily.HEART.glow)
                Text(
                    "Add the signed connector for your wearable, then approve exactly what it can access.",
                    style = AuraType.sub,
                    color = palette.ink.copy(alpha = 0.72f),
                )
                Text(
                    "Runs locally. No account or cloud upload.",
                    style = AuraType.caption,
                    color = palette.ink.copy(alpha = 0.52f),
                )
            }

            AuraSectionHeader(title = "Import")
            Button(
                onClick = onChooseConnectorFile,
                enabled = manager.managerEnabled,
                modifier = Modifier.fillMaxWidth().height(48.dp),
            ) {
                Icon(Icons.Filled.FileOpen, contentDescription = null)
                Spacer(Modifier.padding(horizontal = 4.dp))
                Text("Choose .mavconn document")
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(
                    value = remoteUrl,
                    onValueChange = { remoteUrl = it },
                    label = { Text("HTTPS connector URL") },
                    singleLine = true,
                    modifier = Modifier.weight(1f).semantics { contentDescription = "Connector HTTPS URL" },
                )
                OutlinedButton(
                    onClick = { manager.importRemote(remoteUrl) },
                    enabled = manager.remoteImportEnabled,
                    modifier = Modifier.height(48.dp),
                ) {
                    Icon(Icons.Filled.Link, contentDescription = null)
                    Text("Inspect")
                }
            }

            ConnectorPhaseCard(phase, manager::approve, manager::cancel)

            registryError?.let {
                StatusCard(
                    icon = { Icon(Icons.Filled.ErrorOutline, contentDescription = null) },
                    title = "Online catalog unavailable",
                    body = "Local .mavconn files still work.",
                )
            }

            if (registryEntries.isNotEmpty()) {
                AuraSectionHeader(title = "Available")
                registryEntries.filterNot { it.revoked }.forEach { entry ->
                    RegistryEntryCard(entry) { manager.importRegistryEntry(entry) }
                }
            }

            connection.connectorId?.let {
                StatusCard(
                    icon = { Icon(Icons.Filled.Bluetooth, contentDescription = null) },
                    title = connection.label,
                    body = buildString {
                        append(it)
                        connection.heartRateBpm?.let { bpm -> append(" · $bpm bpm") }
                        connection.batteryPercent?.let { battery -> append(" · $battery%") }
                        connection.errorMessage?.let { error -> append(" · $error") }
                    },
                )
            }

            if (connection.lifecycle == ConnectorLifecycleState.SCANNING) {
                AuraSectionHeader(title = "Nearby")
                if (discoveredDevices.isEmpty()) {
                    StatusCard(
                        icon = { CircularProgressIndicator(modifier = Modifier.height(22.dp)) },
                        title = "Looking for wearables…",
                        body = "Keep the strap nearby and awake.",
                    )
                } else {
                    discoveredDevices.forEach { device ->
                        ScanDeviceCard(device) { manager.selectDevice(device.id) }
                    }
                }
            }

            if (installed.isNotEmpty()) {
                AuraSectionHeader(title = "Installed")
                installed.forEach { record ->
                    InstalledConnectorCard(
                        record = record,
                        connectedConnectorId = connection.connectorId.takeIf {
                            connection.lifecycle != ConnectorLifecycleState.DISCONNECTED &&
                                connection.lifecycle != ConnectorLifecycleState.FAILED
                        },
                        onConnect = { manager.connect(record) },
                        onDisconnect = manager::disconnect,
                        onRollback = { manager.rollback(record.connectorId) },
                        onRemove = { manager.remove(record) },
                    )
                }
            }

            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("Trust policy", style = AuraType.label, color = palette.ink.copy(alpha = 0.8f))
                Text(
                    if (manager.thirdPartyEnabled) {
                        "Developer build: explicitly trusted third-party publisher keys are allowed. Core signature and revocation checks still apply."
                    } else {
                        "Release build: only configured official publisher keys are accepted. A shared file or content URI never bypasses signature checks."
                    },
                    style = AuraType.caption,
                    color = palette.ink.copy(alpha = 0.5f),
                )
            }
        }
    }
}

@Composable
private fun ScanDeviceCard(device: ConnectorScanDevice, onConnect: () -> Unit) {
    val palette = Aura.palette
    AuraDarkCard {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(device.name, style = AuraType.label, color = palette.ink)
                Text("${device.rssi} dBm", style = AuraType.caption, color = palette.ink.copy(alpha = 0.55f))
            }
            Button(onClick = onConnect) { Text("Use this device") }
        }
    }
}

@Composable
private fun RegistryEntryCard(entry: ConnectorRegistryEntry, onInstall: () -> Unit) {
    val palette = Aura.palette
    AuraDarkCard {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(entry.connectorId, style = AuraType.label, color = palette.ink)
                Text(
                    "v${entry.version} · ${entry.channel} · signed",
                    style = AuraType.caption,
                    color = palette.ink.copy(alpha = 0.55f),
                )
            }
            Button(onClick = onInstall) { Text("Inspect") }
        }
    }
}

@Composable
private fun ConnectorPhaseCard(
    phase: ConnectorApprovalPhase,
    approve: () -> Unit,
    cancel: () -> Unit,
) {
    when (phase) {
        ConnectorApprovalPhase.Idle -> Unit
        ConnectorApprovalPhase.Inspecting -> StatusCard(
            icon = { CircularProgressIndicator(modifier = Modifier.height(22.dp)) },
            title = "Inspecting signature…",
            body = "No connector code runs before approval.",
        )
        is ConnectorApprovalPhase.AwaitingApproval -> ApprovalCard(phase.summary, approve, cancel)
        is ConnectorApprovalPhase.Installing -> StatusCard(
            icon = { CircularProgressIndicator(modifier = Modifier.height(22.dp)) },
            title = "Installing ${phase.summary.displayName}…",
            body = "The signed artifact is being committed atomically.",
        )
        is ConnectorApprovalPhase.Installed -> StatusCard(
            icon = { Icon(Icons.Filled.CheckCircle, contentDescription = null) },
            title = "Connector installed",
            body = phase.connectorId,
        )
        is ConnectorApprovalPhase.Failed -> StatusCard(
            icon = { Icon(Icons.Filled.ErrorOutline, contentDescription = null) },
            title = "Couldn’t use connector",
            body = phase.message,
            action = { OutlinedButton(onClick = cancel) { Text("Dismiss") } },
        )
        is ConnectorApprovalPhase.RolledBack -> StatusCard(
            icon = { Icon(Icons.Filled.History, contentDescription = null) },
            title = "Rolled back safely",
            body = phase.connectorId,
        )
        is ConnectorApprovalPhase.Revoked -> StatusCard(
            icon = { Icon(Icons.Filled.RemoveCircleOutline, contentDescription = null) },
            title = "Connector disabled",
            body = "${phase.connectorId} is no longer trusted.",
        )
    }
}

@Composable
private fun ApprovalCard(summary: ConnectorApprovalSummary, approve: () -> Unit, cancel: () -> Unit) {
    val palette = Aura.palette
    AuraDarkCard {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("Review before installing", style = AuraType.heading(19.sp), color = palette.ink)
            Text(summary.displayName, style = AuraType.display(26.sp), color = palette.ink)
            if (summary.detail.isNotBlank()) {
                Text(summary.detail, style = AuraType.sub, color = palette.ink.copy(alpha = 0.68f))
            }
            ApprovalRow("Publisher", summary.publisherKeyId)
            ApprovalRow("Version", summary.version)
            ApprovalRow("Source", summary.sourceName)
            ApprovalRow("Self-tests", "${summary.fixtureCount} verified fixtures")
            if (summary.permissions.isNotEmpty()) {
                Text("Permissions", style = AuraType.caption, color = palette.ink.copy(alpha = 0.5f))
                summary.permissions.forEach { permission ->
                    Text("✓ $permission", style = AuraType.label, color = palette.ink.copy(alpha = 0.82f))
                }
            }
            if (summary.capabilities.isNotEmpty()) {
                Text(
                    "Data streams: ${summary.capabilities.joinToString()}",
                    style = AuraType.caption,
                    color = palette.ink.copy(alpha = 0.55f),
                )
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                OutlinedButton(onClick = cancel, modifier = Modifier.weight(1f)) { Text("Cancel") }
                Button(onClick = approve, modifier = Modifier.weight(1f)) { Text("Approve & install") }
            }
        }
    }
}

@Composable
private fun ApprovalRow(label: String, value: String) {
    val palette = Aura.palette
    Row(Modifier.fillMaxWidth()) {
        Text(label, style = AuraType.caption, color = palette.ink.copy(alpha = 0.5f))
        Spacer(Modifier.weight(1f))
        Text(value, style = AuraType.caption, color = palette.ink.copy(alpha = 0.82f))
    }
}

@Composable
private fun InstalledConnectorCard(
    record: InstalledConnectorRecord,
    connectedConnectorId: String?,
    onConnect: () -> Unit,
    onDisconnect: () -> Unit,
    onRollback: () -> Unit,
    onRemove: () -> Unit,
) {
    val palette = Aura.palette
    AuraDarkCard {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(Modifier.fillMaxWidth()) {
                Column {
                    Text(record.connectorId, style = AuraType.label, color = palette.ink)
                    Text(
                        "v${record.version} · ${record.publisherKeyId}",
                        style = AuraType.caption,
                        color = palette.ink.copy(alpha = 0.52f),
                    )
                }
                Spacer(Modifier.weight(1f))
                Text(
                    record.disabledReason?.let { "Disabled" } ?: if (record.active) "Active" else "Installed",
                    style = AuraType.caption,
                    color = palette.ink.copy(alpha = 0.64f),
                )
            }
            record.disabledReason?.let {
                Text(it, style = AuraType.caption, color = palette.bad)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val connected = connectedConnectorId == record.connectorId
                Button(
                    onClick = if (connected) onDisconnect else onConnect,
                    enabled = record.active && record.disabledReason == null,
                ) {
                    Icon(Icons.Filled.Bluetooth, contentDescription = null)
                    Text(if (connected) "Disconnect" else "Connect")
                }
                OutlinedButton(onClick = onRollback, enabled = record.active) { Text("Roll back") }
                OutlinedButton(onClick = onRemove) { Text("Remove") }
            }
        }
    }
}

@Composable
private fun StatusCard(
    icon: @Composable () -> Unit,
    title: String,
    body: String,
    action: @Composable (() -> Unit)? = null,
) {
    val palette = Aura.palette
    AuraDarkCard {
        Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            icon()
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(title, style = AuraType.label, color = palette.ink)
                Text(body, style = AuraType.caption, color = palette.ink.copy(alpha = 0.62f))
            }
            action?.invoke()
        }
    }
}
