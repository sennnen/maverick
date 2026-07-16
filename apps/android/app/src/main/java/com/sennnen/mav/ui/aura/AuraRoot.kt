package com.sennnen.mav.ui.aura

import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bedtime
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.LocalFireDepartment
import androidx.compose.material.icons.outlined.GridView
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.AppleHealthScreen
import com.sennnen.mav.ui.AutomationsScreen
import com.sennnen.mav.ui.BackupSyncScreen
import com.sennnen.mav.ui.CoachScreen
import com.sennnen.mav.ui.DataSourcesScreen
import com.sennnen.mav.ui.DevicesScreen
import com.sennnen.mav.ui.InsightsHubScreen
import com.sennnen.mav.ui.InsightsScreen
import com.sennnen.mav.ui.LiveScreen
import com.sennnen.mav.ui.NotificationsSettingsScreen
import com.sennnen.mav.ui.SettingsScreen
import com.sennnen.mav.ui.SmartAlarmScreen
import com.sennnen.mav.ui.TestCentreScreen
import com.sennnen.mav.ui.TrendsScreen
import com.sennnen.mav.ui.WorkoutsScreen
import kotlin.math.abs

// The Aura navigation shell (Android port of StrandiOS/App/RootTabView.swift):
// the four-hub IA — Today · Recovery · Strain · Sleep — behind a floating
// glass tab bar. Settings is never a tab: every hub's top-right cog opens the
// app-wide settings (the Aura sheet lands in P3; the existing Settings screen
// is pushed until then). Legacy screens stay routable while their Aura
// replacements land phase by phase.

/** Calm-easing curve — the app-wide motion token (AuraMotion.ease). */
private val AuraEase = AuraMotion.ease

@Composable
fun AuraRootScreen(viewModel: AppViewModel) {
    AuraTheme {
        // Countdown timer: revive a persisted end instant + hand the engine its strap I/O.
        val timerContext = androidx.compose.ui.platform.LocalContext.current
        LaunchedEffect(Unit) {
            AuraCountdown.ensureLoaded(timerContext)
            // Optional wrist cues respect the master haptics toggle (pairing sheet).
            AuraCountdown.buzz = {
                if (AuraHapticsPrefs.enabled(timerContext)) viewModel.buzzStrapOnce()
            }
            AuraCountdown.stopBuzz = { viewModel.stopHaptics() }
        }
        val nav = rememberNavController()
        var tabIndex by rememberSaveable { mutableIntStateOf(0) }
        val tab = AuraTab.entries[tabIndex.coerceIn(0, AuraTab.entries.size - 1)]
        var showSettings by rememberSaveable { mutableStateOf(false) }
        var showMigrate by rememberSaveable { mutableStateOf(false) }
        var showPairing by rememberSaveable { mutableStateOf(false) }
        var showDiagnostics by rememberSaveable { mutableStateOf(false) }

        // noop:// deep links from actionable notifications (roadmap C2).
        val deepLink by AuraDeepLink.requested.collectAsStateWithLifecycle()
        LaunchedEffect(deepLink) {
            when (deepLink) {
                null -> return@LaunchedEffect
                "journal" -> nav.navigate("journal")
                "recovery" -> {
                    nav.popBackStack("hubs", inclusive = false)
                    tabIndex = AuraTab.RECOVERY.ordinal
                }
                "workouts" -> nav.navigate("workouts")
                "live" -> nav.navigate("live")
                "sleep" -> {
                    nav.popBackStack("hubs", inclusive = false)
                    tabIndex = AuraTab.SLEEP.ordinal
                }
            }
            AuraDeepLink.clear()
        }

        CompositionLocalProvider(
            LocalAuraSwitchTab provides { tabIndex = it.ordinal },
            // The cog opens the ONE app-wide settings sheet (never a tab, roadmap B2).
            LocalAuraOpenSettings provides { showSettings = true },
        ) {
            if (showSettings) {
                AuraSettingsSheet(
                    vm = viewModel,
                    onDismiss = { showSettings = false },
                    onOpenDevices = { showSettings = false; nav.navigate("devices") },
                    onOpenCoach = { showSettings = false; nav.navigate("coach") },
                    onOpenJournal = { showSettings = false; nav.navigate("journal") },
                    onOpenHealthConnect = { showSettings = false; nav.navigate("health_connect") },
                    onOpenDataSources = { showSettings = false; nav.navigate("data_sources") },
                    onOpenBackupSync = { showSettings = false; nav.navigate("backup_sync") },
                    onOpenAllSettings = { showSettings = false; nav.navigate("settings") },
                    onOpenMigrate = { showSettings = false; showMigrate = true },
                    onOpenPairing = { showSettings = false; showPairing = true },
                    onOpenDiagnostics = { showSettings = false; showDiagnostics = true },
                )
            }
            if (showMigrate) {
                AuraMigrateSheet(vm = viewModel, onDismiss = { showMigrate = false })
            }
            if (showPairing) {
                AuraPairingSheet(
                    vm = viewModel,
                    onDismiss = { showPairing = false },
                    onOpenDevices = { showPairing = false; nav.navigate("devices") },
                )
            }
            if (showDiagnostics) {
                AuraDiagnosticsSheet(vm = viewModel, onDismiss = { showDiagnostics = false })
            }
            NavHost(
                navController = nav,
                startDestination = "hubs",
                enterTransition = { fadeIn(tween(240, easing = AuraEase)) },
                exitTransition = { fadeOut(tween(240, easing = AuraEase)) },
                popEnterTransition = { fadeIn(tween(240, easing = AuraEase)) },
                popExitTransition = { fadeOut(tween(240, easing = AuraEase)) },
            ) {
                composable("hubs") {
                    AuraHubsShell(
                        vm = viewModel,
                        tab = tab,
                        onSelect = { tabIndex = it.ordinal },
                        onOpenLive = { nav.navigate("live") },
                        onOpenJournal = { nav.navigate("journal") },
                        onOpenCoach = { nav.navigate("coach") },
                        onOpenReports = { nav.navigate("aura_reports") },
                        onOpenTrends = { nav.navigate("aura_trends") },
                        onOpenAlarm = { nav.navigate("alarm") },
                        onOpenWorkouts = { nav.navigate("workouts") },
                        onOpenStrength = { nav.navigate("strength") },
                    )
                }
                // Legacy destinations, pushed from Aura hubs until their Aura
                // replacements land (P2 workouts/live, P3 journal/coach/alarm/
                // pairing/settings, P4 trends/reports).
                composable("live") {
                    LiveScreen(viewModel, onManageDevices = { nav.navigate("devices") })
                }
                composable("devices") {
                    DevicesScreen(viewModel, onUseFileImport = { nav.navigate("data_sources") })
                }
                composable("data_sources") { DataSourcesScreen(viewModel) }
                composable("workouts") { WorkoutsScreen(viewModel) }
                composable("strength") {
                    com.sennnen.mav.ui.StrengthScreen(onClose = { nav.popBackStack() })
                }
                composable("journal") {
                    InsightsScreen(viewModel, onOpenInsightsHub = { nav.navigate("insights_hub") })
                }
                composable("insights_hub") { InsightsHubScreen(viewModel) }
                composable("health_connect") { AppleHealthScreen(viewModel) }
                composable("coach") { CoachScreen() }
                composable("alarm") { SmartAlarmScreen(viewModel) }
                composable("aura_trends") { AuraTrendsScreen(viewModel, onClose = { nav.popBackStack() }) }
                composable("aura_reports") { AuraReportsScreen(viewModel, onClose = { nav.popBackStack() }) }
                composable("trends") { TrendsScreen(viewModel) }
                composable("settings") {
                    SettingsScreen(
                        viewModel,
                        onOpenTestCentre = { nav.navigate("test_centre") },
                        onOpenBackupSync = { nav.navigate("backup_sync") },
                        onOpenAutomations = { nav.navigate("automations") },
                        onOpenNotifications = { nav.navigate("notifications") },
                    )
                }
                composable("test_centre") { TestCentreScreen(viewModel) }
                composable("backup_sync") { BackupSyncScreen() }
                // Wrist-automation config lost its route when AppRoot was replaced by this shell:
                // SedentaryDetector / InactivityNotifier / NoopNotificationListener kept reading
                // NotifPrefs, but no reachable screen wrote them. Settings is their door again.
                composable("automations") { AutomationsScreen(viewModel) }
                composable("notifications") { NotificationsSettingsScreen(viewModel) }
            }
        }
    }
}

// MARK: - The four-hub pager + floating tab bar

@Composable
private fun AuraHubsShell(
    vm: AppViewModel,
    tab: AuraTab,
    onSelect: (AuraTab) -> Unit,
    onOpenLive: () -> Unit,
    onOpenJournal: () -> Unit,
    onOpenCoach: () -> Unit,
    onOpenReports: () -> Unit,
    onOpenTrends: () -> Unit,
    onOpenAlarm: () -> Unit,
    onOpenWorkouts: () -> Unit,
    onOpenStrength: () -> Unit,
) {
    val flickPx = with(LocalDensity.current) { 60.dp.toPx() }
    val p = Aura.palette
    // Material Scaffold owns the frame: the docked NavigationBar and its insets.
    // Hubs pad their own status bar (the lead glow still bleeds edge-to-edge).
    androidx.compose.material3.Scaffold(
        containerColor = p.bg,
        contentWindowInsets = androidx.compose.foundation.layout.WindowInsets(0, 0, 0, 0),
        bottomBar = { AuraNavBar(selection = tab, onSelect = onSelect) },
    ) { inner ->
        Box(
            Modifier
                .fillMaxSize()
                .padding(inner)
                // Decisive horizontal flick moves between hubs; vertical scrolling
                // wins otherwise (mirrors the iOS shell's simultaneous gesture).
                .pointerInput(tab) {
                    var totalX = 0f
                    detectHorizontalDragGestures(
                        onDragStart = { totalX = 0f },
                        onHorizontalDrag = { _, dragAmount -> totalX += dragAmount },
                        onDragEnd = {
                            if (abs(totalX) > flickPx) {
                                val next = (tab.ordinal + if (totalX < 0) 1 else -1)
                                    .coerceIn(0, AuraTab.entries.size - 1)
                                if (next != tab.ordinal) onSelect(AuraTab.entries[next])
                            }
                        },
                    )
                },
        ) {
            // ~240ms opacity swap between tab roots, calm easing.
            Crossfade(
                targetState = tab,
                animationSpec = tween(240, easing = AuraEase),
                label = "auraHub",
            ) { t ->
                when (t) {
                    AuraTab.TODAY -> AuraTodayScreen(
                        vm,
                        onOpenLive = onOpenLive,
                        onOpenJournal = onOpenJournal,
                        onOpenCoach = onOpenCoach,
                        onOpenReports = onOpenReports,
                        onOpenTrends = onOpenTrends,
                    )
                    AuraTab.RECOVERY -> AuraRecoveryScreen(vm)
                    AuraTab.STRAIN -> AuraStrainScreen(
                        vm,
                        onOpenWorkouts = onOpenWorkouts,
                        onOpenLive = onOpenLive,
                        onOpenStrength = onOpenStrength,
                    )
                    AuraTab.SLEEP -> AuraSleepScreen(vm, onOpenAlarm = onOpenAlarm)
                }
            }
        }
    }
}

// MARK: - Material navigation bar (docked; Aura paint, Starship marks the active hub)

private val AuraTab.icon: ImageVector
    get() = when (this) {
        AuraTab.TODAY -> Icons.Outlined.GridView
        AuraTab.RECOVERY -> Icons.Filled.Bolt
        AuraTab.STRAIN -> Icons.Filled.LocalFireDepartment
        AuraTab.SLEEP -> Icons.Filled.Bedtime
    }

@Composable
fun AuraNavBar(selection: AuraTab, onSelect: (AuraTab) -> Unit) {
    // Colours all come from the THEME now: container = surfaceContainer (titanium card),
    // indicator = secondaryContainer (Starship 22%), selected ink = onSecondaryContainer
    // (accentInk) — auraColorScheme maps every one, so no per-item colour plumbing.
    NavigationBar(tonalElevation = 0.dp) {
        AuraTab.entries.forEach { tab ->
            NavigationBarItem(
                selected = tab == selection,
                onClick = { onSelect(tab) },
                icon = { Icon(tab.icon, contentDescription = null, modifier = Modifier.size(22.dp)) },
                label = { Text(tab.title) },
            )
        }
    }
}
