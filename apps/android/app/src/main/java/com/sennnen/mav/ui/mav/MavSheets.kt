package com.sennnen.mav.ui.mav

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDefaults
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.sennnen.mav.connector.ConnectorApprovalPhase
import com.sennnen.mav.connector.ConnectorConnectionState
import com.sennnen.mav.connector.ConnectorScanDevice
import com.sennnen.mav.ui.AppearanceMode
import java.time.LocalDate
import uniffi.mav_ffi.ConnectorLifecycleState
import uniffi.mav_ffi.InstalledConnectorRecord
import uniffi.mav_ffi.ConnectorRegistryEntry

// The device sheet, settings, and connector management. The iOS twins are MavDeviceSheet.swift,
// MavSettingsSheet.swift and MavConnectorsView.swift.
//
// The device sheet is one tap from every tab and is the ONLY place device state, pairing, device
// controls, or a route to connector management exists. That exclusivity is the point of the lane:
// the old shell had connection in three places and connector management in two, and settings
// repeated both, so nobody could tell which copy was real.

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MavDeviceSheet(
    connection: ConnectorConnectionState,
    installedNames: List<Pair<String, String>>,
    discovered: List<ConnectorScanDevice>,
    syncNote: String?,
    lowPower: Boolean,
    onLowPower: (Boolean) -> Unit,
    onPair: (Int) -> Unit,
    onSelectDevice: (String) -> Unit,
    onDisconnect: () -> Unit,
    onManageConnectors: () -> Unit,
    onDismiss: () -> Unit,
) {
    val palette = MavTheme.palette
    val isScanning = connection.lifecycle == ConnectorLifecycleState.SCANNING
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = palette.canvas,
        contentColor = palette.ink,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = MavTheme.screenMargin),
            verticalArrangement = Arrangement.spacedBy(MavTheme.cardSpacing),
        ) {
            if (connection.connected) {
                MavStatusCard {
                    Text(
                        connection.connectorId ?: "Device",
                        style = MavType.title,
                        color = palette.ink,
                    )
                    Text(
                        connection.label,
                        style = MavType.sub,
                        color = palette.inkSecondary,
                        modifier = Modifier.padding(top = 5.dp),
                    )
                    Row(
                        Modifier.fillMaxWidth().padding(top = 22.dp),
                        horizontalArrangement = Arrangement.spacedBy(14.dp),
                    ) {
                        MavDeviceStat("Battery", connection.batteryPercent?.let { "$it%" } ?: "—", Modifier.weight(1f))
                        MavDeviceStat("Wrist", connection.onWrist?.let { if (it) "On" else "Off" } ?: "—", Modifier.weight(1f))
                        MavDeviceStat("Link", connection.label, Modifier.weight(1f))
                    }
                }

                MavSectionHeader("Live")
                MavTile {
                    Text(
                        connection.heartRateBpm?.let { "$it bpm" } ?: "No sample yet",
                        style = MavType.numeralMedium,
                        color = palette.ink,
                    )
                    Text(
                        if (connection.samplesPersisted == 0L) {
                            "Live only · not saved"
                        } else {
                            "${connection.samplesPersisted} samples saved"
                        },
                        style = MavType.sub,
                        color = palette.inkSecondary,
                        modifier = Modifier.padding(top = 4.dp),
                    )
                }

                if (syncNote != null) {
                    MavSectionHeader("History sync")
                    MavTile { Text(syncNote, style = MavType.body, color = palette.ink) }
                }

                MavSectionHeader("Controls")
                MavTile(padded = false) {
                    MavToggleRow(
                        title = "Battery saver",
                        detail = "Uses less power by syncing history less often.",
                        checked = lowPower,
                        onCheckedChange = onLowPower,
                    )
                    // Connector-declared controls (ADR-031) render here, from `device-controls/v1`.
                    // The core does not publish that block yet, so the section holds only the
                    // host-owned row rather than an empty heading promising something absent.
                }

                Row(
                    Modifier.fillMaxWidth().padding(top = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    MavWideButton("Disconnect", Modifier.weight(1f)) { onDisconnect() }
                    MavWideButton("Forget device", Modifier.weight(1f), destructive = true) {
                        onDisconnect()
                    }
                }
            } else {
                MavStatusCard {
                    Text(
                        if (isScanning) "Scanning for devices" else "Connect a device",
                        style = MavType.title,
                        color = palette.ink,
                    )
                    Text(
                        if (installedNames.isEmpty()) {
                            "No connector is installed yet. A connector is the signed driver that " +
                                "knows how to talk to your strap — install one and it appears here."
                        } else if (isScanning) {
                            "Choose your strap under Nearby devices. Previously paired devices stay " +
                                "available even when another app has stopped their advertising."
                        } else {
                            "Choose the device type below to start a Bluetooth scan."
                        },
                        style = MavType.body,
                        color = palette.inkSecondary,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                    connection.errorMessage?.let {
                        Text(
                            it,
                            style = MavType.sub,
                            color = destructiveInk(),
                            modifier = Modifier.padding(top = 8.dp),
                        )
                    }
                }

                if (installedNames.isNotEmpty()) {
                    MavSectionHeader("Start a scan")
                    MavTile(padded = false) {
                        installedNames.forEachIndexed { index, (name, version) ->
                            if (index > 0) MavDivider()
                            MavNavRow(name, "Tap to scan · version $version") { onPair(index) }
                        }
                    }
                }

                if (discovered.isNotEmpty()) {
                    MavSectionHeader("Nearby devices")
                    MavTile(padded = false) {
                        discovered.sortedByDescending { it.rssi }.forEachIndexed { index, device ->
                            if (index > 0) MavDivider()
                            MavNavRow(
                                device.name,
                                if (device.paired) "Paired in Android · tap to connect"
                                else "Signal ${device.rssi} dBm",
                            ) {
                                onSelectDevice(device.id)
                            }
                        }
                    }
                } else if (isScanning) {
                    MavSectionHeader("Nearby devices")
                    MavTile {
                        Text("Searching…", style = MavType.body, color = palette.ink)
                        Text(
                            "Keep the strap close and wake it. Results appear here automatically.",
                            style = MavType.sub,
                            color = palette.inkSecondary,
                            modifier = Modifier.padding(top = 4.dp),
                        )
                    }
                }
            }

            MavSectionHeader("Connectors")
            MavTile(padded = false) {
                MavNavRow(
                    "Manage connectors",
                    "${installedNames.size} installed",
                    onManageConnectors,
                )
            }

            Box(Modifier.padding(bottom = 24.dp))
        }
    }
}

@Composable
private fun MavDeviceStat(key: String, value: String, modifier: Modifier = Modifier) {
    val palette = MavTheme.palette
    Column(modifier) {
        Text(key, style = MavType.caption, color = palette.inkSecondary)
        Text(value, style = MavType.numeralSmall, color = palette.ink, maxLines = 1)
    }
}

// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

/**
 * Settings holds what is genuinely a preference, and nothing else.
 *
 * No device row, no pairing entry, no connector row, no battery saver: all four live in the device
 * sheet, one tap from every tab.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MavSettingsSheet(
    appearance: AppearanceMode,
    onAppearance: (AppearanceMode) -> Unit,
    profileSummary: String,
    scoredDays: Int,
    onProfile: () -> Unit,
    onJournal: () -> Unit,
    onDiagnostics: () -> Unit,
    onDismiss: () -> Unit,
) {
    val palette = MavTheme.palette
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = palette.canvas,
        contentColor = palette.ink,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = MavTheme.screenMargin),
            verticalArrangement = Arrangement.spacedBy(MavTheme.cardSpacing),
        ) {
            MavSectionHeader("You")
            MavTile(padded = false) {
                MavNavRow("Body profile", profileSummary, onProfile)
                MavDivider()
                MavNavRow("Journal", "What you log against your days", onJournal)
            }

            MavSectionHeader("Appearance")
            MavTile {
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                    AppearanceMode.entries.forEachIndexed { index, mode ->
                        SegmentedButton(
                            selected = appearance == mode,
                            onClick = { onAppearance(mode) },
                            shape = SegmentedButtonDefaults.itemShape(
                                index, AppearanceMode.entries.size,
                            ),
                            label = {
                                Text(
                                    mode.name.lowercase().replaceFirstChar { it.uppercase() },
                                    style = MavType.caption,
                                )
                            },
                        )
                    }
                }
            }

            MavSectionHeader("Data")
            MavTile(padded = false) {
                MavRow("Scored days") {
                    Text("$scoredDays", style = MavType.label, color = palette.inkSecondary)
                }
                MavDivider()
                MavNavRow("Diagnostics", "Connection and data details", onDiagnostics)
            }

            MavSectionHeader("About")
            MavTile {
                Text("Maverick", style = MavType.title, color = palette.ink)
                Text(
                    "Every byte of decoding and analytics runs on this device. Nothing leaves it.",
                    style = MavType.sub,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 7.dp),
                )
            }

            Box(Modifier.padding(bottom = 24.dp))
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MavProfileScreen(
    sex: String,
    onSex: (String) -> Unit,
    age: Int,
    onAge: (Int) -> Unit,
    weightKg: Double,
    onWeightKg: (Double) -> Unit,
    heightCm: Double,
    onHeightCm: (Double) -> Unit,
    maxHrOverride: Int,
    effectiveMaxHr: Int,
    onMaxHrOverride: (Int) -> Unit,
    onBack: () -> Unit,
) {
    val palette = MavTheme.palette
    MavDetailScaffold("Body profile", onBack) {
        Text(
            "Used for personal zones and cycle insights. Stored only on this phone.",
            style = MavType.body,
            color = palette.inkSecondary,
            modifier = Modifier.padding(vertical = 10.dp),
        )

        MavTile {
            Text("Sex", style = MavType.label, color = palette.ink)
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(top = 8.dp)) {
                listOf("female", "male").forEachIndexed { index, option ->
                    SegmentedButton(
                        selected = sex.equals(option, ignoreCase = true),
                        onClick = { onSex(option) },
                        shape = SegmentedButtonDefaults.itemShape(index, 2),
                        label = {
                            Text(
                                option.replaceFirstChar { it.uppercase() },
                                style = MavType.caption,
                            )
                        },
                    )
                }
            }
            if (sex.equals("female", ignoreCase = true)) {
                Text(
                    "Cycle insights appear automatically.",
                    style = MavType.sub,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 10.dp),
                )
            }
        }

        MavTile(padded = false) {
            MavProfileStepper(
                "Age",
                "$age years",
                { onAge((age - 1).coerceAtLeast(5)) },
                { onAge((age + 1).coerceAtMost(120)) },
            )
            MavDivider()
            MavProfileStepper(
                "Weight",
                "${weightKg.formatProfile()} kg",
                { onWeightKg((weightKg - 0.5).coerceAtLeast(20.0)) },
                { onWeightKg((weightKg + 0.5).coerceAtMost(300.0)) },
            )
            MavDivider()
            MavProfileStepper(
                "Height",
                "${heightCm.toInt()} cm",
                { onHeightCm((heightCm - 1).coerceAtLeast(90.0)) },
                { onHeightCm((heightCm + 1).coerceAtMost(250.0)) },
            )
            MavDivider()
            MavProfileStepper(
                "Max heart rate",
                if (maxHrOverride == 0) "Automatic ($effectiveMaxHr)" else "$maxHrOverride bpm",
                { onMaxHrOverride((maxHrOverride - 1).coerceAtLeast(0)) },
                {
                    onMaxHrOverride(
                        if (maxHrOverride == 0) effectiveMaxHr
                        else (maxHrOverride + 1).coerceAtMost(230),
                    )
                },
            )
        }
    }
}

@Composable
private fun MavProfileStepper(
    title: String,
    value: String,
    onMinus: () -> Unit,
    onPlus: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = MavTheme.tilePadding, vertical = 7.dp),
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
    ) {
        Text(title, style = MavType.label, color = MavTheme.palette.ink, modifier = Modifier.weight(1f))
        IconButton(onClick = onMinus) {
            Icon(MavIcons.chevronLeft, contentDescription = "Decrease $title")
        }
        Text(value, style = MavType.label, color = MavTheme.palette.inkSecondary)
        IconButton(onClick = onPlus) {
            Icon(MavIcons.chevronRight, contentDescription = "Increase $title")
        }
    }
}

private fun Double.formatProfile(): String =
    if (this % 1.0 == 0.0) toInt().toString() else "%.1f".format(this)

// ---------------------------------------------------------------------------------------------
// Connectors
// ---------------------------------------------------------------------------------------------

/**
 * Connector management, reachable from exactly one row — in the device sheet.
 *
 * The approval card is a security surface. This lane restyles it and does not alter a single fact
 * it states.
 */
@Composable
fun MavConnectorsScreen(
    phase: ConnectorApprovalPhase,
    installed: List<InstalledConnectorRecord>,
    registryEntries: List<ConnectorRegistryEntry>,
    registryError: String?,
    onImport: () -> Unit,
    onImportRegistry: (ConnectorRegistryEntry) -> Unit,
    onApprove: () -> Unit,
    onCancel: () -> Unit,
    onConnect: (Int) -> Unit,
    onRollback: (String) -> Unit,
    onRemove: (InstalledConnectorRecord) -> Unit,
    onBack: () -> Unit,
) {
    val palette = MavTheme.palette
    var openMenuIndex by remember { mutableStateOf<Int?>(null) }
    MavDetailScaffold("Connectors", onBack) {
        when (phase) {
            is ConnectorApprovalPhase.Idle -> {
                MavTile {
                    Text("Signed drivers", style = MavType.title, color = palette.ink)
                    Text(
                        "A connector lets Maverick read one wearable. Every connector is signed " +
                            "and installed only after you approve it.",
                        style = MavType.body,
                        color = palette.inkSecondary,
                        modifier = Modifier.padding(top = 7.dp),
                    )
                }

                MavPrimaryButton("Import a connector", "From a file", onImport)

                MavSectionHeader("Installed")
                if (installed.isEmpty()) {
                    Text(
                        "No connectors installed.",
                        style = MavType.body,
                        color = palette.inkSecondary,
                        modifier = Modifier.padding(vertical = 8.dp),
                    )
                } else {
                    MavTile(padded = false) {
                        installed.forEachIndexed { index, record ->
                            if (index > 0) MavDivider()
                            MavRow(
                                record.connectorId,
                                "Version ${record.version}",
                                trailing = {
                                    Box {
                                        IconButton(onClick = { openMenuIndex = index }) {
                                            Icon(MavIcons.more, contentDescription = "Actions for ${record.connectorId}")
                                        }
                                        DropdownMenu(
                                            expanded = openMenuIndex == index,
                                            onDismissRequest = { openMenuIndex = null },
                                        ) {
                                            DropdownMenuItem(
                                                text = { Text("Connect") },
                                                onClick = {
                                                    openMenuIndex = null
                                                    onConnect(index)
                                                },
                                            )
                                            DropdownMenuItem(
                                                text = { Text("Roll back") },
                                                onClick = {
                                                    openMenuIndex = null
                                                    onRollback(record.connectorId)
                                                },
                                            )
                                            DropdownMenuItem(
                                                text = { Text("Remove") },
                                                onClick = {
                                                    openMenuIndex = null
                                                    onRemove(record)
                                                },
                                            )
                                        }
                                    }
                                },
                            )
                        }
                    }
                }

                if (registryEntries.isNotEmpty()) {
                    MavSectionHeader("Available")
                    MavTile(padded = false) {
                        registryEntries.forEachIndexed { index, entry ->
                            if (index > 0) MavDivider()
                            MavNavRow(entry.connectorId, "Version ${entry.version}") {
                                onImportRegistry(entry)
                            }
                        }
                    }
                }

                if (registryError != null) {
                    MavUnavailableCard("Registry", registryError)
                }
            }

            is ConnectorApprovalPhase.Inspecting ->
                MavTile { Text("Inspecting the artifact", style = MavType.body, color = palette.ink) }

            is ConnectorApprovalPhase.AwaitingApproval -> {
                val summary = phase.summary
                MavStatusCard {
                    Text("Approve this connector?", style = MavType.title, color = palette.ink)
                    Text(
                        summary.displayName,
                        style = MavType.title,
                        color = palette.ink,
                        modifier = Modifier.padding(top = 9.dp),
                    )
                    Text(
                        "${summary.connectorId} · version ${summary.version}\n" +
                            "Signed by ${summary.publisherKeyId}",
                        style = MavType.sub,
                        color = palette.inkSecondary,
                        modifier = Modifier.padding(top = 7.dp),
                    )
                }
                if (summary.capabilities.isNotEmpty()) {
                    MavSectionHeader("It will be able to")
                    MavTile { MavFlowRow(summary.capabilities) { MavChip(it) } }
                }
                if (summary.permissions.isNotEmpty()) {
                    MavSectionHeader("It is asking for")
                    MavTile { MavFlowRow(summary.permissions) { MavChip(it) } }
                }
                Row(
                    Modifier.fillMaxWidth().padding(top = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    MavWideButton("Cancel", Modifier.weight(1f)) { onCancel() }
                    MavWideButton(
                        title = "Approve",
                        modifier = Modifier.weight(1f),
                        prominent = true,
                    ) { onApprove() }
                }
            }

            is ConnectorApprovalPhase.Installing ->
                MavTile {
                    Text(
                        "Installing ${phase.summary.displayName}",
                        style = MavType.body,
                        color = palette.ink,
                    )
                }

            is ConnectorApprovalPhase.Installed ->
                MavOutcome("Installed", phase.connectorId, onCancel)

            is ConnectorApprovalPhase.Failed ->
                MavOutcome("Import failed", phase.message, onCancel)

            is ConnectorApprovalPhase.RolledBack ->
                MavOutcome("Rolled back", phase.connectorId, onCancel)

            is ConnectorApprovalPhase.Revoked ->
                MavOutcome("Revoked", phase.connectorId, onCancel)
        }
    }
}

@Composable
private fun MavOutcome(title: String, detail: String, onDone: () -> Unit) {
    val palette = MavTheme.palette
    MavStatusCard {
        Text(title, style = MavType.title, color = palette.ink)
        Text(
            detail,
            style = MavType.body,
            color = palette.inkSecondary,
            modifier = Modifier.padding(top = 7.dp),
        )
    }
    MavWideButton("Done", Modifier.fillMaxWidth().padding(top = 12.dp)) { onDone() }
}

// ---------------------------------------------------------------------------------------------
// The calendar behind the date title
// ---------------------------------------------------------------------------------------------

/**
 * A real Material 3 [DatePicker] in its dialog. Stepping a day at a time is fine for yesterday and
 * useless for last March.
 *
 * The future holds no recorded days, so it cannot be selected — the constraint is stated to the
 * picker rather than checked afterwards, so the disabled days are visibly disabled.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MavDayPickerDialog(day: LocalDate, onPick: (LocalDate) -> Unit, onDismiss: () -> Unit) {
    val today = LocalDate.now()
    val state = rememberDatePickerState(
        initialSelectedDateMillis = day.toEpochDay() * 86_400_000L,
        selectableDates = object : SelectableDates {
            override fun isSelectableDate(utcTimeMillis: Long): Boolean =
                utcTimeMillis / 86_400_000L <= today.toEpochDay()

            override fun isSelectableYear(year: Int): Boolean = year <= today.year
        },
    )
    DatePickerDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            TextButton(
                onClick = {
                    state.selectedDateMillis?.let { onPick(LocalDate.ofEpochDay(it / 86_400_000L)) }
                    onDismiss()
                },
            ) { Text("Done", style = MavType.label) }
        },
        dismissButton = {
            TextButton(
                onClick = {
                    onPick(today)
                    onDismiss()
                },
            ) { Text("Jump to today", style = MavType.label) }
        },
        colors = DatePickerDefaults.colors(containerColor = MavTheme.palette.surface),
    ) {
        DatePicker(state = state, title = null)
    }
}
