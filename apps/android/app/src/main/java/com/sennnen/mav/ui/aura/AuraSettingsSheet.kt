package com.sennnen.mav.ui.aura

import com.sennnen.mav.ui.AuraZoneMath
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.MenuBook
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.CloudSync
import androidx.compose.material.icons.filled.FileUpload
import androidx.compose.material.icons.filled.MonitorHeart
import androidx.compose.material.icons.filled.MoveToInbox
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material.icons.filled.Sensors
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.data.AuraDataProtection
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.AppearanceMode
import com.sennnen.mav.ui.AppearancePrefs
import com.sennnen.mav.ui.MavPrefs
import com.sennnen.mav.ui.ProfileStore
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

// The ONE app-wide settings sheet (Android port of Strand/UI/AuraSettingsSheet.swift),
// opened from the top-right cog on every hub — never a tab. Profile · Device ·
// Personal · Notifications · Appearance · Data · everything else · About.
// Rows that still have richer legacy screens push them until their Aura ports land.

@Composable
fun AuraSettingsSheet(
    vm: AppViewModel,
    onDismiss: () -> Unit,
    onOpenDevices: () -> Unit,
    onOpenCoach: () -> Unit,
    onOpenJournal: () -> Unit,
    onOpenHealthConnect: () -> Unit,
    onOpenDataSources: () -> Unit,
    onOpenBackupSync: () -> Unit,
    onOpenAllSettings: () -> Unit,
    onOpenMigrate: () -> Unit = onOpenDataSources,
    onOpenPairing: () -> Unit = onOpenDevices,
    onOpenDiagnostics: () -> Unit = {},
) {
    val p = Aura.palette
    val context = LocalContext.current
    val live by vm.live.collectAsStateWithLifecycle()
    val profile = remember { ProfileStore.from(context) }

    // ProfileStore is plain SharedPreferences — mirror into state so steppers are live.
    var age by remember { mutableStateOf(profile.age) }
    var sex by remember { mutableStateOf(profile.sex) }
    var weightKg by remember { mutableStateOf(profile.weightKg) }
    var heightCm by remember { mutableStateOf(profile.heightCm) }
    var hrMaxOverride by remember { mutableStateOf(profile.hrMaxOverride) }
    var morningCheckIn by remember { mutableStateOf(MavPrefs.morningReportEnabled(context)) }
    var encryptAtRest by remember { mutableStateOf(AuraDataProtection.enabled(context)) }

    AuraSheet(title = "Settings", onDismiss = onDismiss) {
        // MARK: Profile
        SettingsGroup("Profile") {
            StepperRow(
                "Age", "$age",
                dec = { age = max(13, age - 1); profile.age = age },
                inc = { age = min(100, age + 1); profile.age = age },
            )
            GroupDivider()
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 11.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Sex", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                Spacer(Modifier.weight(1f))
                CapsuleSegments(
                    options = listOf("male" to "Male", "female" to "Female", "nonbinary" to "NB"),
                    selected = sex,
                    onSelect = { sex = it; profile.sex = it },
                )
            }
            GroupDivider()
            StepperRow(
                "Weight", "${weightKg.roundToInt()} kg",
                dec = { weightKg = max(30.0, weightKg - 1); profile.weightKg = weightKg },
                inc = { weightKg = min(250.0, weightKg + 1); profile.weightKg = weightKg },
            )
            GroupDivider()
            StepperRow(
                "Height", "${heightCm.roundToInt()} cm",
                dec = { heightCm = max(120.0, heightCm - 1); profile.heightCm = heightCm },
                inc = { heightCm = min(230.0, heightCm + 1); profile.heightCm = heightCm },
            )
            GroupDivider()
            StepperRow(
                "Max HR", if (hrMaxOverride > 0) "$hrMaxOverride" else "auto",
                dec = {
                    hrMaxOverride = max(0, AuraZoneMath.maxHr(age, hrMaxOverride) - 1)
                    profile.hrMaxOverride = hrMaxOverride
                },
                inc = {
                    hrMaxOverride = min(230, AuraZoneMath.maxHr(age, hrMaxOverride) + 1)
                    profile.hrMaxOverride = hrMaxOverride
                },
            )
        }

        // MARK: Device
        SettingsGroup("Device") {
            InfoRow("Wearable", live.advertisingName ?: "Not connected")
            GroupDivider()
            InfoRow("Status", if (live.bonded) "Paired · encrypted" else "Not paired")
            GroupDivider()
            InfoRow(
                "Battery",
                live.batteryPct?.let { "${it.roundToInt()}%${if (live.charging == true) " ⚡" else ""}" } ?: "--",
            )
            GroupDivider()
            AuraNavRow(
                icon = Icons.Filled.Sensors, title = "Find my strap", detail = "Buzz it",
                tint = p.accentInk, onClick = { vm.buzz(loops = 2) },
            )
            GroupDivider()
            AuraNavRow(
                icon = Icons.Filled.Add,
                title = if (live.bonded) "Manage devices" else "Pair a strap",
                onClick = onOpenPairing,
            )
        }

        // MARK: Personal
        SettingsGroup("Personal") {
            AuraNavRow(
                icon = Icons.Filled.AutoAwesome, title = "Coach",
                detail = "Private, bring-your-own-key",
                tint = AuraFamily.EFFORT.glow, onClick = onOpenCoach,
            )
            GroupDivider()
            AuraNavRow(
                icon = Icons.AutoMirrored.Filled.MenuBook, title = "Journal history",
                detail = "Behaviours → recovery",
                tint = AuraFamily.ENERGY.glow, onClick = onOpenJournal,
            )
        }

        // MARK: System health (plan Phase 4: master switch · status · deep links)
        SettingsGroup("Battery") {
            var lowPower by remember { mutableStateOf(AuraLowPowerPrefs.enabled(context)) }
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 11.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text("Battery saver", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                    Text(
                        "Syncs history less often and drops device diagnostics to save strap and " +
                            "phone battery. Live heart rate keeps working.",
                        style = AuraType.caption, color = p.ink.copy(alpha = 0.55f),
                    )
                }
                Switch(
                    checked = lowPower,
                    onCheckedChange = { on ->
                        lowPower = on
                        AuraLowPowerPrefs.setEnabled(context, on)
                        vm.setLowPower(on)
                    },
                    colors = SwitchDefaults.colors(
                        checkedTrackColor = AuraFamily.HEART.glow,
                        checkedThumbColor = Color.Black,
                    ),
                )
            }
        }
        SettingsGroup("System health") {
            val hcAvailable = remember {
                com.sennnen.mav.ingest.HealthConnectImporter.sdkStatus(context) ==
                    androidx.health.connect.client.HealthConnectClient.SDK_AVAILABLE
            }
            var healthSync by remember { mutableStateOf(AuraHealthSyncPrefs.enabled(context)) }
            val hcPermLauncher = androidx.activity.compose.rememberLauncherForActivityResult(
                androidx.health.connect.client.PermissionController.createRequestPermissionResultContract(),
            ) { granted ->
                val ok = granted.any { it in com.sennnen.mav.ingest.HealthConnectImporter.PERMISSIONS }
                healthSync = ok
                AuraHealthSyncPrefs.setEnabled(context, ok)
            }
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 11.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text("System health sync", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                    Text(
                        if (hcAvailable)
                            "Health Connect fills gaps when the wearable is off-wrist. Direct " +
                                "telemetry always wins and is never overwritten."
                        else "Health Connect isn't available on this device.",
                        style = AuraType.caption, color = p.ink.copy(alpha = 0.55f),
                    )
                }
                Switch(
                    checked = healthSync,
                    enabled = hcAvailable,
                    onCheckedChange = { on ->
                        if (on) hcPermLauncher.launch(com.sennnen.mav.ingest.HealthConnectImporter.PERMISSIONS)
                        else { healthSync = false; AuraHealthSyncPrefs.setEnabled(context, false) }
                    },
                    colors = SwitchDefaults.colors(
                        checkedTrackColor = AuraFamily.HEART.glow,
                        checkedThumbColor = Color.Black,
                    ),
                )
            }
            GroupDivider()
            AuraNavRow(
                icon = Icons.Filled.MonitorHeart, title = "Sync status & log",
                detail = "Imports · write-back",
                tint = AuraFamily.HEART.glow, onClick = onOpenHealthConnect,
            )
            GroupDivider()
            AuraNavRow(
                icon = Icons.Filled.Settings, title = "System permissions",
                detail = "Health Connect",
                onClick = {
                    runCatching {
                        context.startActivity(
                            android.content.Intent("androidx.health.ACTION_HEALTH_CONNECT_SETTINGS"),
                        )
                    }
                },
            )
        }

        // MARK: Notifications
        SettingsGroup("Notifications") {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 11.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text("Morning check-in", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                    Text(
                        "A recap after your strap syncs, with a one-tap \"Log how you feel\" action.",
                        style = AuraType.caption, color = p.ink.copy(alpha = 0.55f),
                    )
                }
                Switch(
                    checked = morningCheckIn,
                    onCheckedChange = {
                        morningCheckIn = it
                        MavPrefs.setMorningReportEnabled(context, it)
                    },
                    colors = SwitchDefaults.colors(
                        checkedTrackColor = AuraFamily.ENERGY.glow,
                        checkedThumbColor = Color.Black,
                    ),
                )
            }
        }

        // MARK: Appearance
        SettingsGroup("Appearance") {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 11.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Theme", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                Spacer(Modifier.weight(1f))
                CapsuleSegments(
                    options = AppearanceMode.entries.map { it.storageValue to it.label },
                    selected = AppearancePrefs.mode.storageValue,
                    onSelect = { raw ->
                        AppearancePrefs.set(context, AppearanceMode.fromStorage(raw))
                    },
                )
            }
        }

        // MARK: Data
        SettingsGroup("Data") {
            AuraNavRow(
                icon = Icons.Filled.FileUpload, title = "Export CSV",
                detail = "Portable archive", onClick = onOpenDataSources,
            )
            GroupDivider()
            AuraNavRow(
                icon = Icons.Filled.MoveToInbox, title = "Migrate from the original app",
                detail = "Export zip", tint = AuraFamily.CHARGE.glow, onClick = onOpenMigrate,
            )
            GroupDivider()
            AuraNavRow(
                icon = Icons.Filled.CloudSync, title = "Backup & Sync",
                detail = "Your folder, your files", onClick = onOpenBackupSync,
            )
            GroupDivider()
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 11.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text("Encrypt backups at rest", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                    Text(
                        "Optional hardware encryption for backup snapshots. Encrypted backups only restore on this device. Debug logs stay readable.",
                        style = AuraType.caption, color = p.ink.copy(alpha = 0.55f),
                    )
                }
                Switch(
                    checked = encryptAtRest,
                    onCheckedChange = {
                        encryptAtRest = it
                        AuraDataProtection.setEnabled(context, it)
                    },
                    colors = SwitchDefaults.colors(
                        checkedTrackColor = AuraFamily.VITALS.glow,
                        checkedThumbColor = Color.Black,
                    ),
                )
            }
            GroupDivider()
            AuraNavRow(
                icon = Icons.Filled.Storage, title = "Storage & diagnostics",
                detail = "DB size · integrity", onClick = onOpenDiagnostics,
            )
        }

        // MARK: Everything else (full legacy settings until each port lands)
        SettingsGroup("More") {
            AuraNavRow(
                icon = Icons.Filled.Settings, title = "All settings",
                detail = "Alarms · automations · units · debug", onClick = onOpenAllSettings,
            )
        }

        // MARK: About
        Column(Modifier.padding(horizontal = 4.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("Maverick", style = AuraType.heading(17.sp), color = p.ink)
            Text(
                "Connects directly to approved wearables over Bluetooth. No account, no cloud, nothing ever leaves this device.",
                style = AuraType.sub, color = p.ink.copy(alpha = 0.55f),
            )
        }
    }
}

// MARK: - Group scaffolding

@Composable
private fun SettingsGroup(title: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        AuraSectionHeader(title = title)
        AuraDarkCard(padding = 0.dp) {
            Spacer(Modifier.height(4.dp))
            content()
            Spacer(Modifier.height(4.dp))
        }
    }
}

@Composable
private fun GroupDivider() {
    HorizontalDivider(
        color = Aura.palette.hairline, thickness = 1.dp,
        modifier = Modifier.padding(start = 18.dp),
    )
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

@Composable
private fun StepperRow(label: String, value: String, dec: () -> Unit, inc: () -> Unit) {
    val p = Aura.palette
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 11.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
        Spacer(Modifier.weight(1f))
        Text(value, style = AuraType.number(18.sp), color = p.ink)
        Row(
            Modifier.background(p.ink.copy(alpha = 0.08f), CircleShape),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.size(width = 34.dp, height = 30.dp).clickable(onClick = dec),
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.Filled.Remove, contentDescription = "Decrease $label", tint = p.ink, modifier = Modifier.size(14.dp))
            }
            Box(Modifier.width(1.dp).height(16.dp).background(p.ink.copy(alpha = 0.15f)))
            Box(
                Modifier.size(width = 34.dp, height = 30.dp).clickable(onClick = inc),
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.Filled.Add, contentDescription = "Increase $label", tint = p.ink, modifier = Modifier.size(14.dp))
            }
        }
    }
}

@Composable
private fun CapsuleSegments(
    options: List<Pair<String, String>>,
    selected: String,
    onSelect: (String) -> Unit,
) {
    val p = Aura.palette
    Row(
        Modifier.background(p.ink.copy(alpha = 0.08f), CircleShape).padding(3.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        options.forEach { (raw, label) ->
            val active = raw == selected
            Text(
                label,
                style = AuraType.caption,
                color = if (active) Color.Black else p.ink.copy(alpha = 0.65f),
                modifier = Modifier
                    .background(if (active) p.accent else Color.Transparent, CircleShape)
                    .clickable { onSelect(raw) }
                    .padding(horizontal = 12.dp, vertical = 6.dp),
            )
        }
    }
}
