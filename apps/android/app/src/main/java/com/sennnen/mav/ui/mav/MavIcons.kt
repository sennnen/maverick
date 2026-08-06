package com.sennnen.mav.ui.mav

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.rounded.DirectionsRun
import androidx.compose.material.icons.automirrored.rounded.HelpOutline
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.rounded.Bedtime
import androidx.compose.material.icons.rounded.CalendarMonth
import androidx.compose.material.icons.rounded.ElectricBolt
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FitnessCenter
import androidx.compose.material.icons.rounded.LocalFireDepartment
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.MonitorHeart
import androidx.compose.material.icons.rounded.Warning
import androidx.compose.material.icons.rounded.Spa
import androidx.compose.material.icons.rounded.WbSunny
import androidx.compose.ui.graphics.vector.ImageVector

// The icon set, named by role rather than by picture, so a screen asks for "the settings control"
// and not for a particular glyph. Only the icons bundled with material-icons-core are used - an
// extended-set dependency for six glyphs is a megabyte nobody asked for.

object MavIcons {
    val settings: ImageVector = Icons.Filled.Settings
    val back: ImageVector = Icons.AutoMirrored.Filled.ArrowBack
    val play: ImageVector = Icons.Filled.PlayArrow
    val more: ImageVector = Icons.Filled.MoreVert
    val strength: ImageVector = Icons.Rounded.FitnessCenter
    val heartRate: ImageVector = Icons.Rounded.MonitorHeart

    // Real chevrons. The serif role is Old Standard TT, which has no guillemets, so a text "‹"
    // fell back to a parenthesis on device — a glyph is not a substitute for an icon.
    val chevronLeft: ImageVector = Icons.AutoMirrored.Filled.KeyboardArrowLeft
    val chevronRight: ImageVector = Icons.AutoMirrored.Filled.KeyboardArrowRight
    val check: ImageVector = Icons.Rounded.CheckCircle
    val alert: ImageVector = Icons.Rounded.Warning
    val unknown: ImageVector = Icons.AutoMirrored.Rounded.HelpOutline

    fun tabIcon(tab: MavTab): ImageVector = when (tab) {
        MavTab.TODAY -> Icons.Rounded.WbSunny
        MavTab.VITALS -> Icons.Rounded.MonitorHeart
        MavTab.WORKOUTS -> Icons.AutoMirrored.Rounded.DirectionsRun
    }

    /** The small family marker on a metric row. One shape per family, so it is not colour alone. */
    fun familyIcon(family: MavFamily): ImageVector = when (family) {
        MavFamily.CHARGE -> Icons.Rounded.ElectricBolt
        MavFamily.REST -> Icons.Rounded.Bedtime
        MavFamily.EFFORT -> Icons.Rounded.LocalFireDepartment
        MavFamily.HEART -> Icons.Rounded.Favorite
        MavFamily.ENERGY -> Icons.Rounded.Spa
        MavFamily.VITALS -> Icons.Rounded.MonitorHeart
        MavFamily.CYCLE -> Icons.Rounded.CalendarMonth
    }
}
