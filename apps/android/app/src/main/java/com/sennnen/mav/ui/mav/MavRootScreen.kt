package com.sennnen.mav.ui.mav

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.collectAsState
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import com.sennnen.mav.BuildConfig
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.AppearanceMode
import com.sennnen.mav.ui.AppearancePrefs
import com.sennnen.mav.ui.ProfileStore
import com.sennnen.mav.ui.AuraZoneMath
import com.sennnen.mav.data.DailyMetric
import java.time.Instant

// The one composable MainActivity sets. Everything below it is a pure function of read models.
//
// Navigation is a `when` over a single destination value rather than a NavHost: the app has three
// tabs and six pushed screens, and a navigation graph for that is more machinery than it saves.
// Predictive back still works because every pushed screen sits under a BackHandler.

@Composable
fun MavRootScreen(viewModel: AppViewModel, onChooseConnectorFile: () -> Unit) {
    MavTheme {
        val context = LocalContext.current
        val shell = rememberMavShellState()

        val snapshot by viewModel.dailySnapshot.collectAsState()
        val syncNote by viewModel.syncNote.collectAsState()
        val recentDays by viewModel.recentDays.collectAsState()
        val workouts by viewModel.workouts.collectAsState()
        val activeWorkout by viewModel.activeWorkout.collectAsState()
        val bpm by viewModel.bpm.collectAsState()
        val connection by viewModel.connectors.connection.collectAsState()
        val installed by viewModel.connectors.installed.collectAsState()
        val registryEntries by viewModel.connectors.registryEntries.collectAsState()
        val discovered by viewModel.connectors.discoveredDevices.collectAsState()
        val phase by viewModel.connectors.phase.collectAsState()
        val registryError by viewModel.connectors.registryError.collectAsState()
        val ecgCapabilities by viewModel.connectors.ecgCapabilities.collectAsState()
        val ecgCapture by viewModel.connectors.ecgCapture.collectAsState()
        val ecgResults by viewModel.connectors.ecgResults.collectAsState()
        val ecgError by viewModel.connectors.ecgError.collectAsState()
        val analytics by viewModel.analytics.collectAsState()
        // Null until a pass has produced signals, which is what hides the Today row: a link to a
        // screen that can only say "nothing has run" is a link to an apology.
        val analyticsSummary = if (analytics.signals.isEmpty() && !analytics.working) {
            null
        } else {
            MavSignalCopy.rowDetail(context, analytics)
        }

        var cycleStarts by remember { mutableStateOf(MavCycleLog.load(context)) }
        var journalAnswers by remember {
            mutableStateOf(MavJournalLog.load(context, shell.dayKey))
        }
        var lowPower by remember { mutableStateOf(false) }

        // Debug only, and only while nothing real has arrived: seed the fixture so the layout can
        // be judged without a strap. Every surface it feeds is badged.
        var fixtureDays by remember { mutableStateOf<List<uniffi.mav_ffi.DailySnapshotReport>>(emptyList()) }
        // Seed when the core has produced no *values*, not merely no snapshot. A real snapshot
        // whose every analytic is unavailable is the normal state with no strap in the room, and it
        // leaves nothing on screen to judge. Matches the iOS guard in MaverickApp.swift.
        LaunchedEffect(connection.connected) {
            if (BuildConfig.MAV_SHOW_SYNTHETIC_DATA && !connection.connected && fixtureDays.isEmpty()) {
                fixtureDays = MavDebugFixture.snapshots()
            }
        }

        val usingFixture =
            BuildConfig.MAV_SHOW_SYNTHETIC_DATA && !connection.connected && fixtureDays.isNotEmpty()
        // When the fixture is in use it must actually be what is drawn. Preferring the real
        // snapshot here badged the screen SAMPLE while rendering the core's own barren day, which
        // is the worst of both: a warning that does not match what is on screen.
        val effectiveSnapshot = if (usingFixture) fixtureDays.lastOrNull() else snapshot
        val effectiveDays: List<DailyMetric> = if (usingFixture) {
            fixtureDays.map { day ->
                DailyMetric(
                    deviceId = "fixture",
                    day = day.day,
                    restingHr = null,
                    avgHrv = day.hrv?.rmssdMs,
                    hrvLabel = day.hrv?.label,
                )
            }
        } else {
            recentDays
        }
        val effectiveWorkouts = if (usingFixture && workouts.isEmpty()) {
            MavDebugFixture.workouts(shell.day)
        } else {
            workouts
        }
        val effectiveConnection = if (usingFixture) {
            connection.copy(
                connectorId = "whoop5",
                connected = true,
                label = "Streaming",
                heartRateBpm = 64,
                batteryPercent = 41,
                onWrist = true,
            )
        } else {
            connection
        }

        // Cycle surfaces follow from the profile. Asking someone to state their sex and then asking
        // again, elsewhere, whether they want the feature that follows from it is a question the
        // app already knows the answer to.
        val profile = remember(context) {
            ProfileStore(context.getSharedPreferences("mav_profile", android.content.Context.MODE_PRIVATE))
        }
        // Held in state as well as in prefs so the sheet recomposes the moment either changes.
        var profileSex by remember(profile) { mutableStateOf(profile.sex) }
        var profileAge by remember(profile) { mutableStateOf(profile.age) }
        var profileWeight by remember(profile) { mutableStateOf(profile.weightKg) }
        var profileHeight by remember(profile) { mutableStateOf(profile.heightCm) }
        var profileMaxHr by remember(profile) { mutableStateOf(profile.hrMaxOverride) }
        val tracksCycle = profileSex.equals("female", ignoreCase = true)
        val rows = MavMetricMapper.rows(effectiveSnapshot, tracksCycle || usingFixture)
        val effectiveCycleStarts = if (usingFixture && cycleStarts.isEmpty()) {
            listOf(127L, 98L, 70L, 42L, 14L)
                .map { java.time.LocalDate.now().minusDays(it).toString() }
        } else {
            cycleStarts
        }

        BackHandler(enabled = shell.destination != MavDestination.None) {
            shell.destination = MavDestination.None
        }

        AnimatedContent(
            targetState = shell.destination,
            transitionSpec = {
                val forward = targetState != MavDestination.None
                (
                    slideInHorizontally(tween(240, easing = FastOutSlowInEasing)) { width ->
                        if (forward) width / 8 else -width / 8
                    } + fadeIn(tween(180))
                ) togetherWith (
                    slideOutHorizontally(tween(200, easing = FastOutSlowInEasing)) { width ->
                        if (forward) -width / 12 else width / 12
                    } + fadeOut(tween(140))
                )
            },
            label = "Screen",
        ) { destination ->
        when (destination) {
            is MavDestination.Metric -> {
                val metric = MavMetric.named(destination.metricId)
                if (metric == null) {
                    shell.destination = MavDestination.None
                } else if (metric.group == MavMetricGroup.CYCLE) {
                    MavCycleScreen(
                        starts = effectiveCycleStarts,
                        onLogToday = {
                            cycleStarts = MavCycleLog.logStart(context, java.time.LocalDate.now().toString())
                        },
                        onRemove = { cycleStarts = MavCycleLog.removeStart(context, it) },
                        onBack = { shell.destination = MavDestination.None },
                    )
                } else {
                    MavMetricDetailScreen(
                        metric = metric,
                        snapshot = effectiveSnapshot,
                        history = effectiveDays,
                        liveBpm = if (usingFixture) 64 else bpm,
                        connected = effectiveConnection.connected,
                        deviceName = effectiveConnection.connectorId,
                        usingFixture = usingFixture,
                        onBack = { shell.destination = MavDestination.None },
                    )
                }
            }

            is MavDestination.Cycle -> MavCycleScreen(
                starts = effectiveCycleStarts,
                onLogToday = {
                    cycleStarts = MavCycleLog.logStart(context, java.time.LocalDate.now().toString())
                },
                onRemove = { cycleStarts = MavCycleLog.removeStart(context, it) },
                onBack = { shell.destination = MavDestination.None },
            )

            is MavDestination.Connectors -> MavConnectorsScreen(
                phase = phase,
                installed = installed,
                registryEntries = registryEntries,
                registryError = registryError,
                onImport = onChooseConnectorFile,
                onImportRegistry = { viewModel.connectors.importRegistryEntry(it) },
                onApprove = { viewModel.connectors.approve() },
                onCancel = { viewModel.connectors.cancel() },
                onConnect = { index -> installed.getOrNull(index)?.let { viewModel.connectors.connect(it) } },
                onRollback = { viewModel.connectors.rollback(it) },
                onRemove = { viewModel.connectors.remove(it) },
                onBack = { shell.destination = MavDestination.None },
            )

            is MavDestination.Reports -> MavReportsScreen(
                days = effectiveDays,
                onBack = { shell.destination = MavDestination.None },
            )

            is MavDestination.Analytics -> MavAnalyticsScreen(
                viewModel = viewModel,
                onBack = { shell.destination = MavDestination.None },
            )

            is MavDestination.Ecg -> {
                // A finished analysis opens its own result. Landing back on the start card and
                // leaving the wearer to spot a new history row is how a completed capture read as
                // nothing having happened.
                val finished = ecgCapture?.takeIf { it.phase == "result" }?.captureId
                LaunchedEffect(finished) {
                    if (finished != null && ecgResults.any { it.captureId == finished }) {
                        shell.destination = MavDestination.EcgResult(finished)
                    }
                }
                MavEcgScreen(
                    capabilities = ecgCapabilities,
                    capture = ecgCapture,
                    results = ecgResults,
                    error = ecgError,
                    loadPayload = viewModel.connectors::ecgReportPayload,
                    onStart = viewModel.connectors::startEcgCapture,
                    onStop = viewModel.connectors::stopEcgCapture,
                    onOpenResult = { shell.destination = MavDestination.EcgResult(it) },
                    onBack = { shell.destination = MavDestination.None },
                )
            }

            is MavDestination.EcgResult -> {
                val result = ecgResults.firstOrNull { it.captureId == destination.captureId }
                if (result == null) {
                    LaunchedEffect(destination) { shell.destination = MavDestination.Ecg }
                } else {
                    MavEcgResultScreen(
                        result = result,
                        loadPayload = viewModel.connectors::ecgReportPayload,
                        onRemove = {
                            viewModel.connectors.removeEcgResult(it)
                            shell.destination = MavDestination.Ecg
                        },
                        onBack = { shell.destination = MavDestination.Ecg },
                    )
                }
            }

            is MavDestination.Diagnostics -> MavDiagnosticsScreen(
                days = effectiveDays,
                connection = effectiveConnection,
                snapshot = effectiveSnapshot,
                onBack = { shell.destination = MavDestination.None },
            )

            is MavDestination.WorkoutStart -> MavWorkoutStartScreen(
                onConfigure = { sport ->
                    shell.destination = MavDestination.WorkoutConfig(sport.name)
                },
                onStrength = { shell.destination = MavDestination.Strength },
                onBack = { shell.destination = MavDestination.None },
            )

            is MavDestination.WorkoutConfig -> {
                val sport = MavSportCatalog.sport(destination.sport)
                // A stored destination naming a sport the catalogue no longer has is a bad state,
                // not a screen. Returning to the list is the honest recovery.
                if (sport == null) {
                    LaunchedEffect(destination) { shell.destination = MavDestination.WorkoutStart }
                } else {
                    MavWorkoutConfigScreen(
                        sport = sport,
                        deviceName = effectiveConnection.label,
                        onStart = { name, _ ->
                            viewModel.startWorkout(name)
                            shell.destination = MavDestination.WorkoutLive
                        },
                        onBack = { shell.destination = MavDestination.WorkoutStart },
                    )
                }
            }

            is MavDestination.WorkoutLive -> MavLiveWorkoutScreen(
                active = activeWorkout,
                bpm = bpm,
                connected = effectiveConnection.connected,
                onStop = {
                    viewModel.stopWorkout()
                    shell.destination = MavDestination.None
                },
                onBack = { shell.destination = MavDestination.None },
            )

            is MavDestination.Strength ->
                MavStrengthScreen(
                    usingFixture = usingFixture,
                    onBack = { shell.destination = MavDestination.None },
                )

            is MavDestination.Journal -> MavJournalScreen(
                day = shell.dayKey,
                answers = journalAnswers,
                onToggle = {
                    journalAnswers = MavJournalLog.toggle(context, shell.dayKey, it)
                },
                onBack = { shell.destination = MavDestination.None },
            )

            is MavDestination.Profile -> MavProfileScreen(
                sex = profileSex,
                onSex = {
                    profile.sex = it
                    profileSex = it
                },
                age = profileAge,
                onAge = {
                    profile.age = it
                    profileAge = profile.age
                },
                weightKg = profileWeight,
                onWeightKg = {
                    profile.weightKg = it
                    profileWeight = profile.weightKg
                },
                heightCm = profileHeight,
                onHeightCm = {
                    profile.heightCm = it
                    profileHeight = profile.heightCm
                },
                maxHrOverride = profileMaxHr,
                effectiveMaxHr = profile.hrMax,
                onMaxHrOverride = {
                    profile.hrMaxOverride = it
                    profileMaxHr = profile.hrMaxOverride
                },
                onBack = { shell.destination = MavDestination.None },
            )

            // The atmosphere sits behind the entire shell — under the top bar and the navigation
            // bar, and outside the tab switcher, so it stays put while tabs slide across it.
            MavDestination.None -> Box(Modifier.fillMaxSize()) {
                MavAtmosphere(Modifier.matchParentSize())
                Scaffold(
                containerColor = Color.Transparent,
                topBar = {
                    MavTopBar(
                        shell = shell,
                        batteryPercent = effectiveConnection.batteryPercent,
                        connected = effectiveConnection.connected,
                        deviceName = effectiveConnection.connectorId,
                        onSettings = { shell.showSettings = true },
                        onDevice = { shell.showDevice = true },
                    )
                },
                bottomBar = { MavBottomBar(shell.tab) { shell.tab = it } },
                ) { padding ->
                AnimatedContent(
                    targetState = shell.tab,
                    modifier = Modifier.fillMaxSize().padding(padding),
                    transitionSpec = {
                        val direction = if (targetState.ordinal > initialState.ordinal) 1 else -1
                        (
                            slideInHorizontally(tween(240, easing = FastOutSlowInEasing)) {
                                direction * it / 10
                            } + fadeIn(tween(190))
                        ) togetherWith (
                            slideOutHorizontally(tween(180, easing = FastOutSlowInEasing)) {
                                -direction * it / 14
                            } + fadeOut(tween(130))
                        )
                    },
                    label = "Tab",
                ) { tab ->
                        when (tab) {
                            MavTab.TODAY -> MavTodayScreen(
                                rows = rows,
                                snapshot = effectiveSnapshot,
                                syncNote = syncNote,
                                workouts = effectiveWorkouts.filter {
                                    Instant.ofEpochSecond(it.startTs)
                                        .atZone(java.time.ZoneId.systemDefault())
                                        .toLocalDate() == shell.day
                                },
                                usingFixture = usingFixture,
                                dayKey = shell.dayKey,
                                onOpenMetric = { shell.destination = MavDestination.Metric(it.id) },
                                onOpenReports = { shell.destination = MavDestination.Reports },
                                onOpenDiagnostics = { shell.destination = MavDestination.Diagnostics },
                                analyticsSummary = analyticsSummary,
                                onOpenAnalytics = { shell.destination = MavDestination.Analytics },
                            )

                            MavTab.VITALS -> MavVitalsScreen(
                                rows = rows,
                                usingFixture = usingFixture,
                                showEcg = ecgCapabilities.any { it.stream == "ecg" } ||
                                    ecgResults.isNotEmpty() ||
                                    ecgCapture != null,
                                ecgDetail = ecgCapture?.let { ecgCaptureTitle(it.phase) }
                                    ?: ecgResults.firstOrNull()?.let { rhythmTitle(it.rhythm) }
                                    ?: "30-second recording",
                                onOpenEcg = { shell.destination = MavDestination.Ecg },
                                onOpenMetric = { shell.destination = MavDestination.Metric(it.id) },
                            )

                            MavTab.WORKOUTS -> MavWorkoutsScreen(
                                workouts = effectiveWorkouts,
                                activeWorkout = activeWorkout,
                                onStart = { shell.destination = MavDestination.WorkoutStart },
                                onOpenActive = { shell.destination = MavDestination.WorkoutLive },
                            )
                        }
                }
                }
            }
        }
        }

        if (shell.showDevice) {
            MavDeviceSheet(
                connection = effectiveConnection,
                installedNames = installed.map { it.displayName to it.version },
                discovered = discovered,
                syncNote = syncNote,
                lowPower = lowPower,
                onLowPower = {
                    lowPower = it
                    viewModel.setLowPower(it)
                },
                onPair = { index -> installed.getOrNull(index)?.let { viewModel.connectors.connect(it) } },
                onSelectDevice = { viewModel.connectors.selectDevice(it) },
                onDisconnect = { viewModel.connectors.disconnect() },
                onManageConnectors = {
                    shell.showDevice = false
                    shell.destination = MavDestination.Connectors
                },
                onDismiss = { shell.showDevice = false },
            )
        }

        if (shell.showCalendar) {
            MavDayPickerDialog(
                day = shell.day,
                onPick = { shell.day = it },
                onDismiss = { shell.showCalendar = false },
            )
        }

        if (shell.showSettings) {
            MavSettingsSheet(
                appearance = AppearancePrefs.mode,
                onAppearance = { AppearancePrefs.set(context, it) },
                profileSummary = "$profileAge years · ${profileWeight.toInt()} kg",
                scoredDays = effectiveDays.size,
                onProfile = {
                    shell.showSettings = false
                    shell.destination = MavDestination.Profile
                },
                onJournal = {
                    shell.showSettings = false
                    journalAnswers = MavJournalLog.load(context, shell.dayKey)
                    shell.destination = MavDestination.Journal
                },
                onDiagnostics = {
                    shell.showSettings = false
                    shell.destination = MavDestination.Diagnostics
                },
                onDismiss = { shell.showSettings = false },
            )
        }
    }
}
