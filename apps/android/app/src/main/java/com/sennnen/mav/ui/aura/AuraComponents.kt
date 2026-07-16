package com.sennnen.mav.ui.aura

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.animateFloat
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.NorthEast
import androidx.compose.material.icons.filled.SouthEast
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.scale
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.sennnen.mav.ui.NoopPrefs
import kotlin.math.abs
import kotlin.math.roundToInt

// High-contrast data-viz + chrome components (Android port of
// Strand/UI/AuraComponents.swift + AuraHubHeader.swift). Thin numerals,
// visible tracks/markers, adaptive ink so everything reads on black and on
// the glow tiles alike.

private fun Double.clamped01() = coerceIn(0.0, 1.0)

// MARK: - Cross-hub environment (AuraHubHeader.swift equivalents)

enum class AuraTab(val title: String) {
    TODAY("Today"), RECOVERY("Recovery"), STRAIN("Strain"), SLEEP("Sleep")
}

/** Set by the shell so any hub content can switch tabs. */
val LocalAuraSwitchTab = staticCompositionLocalOf<(AuraTab) -> Unit> { {} }

/** Set by the shell; hubs call it to present the app-wide settings sheet. */
val LocalAuraOpenSettings = staticCompositionLocalOf<() -> Unit> { {} }

// MARK: - Slider (track + glowing marker)

/** A read-only value indicator. Material 3: a stock `LinearProgressIndicator` (the former custom
 *  track + glow marker is dropped for the native-Material look). `glow` becomes the indicator colour. */
@Composable
fun AuraSlider(value: Double, glow: Color = Aura.palette.accent, modifier: Modifier = Modifier) {
    LinearProgressIndicator(
        progress = { value.clamped01().toFloat() },
        modifier = modifier.fillMaxWidth().height(6.dp),
        color = glow,
        trackColor = MaterialTheme.colorScheme.surfaceVariant,
    )
}

// MARK: - Mini stat (value + label + bar)

@Composable
fun AuraMiniStat(
    value: String,
    label: String,
    level: Double,
    tint: Color,
    unit: String = "",
    modifier: Modifier = Modifier,
) {
    val p = Aura.palette
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(3.dp), verticalAlignment = Alignment.Bottom) {
            Text(value, style = AuraType.number(30.sp), color = p.ink, maxLines = 1)
            if (unit.isNotEmpty()) {
                Text(
                    unit, style = AuraType.caption, color = p.ink.copy(alpha = 0.68f),
                    modifier = Modifier.padding(bottom = 4.dp),
                )
            }
        }
        Text(label, style = AuraType.caption, color = p.ink.copy(alpha = 0.78f), maxLines = 1)
        LinearProgressIndicator(
            progress = { level.clamped01().toFloat() },
            modifier = Modifier.fillMaxWidth().height(4.dp),
            color = tint,
            trackColor = MaterialTheme.colorScheme.surfaceVariant,
        )
    }
}

// MARK: - Score ring (hero, from AuraCharts.swift)

@Composable
fun AuraScoreRing(
    value: Double?,
    text: String,
    label: String,
    status: AuraStatus,
    maxValue: Double = 100.0,
    unit: String = "",
    /** FAMILY hue instead of the status colour (informational metrics like Effort). */
    tintOverride: Color? = null,
    size: Dp = 168.dp,
    lineWidth: Dp = 9.dp,
    modifier: Modifier = Modifier,
) {
    val p = Aura.palette
    val tint = tintOverride ?: status.color
    val target = ((value ?: 0.0) / maxValue).clamped01().toFloat()
    val fill by animateFloatAsState(
        targetValue = target,
        animationSpec = spring(dampingRatio = 0.85f, stiffness = Spring.StiffnessLow),
        label = "auraScoreFill",
    )
    Box(
        modifier.size(size).semantics { contentDescription = "$label $text" },
        contentAlignment = Alignment.Center,
    ) {
        // Material 3: a stock determinate `CircularProgressIndicator` hero ring (the former hand-drawn
        // glow arc is dropped), with the score + label centred inside.
        CircularProgressIndicator(
            progress = { fill },
            modifier = Modifier.size(size),
            color = tint,
            trackColor = MaterialTheme.colorScheme.surfaceVariant,
            strokeWidth = lineWidth,
            strokeCap = StrokeCap.Round,
        )
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.padding(horizontal = lineWidth + 8.dp),
        ) {
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    text, style = AuraType.number((size.value * 0.28f).sp), color = p.ink,
                    maxLines = 1,
                )
                if (unit.isNotEmpty() && value != null) {
                    Text(
                        unit, style = AuraType.caption, color = p.ink.copy(alpha = 0.6f),
                        modifier = Modifier.padding(bottom = 8.dp),
                    )
                }
            }
            Text(label, style = AuraType.caption, color = p.ink.copy(alpha = 0.65f), maxLines = 1)
        }
    }
}

// MARK: - Delta label

@Composable
fun AuraDelta(value: Double, suffix: String = "/AVG") {
    val p = Aura.palette
    val up = value >= 0
    val color = if (up) p.good else p.bad
    Row(
        Modifier
            .auraGlass(CircleShape)
            .padding(horizontal = 8.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(3.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            if (up) Icons.Filled.NorthEast else Icons.Filled.SouthEast,
            contentDescription = null, tint = color, modifier = Modifier.size(10.dp),
        )
        Text("${abs(value.roundToInt())} $suffix", style = AuraType.caption, color = color)
    }
}

// MARK: - Status chip

enum class AuraChipKind { POSITIVE, CAUTION, NEGATIVE, NEUTRAL }

val AuraStatus.chipKind: AuraChipKind
    get() = when (this) {
        AuraStatus.GOOD -> AuraChipKind.POSITIVE
        AuraStatus.FAIR -> AuraChipKind.CAUTION
        AuraStatus.LOW -> AuraChipKind.NEGATIVE
        AuraStatus.NONE -> AuraChipKind.NEUTRAL
    }

@Composable
fun AuraStatusChip(text: String, kind: AuraChipKind, pulsing: Boolean = false) {
    val p = Aura.palette
    val color = when (kind) {
        AuraChipKind.POSITIVE -> p.good
        AuraChipKind.CAUTION -> p.fair
        AuraChipKind.NEGATIVE -> p.bad
        AuraChipKind.NEUTRAL -> p.ink.copy(alpha = 0.7f)
    }
    val pulse = rememberInfiniteTransition(label = "chipPulse")
    val scale by if (pulsing) {
        pulse.animateFloat(
            initialValue = 1f, targetValue = 1.3f,
            animationSpec = infiniteRepeatable(tween(900), RepeatMode.Reverse),
            label = "chipPulseScale",
        )
    } else remember { mutableStateOf(1f) }
    Row(
        Modifier
            .auraGlass(CircleShape)
            .padding(horizontal = 9.dp, vertical = 5.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(7.dp)
                .scale(scale)
                .alpha(if (pulsing && scale > 1.15f) 0.6f else 1f)
                .background(color, CircleShape)
        )
        Text(text, style = AuraType.caption, color = color)
    }
}

// MARK: - Section header

@Composable
fun AuraSectionHeader(title: String, actionTitle: String? = null, action: (() -> Unit)? = null) {
    val p = Aura.palette
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
        Text(title, style = AuraType.heading(19.sp), color = p.ink)
        Spacer(Modifier.weight(1f))
        if (actionTitle != null && action != null) {
            Text(
                actionTitle, style = AuraType.caption, color = p.ink.copy(alpha = 0.6f),
                modifier = Modifier.clickable(onClick = action),
            )
        }
    }
}

// MARK: - Live heart-rate pill (glass chrome)

@Composable
fun AuraLiveHRPill(
    bpm: Int?,
    deviceName: String,
    batteryPercent: Int?,
    bonded: Boolean,
    onClick: () -> Unit = {},
) {
    val p = Aura.palette
    val pulse = rememberInfiniteTransition(label = "hrPulse")
    val heartScale by if (bpm != null) {
        pulse.animateFloat(
            initialValue = 1f, targetValue = 1.18f,
            animationSpec = infiniteRepeatable(tween(650), RepeatMode.Reverse),
            label = "hrPulseScale",
        )
    } else remember { mutableStateOf(1f) }
    Row(
        Modifier
            .fillMaxWidth()
            .auraGlass(CircleShape)
            .auraPressable(onClick = onClick)
            .padding(horizontal = 18.dp, vertical = 14.dp)
            .semantics { contentDescription = "Live heart rate ${bpm ?: "--"}" },
        horizontalArrangement = Arrangement.spacedBy(11.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            Icons.Filled.Favorite, contentDescription = null,
            tint = if (bonded) p.bad else p.ink.copy(alpha = 0.5f),
            modifier = Modifier.size(16.dp).scale(heartScale),
        )
        Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            // The number rolls in the direction it moved on each live update
            // (twin of iOS `.contentTransition(.numericText())`).
            AnimatedContent(
                targetState = bpm,
                transitionSpec = {
                    val up = (targetState ?: 0) >= (initialState ?: 0)
                    (slideInVertically { if (up) it / 2 else -it / 2 } + fadeIn(tween(180))) togetherWith
                        (slideOutVertically { if (up) -it / 2 else it / 2 } + fadeOut(tween(120)))
                },
                label = "bpmRoll",
            ) { v ->
                Text(v?.toString() ?: "--", style = AuraType.number(22.sp), color = p.ink)
            }
            Text(
                "bpm", style = AuraType.caption, color = p.ink.copy(alpha = 0.55f),
                modifier = Modifier.padding(bottom = 3.dp),
            )
        }
        Text(
            deviceName, style = AuraType.sub, color = p.ink.copy(alpha = 0.55f),
            maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f),
        )
        if (batteryPercent != null) {
            Text(
                "$batteryPercent%", style = AuraType.caption,
                color = if (batteryPercent <= 20) p.bad else p.ink.copy(alpha = 0.55f),
            )
        }
    }
}

// MARK: - Nav row

@Composable
fun AuraNavRow(
    icon: ImageVector,
    title: String,
    detail: String = "",
    tint: Color? = null,
    onClick: () -> Unit,
) {
    val p = Aura.palette
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 18.dp, vertical = 15.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            icon, contentDescription = null,
            tint = tint ?: p.ink.copy(alpha = 0.85f), modifier = Modifier.width(26.dp).size(20.dp),
        )
        Text(title, style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
        Spacer(Modifier.weight(1f))
        if (detail.isNotEmpty()) {
            Text(
                detail, style = AuraType.sub, color = p.ink.copy(alpha = 0.5f),
                maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.widthIn(max = 160.dp),
            )
        }
        Icon(
            Icons.Filled.ChevronRight, contentDescription = null,
            tint = p.ink.copy(alpha = 0.35f), modifier = Modifier.size(16.dp),
        )
    }
}

// MARK: - Range picker

enum class AuraTrendRange(val label: String, val days: Int) {
    WEEK("1W", 7), MONTH("1M", 30), SIX_MONTHS("6M", 182)
}

@Composable
fun AuraRangePicker(selection: AuraTrendRange, onSelect: (AuraTrendRange) -> Unit) {
    val p = Aura.palette
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        AuraTrendRange.entries.forEach { r ->
            val active = r == selection
            FilterChip(
                selected = active,
                onClick = { onSelect(r) },
                label = { Text(r.label, style = AuraType.caption) },
                colors = FilterChipDefaults.filterChipColors(
                    containerColor = p.ink.copy(alpha = 0.08f),
                    labelColor = p.ink.copy(alpha = 0.65f),
                    selectedContainerColor = p.accent,
                    selectedLabelColor = Color.Black,
                ),
            )
        }
    }
}

// MARK: - Per-hub card visibility (the pencil's restricted edit mode)

/** Secondary-card show/hide per hub, CSV-persisted under the SAME keys as iOS
 *  AppStorage (`aura.hiddenCards.<hub>`) so a future sync stays trivial. */
object AuraHubCards {
    fun storageKey(hub: String) = "aura.hiddenCards.$hub"
    fun decode(csv: String): Set<String> =
        csv.split(",").filter { it.isNotEmpty() }.toSet()
    fun encode(hidden: Set<String>): String = hidden.sorted().joinToString(",")

    fun load(context: android.content.Context, hub: String): String =
        NoopPrefs.of(context).getString(storageKey(hub), "") ?: ""

    fun save(context: android.content.Context, hub: String, csv: String) {
        NoopPrefs.of(context).edit().putString(storageKey(hub), csv).apply()
    }
}

// MARK: - Hub header (title + pencil + cog)

@Composable
fun AuraHubHeader(
    title: String,
    subtitle: String = "",
    /** null = hub has no customisable cards (pencil hidden). */
    editing: Boolean? = null,
    onToggleEditing: () -> Unit = {},
) {
    val p = Aura.palette
    val openSettings = LocalAuraOpenSettings.current
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(title, style = AuraType.display(34.sp), color = p.ink)
            if (subtitle.isNotEmpty()) {
                Text(subtitle, style = AuraType.sub, color = p.ink.copy(alpha = 0.66f))
            }
        }
        if (editing != null) {
            AuraChromeButton(
                icon = if (editing) Icons.Filled.Check else Icons.Filled.Edit,
                label = if (editing) "Done editing" else "Edit cards",
                active = editing,
                onClick = onToggleEditing,
            )
        }
        AuraChromeButton(Icons.Outlined.Settings, "Settings", active = false, onClick = openSettings)
    }
}

@Composable
fun AuraChromeButton(icon: ImageVector, label: String, active: Boolean, onClick: () -> Unit) {
    val p = Aura.palette
    IconButton(
        onClick = onClick,
        modifier = Modifier.size(40.dp).semantics { contentDescription = label },
        colors = IconButtonDefaults.iconButtonColors(
            containerColor = if (active) p.accent else Color(0xFF17171A).copy(alpha = 0.94f),
            contentColor = if (active) Color.Black else p.ink.copy(alpha = 0.9f),
        ),
    ) {
        Icon(icon, contentDescription = null, modifier = Modifier.size(18.dp))
    }
}

// MARK: - Editable secondary card wrapper

/** Wraps a secondary hub card. In edit mode it shows a toggle badge and dims
 *  hidden cards; at rest hidden cards vanish. Hero/pillar cards are never
 *  wrapped, so they can't be removed (restricted edit). */
@Composable
fun AuraEditableCard(
    key: String,
    hiddenCSV: String,
    onHiddenCSVChange: (String) -> Unit,
    editing: Boolean,
    content: @Composable () -> Unit,
) {
    val p = Aura.palette
    val hidden = AuraHubCards.decode(hiddenCSV).contains(key)
    if (editing) {
        Box(Modifier.fillMaxWidth().alpha(if (hidden) 0.35f else 1f)) {
            content()
            Box(
                Modifier
                    .align(Alignment.TopEnd)
                    .padding(10.dp)
                    .size(32.dp)
                    .background(if (hidden) p.card else p.accent, CircleShape)
                    .clickable {
                        val set = AuraHubCards.decode(hiddenCSV).toMutableSet()
                        if (hidden) set.remove(key) else set.add(key)
                        onHiddenCSVChange(AuraHubCards.encode(set))
                    }
                    .semantics { contentDescription = if (hidden) "Show card" else "Hide card" },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    if (hidden) Icons.Filled.VisibilityOff else Icons.Filled.Visibility,
                    contentDescription = null,
                    tint = if (hidden) p.ink.copy(alpha = 0.6f) else Color.Black,
                    modifier = Modifier.size(16.dp),
                )
            }
        }
    } else if (!hidden) {
        content()
    }
}

// MARK: - Sparkline (inline vitals trend)

/** A tiny inline trend line — no axes, no labels; just the last fortnight's shape.
 *  Used beside vital values so a drift is visible without opening the detail. */
@Composable
fun AuraSparkline(
    values: List<Double>,
    tint: Color,
    modifier: Modifier = Modifier,
) {
    if (values.size < 2) return
    val lo = values.min()
    val hi = values.max()
    val range = (hi - lo).takeIf { it > 0.0001 } ?: 1.0
    androidx.compose.foundation.Canvas(modifier.width(48.dp).height(18.dp)) {
        val stepX = size.width / (values.size - 1)
        val pts = values.mapIndexed { i, v ->
            Offset(i * stepX, size.height - ((v - lo) / range).toFloat() * size.height)
        }
        for (i in 1 until pts.size) {
            drawLine(
                tint.copy(alpha = 0.85f), pts[i - 1], pts[i],
                strokeWidth = 1.5f.dp.toPx(), cap = StrokeCap.Round,
            )
        }
        drawCircle(tint, radius = 2.dp.toPx(), center = pts.last())
    }
}

/** Remembered, persisted hidden-cards CSV state for a hub. */
@Composable
fun rememberHubHiddenCards(hub: String): Pair<String, (String) -> Unit> {
    val context = LocalContext.current
    var csv by rememberSaveable(hub) { mutableStateOf(AuraHubCards.load(context, hub)) }
    return csv to { new: String ->
        csv = new
        AuraHubCards.save(context, hub, new)
    }
}
