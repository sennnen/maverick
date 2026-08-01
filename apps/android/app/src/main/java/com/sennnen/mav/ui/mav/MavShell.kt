package com.sennnen.mav.ui.mav

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.border
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CenterAlignedTopAppBar
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import java.time.LocalDate

// The whole app's structure: three tabs, one top bar, one settings sheet, one device sheet.
// The iOS twin is UI/MavShell.swift.
//
// Built out of Material 3's own components — Scaffold, CenterAlignedTopAppBar, NavigationBar — so
// the chrome inherits the platform's insets, elevation, ripple, predictive-back behaviour and
// accessibility semantics instead of approximating them. The Terrain palette reaches them through
// the ColorScheme in MavTheme, so using the real component is also how the real palette is applied.
//
// `navigationIcon` puts settings hard against the left edge and `actions` puts the strap hard
// against the right, which is what the slots are for.
//
// The shell owns the selected day. Every tab reads it and none keeps its own.

enum class MavTab(val title: String) {
    TODAY("Today"),
    VITALS("Vitals"),
    WORKOUTS("Workouts"),
}

/** Where a tab can push to. Data rather than a navigation graph, so the back stack is one `when`. */
sealed interface MavDestination {
    data object None : MavDestination
    data class Metric(val metricId: String) : MavDestination
    data object Cycle : MavDestination
    data object Connectors : MavDestination
    data object Diagnostics : MavDestination
    data object Reports : MavDestination
    data object Ecg : MavDestination
    data class EcgResult(val captureId: ULong) : MavDestination
    data object WorkoutStart : MavDestination

    /** The confirm screen for a chosen sport: end condition, zone target, GPS, keep screen on. */
    data class WorkoutConfig(val sport: String) : MavDestination
    data object WorkoutLive : MavDestination
    data object Strength : MavDestination
    data object Journal : MavDestination
    data object Profile : MavDestination
}

class MavShellState {
    var tab by mutableStateOf(MavTab.TODAY)
    var day by mutableStateOf(LocalDate.now())
    var destination by mutableStateOf<MavDestination>(MavDestination.None)
    var showSettings by mutableStateOf(false)
    var showDevice by mutableStateOf(false)
    var showCalendar by mutableStateOf(false)

    /** Forward stops on the newest day. */
    val canGoForward: Boolean get() = day.isBefore(LocalDate.now())

    val dayKey: String get() = day.toString()
}

@Composable
fun rememberMavShellState(): MavShellState = remember { MavShellState() }

// ---------------------------------------------------------------------------------------------
// Chrome
// ---------------------------------------------------------------------------------------------

/**
 * Settings hard left, the date centred, the strap hard right — a real `CenterAlignedTopAppBar`, so
 * the insets, the scroll-edge colour change and the slot semantics are Material's.
 *
 * The first attempt put a three-part date stepper in the title slot and it wrapped onto a second
 * line on a Pixel: the title gets whatever width is left after the navigation icon and the actions,
 * and two 48dp icon buttons plus a date did not fit. The arrows are 36dp now and the title is a
 * single compact row, which does.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MavTopBar(
    shell: MavShellState,
    batteryPercent: Int?,
    connected: Boolean,
    deviceName: String?,
    onSettings: () -> Unit,
    onDevice: () -> Unit,
) {
    CenterAlignedTopAppBar(
        title = {
            AnimatedContent(
                targetState = shell.tab,
                transitionSpec = { fadeIn(tween(180)) togetherWith fadeOut(tween(120)) },
                label = "Section title",
            ) { tab ->
                if (tab == MavTab.TODAY) {
                    MavDateStepper(shell)
                } else {
                    Text(
                        tab.title,
                        style = MavType.label,
                        modifier = Modifier.semantics { heading() },
                    )
                }
            }
        },
        navigationIcon = {
            IconButton(onClick = onSettings) {
                Icon(MavIcons.settings, contentDescription = "Settings")
            }
        },
        actions = { MavDeviceChip(batteryPercent, connected, deviceName, onDevice) },
        // Transparent, so the atmosphere behind the shell runs under the status bar. Painting the
        // canvas here put a hard horizontal edge across the top of every tab where the bar ended.
        // `scrolledContainerColor` still opaques it once content is underneath, which is the one
        // moment the bar has to separate itself from what it is covering.
        colors = TopAppBarDefaults.centerAlignedTopAppBarColors(
            containerColor = Color.Transparent,
            scrolledContainerColor = MavTheme.palette.surface,
            titleContentColor = MavTheme.palette.ink,
            navigationIconContentColor = MavTheme.palette.ink,
            actionIconContentColor = MavTheme.palette.ink,
        ),
    )
}

@Composable
private fun MavDateStepper(shell: MavShellState) {
    val palette = MavTheme.palette
    val today = LocalDate.now()
    val title = when (shell.day) {
        today -> "Today"
        today.minusDays(1) -> "Yesterday"
        else -> shell.day.toString()
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        IconButton(
            onClick = { shell.day = shell.day.minusDays(1) },
            modifier = Modifier.size(36.dp),
        ) {
            Icon(
                MavIcons.chevronLeft,
                contentDescription = "Previous day",
                tint = palette.inkSecondary,
                modifier = Modifier.size(22.dp),
            )
        }

        // The title is a control, not a label. Stepping a day at a time is fine for yesterday and
        // useless for last March, so tapping it opens a calendar.
        TextButton(
            onClick = { shell.showCalendar = true },
            colors = ButtonDefaults.textButtonColors(contentColor = palette.ink),
            contentPadding = PaddingValues(horizontal = 8.dp, vertical = 4.dp),
        ) {
            AnimatedContent(
                targetState = title,
                transitionSpec = { fadeIn(tween(180)) togetherWith fadeOut(tween(120)) },
                label = "Selected date",
            ) { selectedTitle ->
                Text(
                    selectedTitle,
                    style = MavType.label,
                    maxLines = 1,
                    modifier = Modifier.semantics { heading() },
                )
            }
        }

        IconButton(
            onClick = { shell.day = shell.day.plusDays(1) },
            enabled = shell.canGoForward,
            modifier = Modifier.size(36.dp),
        ) {
            Icon(
                MavIcons.chevronRight,
                contentDescription = "Next day",
                tint = palette.inkSecondary.copy(alpha = if (shell.canGoForward) 1f else 0.28f),
                modifier = Modifier.size(22.dp),
            )
        }
    }
}

/**
 * The battery percentage is shown whenever the core has one, and it disappears together with the
 * link — a battery percentage with no link is a stale number pretending to be live.
 */
@Composable
private fun MavDeviceChip(
    batteryPercent: Int?,
    connected: Boolean,
    deviceName: String?,
    onClick: () -> Unit,
) {
    val palette = MavTheme.palette
    val summary = when {
        !connected -> "No device connected. Open device settings."
        batteryPercent == null -> "${deviceName ?: "Device"}, connected."
        else -> "${deviceName ?: "Device"}, $batteryPercent percent battery, connected."
    }
    AssistChip(
        onClick = onClick,
        label = {
            if (connected && batteryPercent != null) {
                Text("$batteryPercent%", style = MavType.caption)
            } else if (!connected) {
                Text("Connect", style = MavType.caption)
            }
        },
        trailingIcon = { MavStrapGlyph(connected) },
        colors = AssistChipDefaults.assistChipColors(
            containerColor = palette.raised,
            labelColor = palette.ink,
        ),
        border = null,
        modifier = Modifier
            .padding(end = 8.dp)
            .semantics(mergeDescendants = true) { contentDescription = summary },
    )
}

@Composable
private fun MavStrapGlyph(connected: Boolean) {
    val palette = MavTheme.palette
    Box(
        Modifier
            .size(15.dp, 21.dp)
            .border(1.6.dp, palette.ink.copy(alpha = 0.85f), RoundedCornerShape(5.5.dp)),
    ) {
        Box(
            Modifier
                .padding(top = 2.5.dp)
                .align(Alignment.TopEnd)
                .size(5.5.dp)
                .clip(CircleShape)
                // Link state is weight, not category colour: full ink when live, faint ink when not.
                .background(if (connected) mavLiveInk() else palette.ink.copy(alpha = 0.25f)),
        )
    }
}

/**
 * Material's `NavigationBar`. Selection is the indicator pill, not a hue change: the accent is
 * rationed to one affirmative action per screen, and a selected tab is not one.
 */
@Composable
fun MavBottomBar(current: MavTab, onSelect: (MavTab) -> Unit) {
    val palette = MavTheme.palette
    NavigationBar(
        // Transparent for the same reason as the top bar: an opaque bar cropped the atmosphere at
        // the bottom of every tab.
        containerColor = Color.Transparent,
        contentColor = palette.ink,
        tonalElevation = 0.dp,
    ) {
        MavTab.entries.forEach { tab ->
            NavigationBarItem(
                selected = tab == current,
                onClick = { onSelect(tab) },
                icon = { Icon(MavIcons.tabIcon(tab), contentDescription = null) },
                label = { Text(tab.title, style = MavType.caption) },
                alwaysShowLabel = true,
                colors = NavigationBarItemDefaults.colors(
                    selectedIconColor = palette.ink,
                    selectedTextColor = palette.ink,
                    indicatorColor = palette.raised,
                    unselectedIconColor = palette.inkSecondary,
                    unselectedTextColor = palette.inkSecondary,
                ),
            )
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Scroll containers
// ---------------------------------------------------------------------------------------------

/**
 * One scroll container, so the padding a tab's content sits in is decided once.
 *
 * The atmosphere is deliberately NOT drawn here. This composable lives inside the tab switcher's
 * `AnimatedContent`, so a background drawn at this level slid sideways and cross-faded on every tab
 * change, and it stopped at the content padding rather than running under the bars. It belongs
 * behind the whole shell — see `MavRootScreen`.
 */
@Composable
fun MavTabScroll(content: @Composable () -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = MavTheme.screenMargin),
        verticalArrangement = Arrangement.spacedBy(MavTheme.cardSpacing),
    ) {
        content()
        Spacer(Modifier.height(32.dp))
    }
}

/**
 * A pushed destination: a back button, a title, and nothing that competes with the bottom bar it
 * replaced. You back out to move sideways.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MavDetailScaffold(
    title: String,
    onBack: () -> Unit,
    /**
     * A landscape to run full-bleed behind the whole screen, veiled so ordinary ink still sits on
     * it. A metric opened from a row keeps the row's own crop, so the card the reader tapped grows
     * into the page rather than being replaced by an unrelated one.
     */
    scene: Alignment? = null,
    content: @Composable () -> Unit,
) {
    val palette = MavTheme.palette
    Scaffold(
        containerColor = palette.canvas,
        topBar = {
            CenterAlignedTopAppBar(
                title = {
                    Text(
                        title,
                        style = MavType.caption,
                        maxLines = 1,
                        modifier = Modifier.semantics { heading() },
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(MavIcons.back, contentDescription = "Back")
                    }
                },
                colors = TopAppBarDefaults.centerAlignedTopAppBarColors(
                    containerColor = Color.Transparent,
                    titleContentColor = palette.ink,
                    navigationIconContentColor = palette.ink,
                ),
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize()) {
            if (scene != null) {
                MavScene(
                    Modifier.matchParentSize(),
                    alignment = scene,
                    treatment = MavSceneTreatment.VEILED,
                )
            }
            Column(
                Modifier
                    .fillMaxSize()
                    .padding(padding)
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = MavTheme.screenMargin),
                verticalArrangement = Arrangement.spacedBy(MavTheme.cardSpacing),
            ) {
                content()
                Spacer(Modifier.height(60.dp))
            }
        }
    }
}
