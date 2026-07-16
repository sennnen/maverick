package com.sennnen.mav.aura

import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bedtime
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.LocalFireDepartment
import androidx.compose.material.icons.outlined.GridView
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import com.sennnen.mav.MavAppState
import kotlin.math.abs

fun swipeDestination(current: AuraTab, dx: Float, dy: Float, threshold: Float): AuraTab {
    if (abs(dx) <= threshold || abs(dx) <= abs(dy) * 1.6f) return current
    return AuraTab.entries[(current.ordinal + if (dx < 0) 1 else -1)
        .coerceIn(0, AuraTab.entries.lastIndex)]
}

@Composable
fun AuraRootScreen(state: MavAppState, onRefresh: () -> Unit) {
    AuraTheme {
        var index by rememberSaveable { mutableIntStateOf(0) }
        var settingsOpen by rememberSaveable { mutableStateOf(false) }
        val selected = AuraTab.entries[index]
        val flickPx = with(LocalDensity.current) { 60.dp.toPx() }
        CompositionLocalProvider(
            LocalAuraSwitchTab provides { index = it.ordinal },
            LocalAuraOpenSettings provides { settingsOpen = true },
        ) {
            if (settingsOpen) MavSettingsSheet(state, onRefresh) { settingsOpen = false }
            Scaffold(containerColor = Aura.palette.bg, bottomBar = { AuraNavBar(selected) { index = it.ordinal } }) { inset ->
                Box(Modifier.fillMaxSize().padding(inset).pointerInput(selected) {
                    var totalX = 0f
                    detectHorizontalDragGestures(
                        onDragStart = { totalX = 0f },
                        onHorizontalDrag = { _, dx -> totalX += dx },
                        onDragEnd = {
                            if (abs(totalX) > flickPx) {
                                index = swipeDestination(selected, totalX, 0f, flickPx).ordinal
                            }
                        },
                    )
                }) {
                    Crossfade(selected, animationSpec = tween(240, easing = AuraMotion.ease), label = "auraHub") { tab ->
                        when (tab) {
                            AuraTab.TODAY -> AuraTodayScreen(state)
                            AuraTab.RECOVERY -> AuraRecoveryScreen(state)
                            AuraTab.STRAIN -> AuraStrainScreen(state)
                            AuraTab.SLEEP -> AuraSleepScreen(state)
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun AuraNavBar(selection: AuraTab, onSelect: (AuraTab) -> Unit) {
    NavigationBar(modifier = Modifier.navigationBarsPadding(), tonalElevation = 0.dp) {
        AuraTab.entries.forEach { tab ->
            NavigationBarItem(selected = tab == selection, onClick = { onSelect(tab) }, icon = {
                Icon(tab.icon, contentDescription = null, modifier = Modifier.size(22.dp))
            }, label = { Text(tab.title) })
        }
    }
}

private val AuraTab.icon get() = when (this) {
    AuraTab.TODAY -> Icons.Outlined.GridView
    AuraTab.RECOVERY -> Icons.Filled.Bolt
    AuraTab.STRAIN -> Icons.Filled.LocalFireDepartment
    AuraTab.SLEEP -> Icons.Filled.Bedtime
}
