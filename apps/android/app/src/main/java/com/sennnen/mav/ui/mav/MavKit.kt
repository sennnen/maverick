package com.sennnen.mav.ui.mav

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.sennnen.mav.R

// The Terrain component vocabulary, Android side. The iOS twin is UI/MavKit.swift, and the two are
// deliberately *not* built from the same components — they are the same design expressed in each
// platform's own material.
//
// On iOS that means Liquid Glass on as much as the OS will put it on. Here it means Material 3:
// Card, OutlinedCard, ListItem, Button, OutlinedButton, TextButton, Switch, AssistChip, FlowRow,
// HorizontalDivider. Terrain reaches all of them through the ColorScheme in MavTheme, so reaching
// for the real component is also how the real palette gets applied — and it is how the app inherits
// ripple, state layers, elevation, predictive back and every accessibility affordance instead of
// approximating them one at a time.
//
// Custom is reserved for the four things Material has no component for: the arc gauge, the baseline
// range bar, the zone ladder, and the scene.
//
// Two rules stay structural:
//
//  1. A component takes a family, never a colour. It resolves its own wash.
//  2. A component that draws data takes an accessibilitySummary it cannot be constructed without.

/** The minimum touch target, applied here so it is never a per-screen decision. */
fun Modifier.mavTarget(): Modifier = defaultMinSize(minWidth = 48.dp, minHeight = 48.dp)

// ---------------------------------------------------------------------------------------------
// Photography
// ---------------------------------------------------------------------------------------------

/**
 * How hard a landscape is pushed back, which follows from what sits on top of it.
 *
 * The wash is not decoration: it is what makes text contrast a constant instead of a property of
 * whichever photograph happened to load.
 */
enum class MavSceneTreatment {
    /**
     * A hero. White copy sits directly on the landscape, so the scrim is dark and
     * scheme-independent - white on a light photograph is the failure this prevents.
     */
    STORY,

    /**
     * A repeating surface. Ordinary ink sits on it, so the photograph is veiled almost to the
     * canvas and survives only as texture. This is what lets a metric row carry a landscape
     * without the row turning into a poster.
     */
    VEILED,
}

/**
 * A landscape behind a wash.
 *
 * Photography is deliberately scarce. One landscape on a screen reads as considered; a landscape
 * behind every card reads as a screensaver, and the copy stops being the thing you look at. The
 * rule is at most one [MavSceneTreatment.STORY] scene per screen, and VEILED for anything
 * repeating.
 *
 * Placeholder art until licensed landscapes land, each of which needs a light and a dark variant
 * so the wash is not fighting the photograph.
 */
@Composable
fun MavScene(
    modifier: Modifier = Modifier,
    alignment: Alignment = Alignment.Center,
    treatment: MavSceneTreatment = MavSceneTreatment.STORY,
) {
    val palette = MavTheme.palette
    Box(modifier.background(palette.canvas).clearAndSetSemantics {}) {
        Image(
            painter = painterResource(R.drawable.terrain_placeholder),
            contentDescription = null,
            contentScale = ContentScale.Crop,
            alignment = alignment,
            modifier = Modifier.fillMaxSize(),
        )
        Box(
            Modifier
                .fillMaxSize()
                .background(
                    when (treatment) {
                        MavSceneTreatment.STORY -> palette.photoScrim
                        MavSceneTreatment.VEILED -> palette.photoVeil
                    },
                ),
        )
    }
}

/**
 * The atmosphere behind a tab root.
 *
 * Three tab roots on a flat near-black read as a void: the cards float on nothing and the screen has
 * no depth to scroll against. Two soft blooms of the one hue, off-centre so the screen has a light
 * source rather than a symmetrical glow.
 *
 * It is deliberately not a photograph. The landscape already appears on the hero cards, on every
 * Vitals row and behind every detail screen; putting it behind the tab roots as well would make it
 * wallpaper rather than an accent. tools/check_a11y.py checks ink against the canvas at full bloom
 * strength, so no part of the gradient can put a label under its ratio.
 */
@Composable
fun MavAtmosphere(modifier: Modifier = Modifier) {
    val palette = MavTheme.palette
    Canvas(modifier.clearAndSetSemantics {}) {
        drawRect(palette.canvas)
    // Fading to a fully transparent *hue* rather than to Color.Transparent. Transparent is
    // (0,0,0,0), so interpolating to it walks the RGB toward black on the way out and leaves a
    // visible dark ring around each bloom. Holding the colour and moving only the alpha keeps the
    // falloff clean.

        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(palette.bloomTop, palette.bloomTop.copy(alpha = 0f)),
                center = Offset(size.width * 0.16f, size.height * 0.06f),
                radius = size.width * 0.85f,
            ),
            radius = size.width * 0.85f,
            center = Offset(size.width * 0.16f, size.height * 0.06f),
        )
        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(palette.bloomBottom, palette.bloomBottom.copy(alpha = 0f)),
                center = Offset(size.width * 0.95f, size.height * 0.72f),
                radius = size.width * 0.8f,
            ),
            radius = size.width * 0.8f,
            center = Offset(size.width * 0.95f, size.height * 0.72f),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Chrome
// ---------------------------------------------------------------------------------------------

/** A tonal icon button — Material's own, so it carries its own ripple and state layer. */
@Composable
fun MavIconButton(icon: ImageVector, label: String, onClick: () -> Unit) {
    FilledTonalIconButton(
        onClick = onClick,
        colors = IconButtonDefaults.filledTonalIconButtonColors(
            containerColor = MavTheme.palette.raised,
            contentColor = MavTheme.palette.ink,
        ),
    ) {
        Icon(icon, contentDescription = label, modifier = Modifier.size(20.dp))
    }
}

/** A compact tool shortcut. The familiar Material icon-button shape keeps utility actions quiet. */
@Composable
fun MavToolShortcut(
    title: String,
    icon: ImageVector,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
) {
    Column(
        modifier = modifier,
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        FilledTonalIconButton(
            onClick = onClick,
            colors = IconButtonDefaults.filledTonalIconButtonColors(
                containerColor = MavTheme.palette.raised,
                contentColor = MavTheme.palette.ink,
            ),
        ) {
            Icon(icon, contentDescription = title, modifier = Modifier.size(19.dp))
        }
        Text(title, style = MavType.caption, color = MavTheme.palette.inkSecondary)
    }
}

// ---------------------------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------------------------

@Composable
fun MavSectionHeader(title: String) {
    Text(
        text = title,
        style = MavType.title,
        color = MavTheme.palette.ink,
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = MavTheme.sectionGap, bottom = 8.dp)
            .semantics { heading() },
    )
}

/** A neutral card — Material's `Card`, in the Terrain surface colour. */
@Composable
fun MavTile(
    modifier: Modifier = Modifier,
    padded: Boolean = true,
    content: @Composable () -> Unit,
) {
    Card(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(MavTheme.tileRadius),
        colors = CardDefaults.cardColors(
            containerColor = MavTheme.palette.surface,
            contentColor = MavTheme.palette.ink,
        ),
    ) {
        Column(Modifier.padding(if (padded) MavTheme.tilePadding else 0.dp)) { content() }
    }
}

/**
 * A card whose surface carries a verdict. The tint is resolved here from the status, so no caller
 * can tint a card with something that is not a judgement.
 *
 * The status is one quiet, solid tonal surface. Directional gradients made identical cards look
 * artificially lit and competed with the data.
 */
@Composable
fun MavStatusCard(
    /** Which metric this card belongs to, or null when it is not a metric. */
    family: MavFamily? = null,
    modifier: Modifier = Modifier,
    radius: Dp = MavTheme.cardRadius,
    onClick: (() -> Unit)? = null,
    /**
     * Crop of the veiled landscape to carry behind the card, or null for a plain surface. The
     * photograph is knocked back to texture, so the status wash still reads over it.
     */
    scene: Alignment? = null,
    content: @Composable () -> Unit,
) {
    val palette = MavTheme.palette
    val wash = family?.tint(palette) ?: palette.tintNeutral
    val shape = RoundedCornerShape(radius)
    // With a landscape behind it the container has to stay translucent, or the card paints over
    // the photograph it was given. Without one it stays opaque, which is cheaper to compose.
    val colors = CardDefaults.cardColors(
        containerColor = if (scene == null) wash.compositeOver(palette.surface) else Color.Transparent,
        contentColor = palette.ink,
    )
    val inner: @Composable () -> Unit = {
        Box {
            if (scene != null) {
                // Veiled landscape, then the status wash at its own alpha on top. The veil is what
                // tools/check_a11y.py proved ink safe against for any photograph; the wash is a
                // tenth-alpha pigment over it, so the composite stays inside the proven case.
                MavScene(
                    Modifier.matchParentSize(),
                    alignment = scene,
                    treatment = MavSceneTreatment.VEILED,
                )
                Box(Modifier.matchParentSize().background(wash))
            }
            Column(
                Modifier
                    .fillMaxWidth()
                    .padding(18.dp),
            ) { content() }
        }
    }
    if (onClick == null) {
        Card(modifier.fillMaxWidth(), shape = shape, colors = colors) { inner() }
    } else {
        Card(onClick, modifier.fillMaxWidth(), shape = shape, colors = colors) {
            inner()
        }
    }
}

/**
 * The honest absence. An `OutlinedCard` rather than a filled one, so an absent metric is a
 * different *kind* of object at a glance rather than just a paler one.
 */
@Composable
fun MavUnavailableCard(
    name: String,
    reason: String,
    modifier: Modifier = Modifier,
    /** Crop of the veiled landscape behind the card, or null for a plain surface. */
    scene: Alignment? = null,
) {
    val palette = MavTheme.palette
    OutlinedCard(
        modifier = modifier
            .fillMaxWidth()
            .semantics(mergeDescendants = true) { contentDescription = "$name. $reason" },
        shape = RoundedCornerShape(MavTheme.tileRadius),
        colors = CardDefaults.outlinedCardColors(
            containerColor = if (scene == null) MavTheme.palette.tintNeutral else Color.Transparent,
            contentColor = palette.inkSecondary,
        ),
        border = BorderStroke(1.dp, palette.hairlineStrong),
    ) {
        if (scene != null) {
            Box {
                MavScene(
                    Modifier.matchParentSize(),
                    alignment = scene,
                    treatment = MavSceneTreatment.VEILED,
                )
                Box(Modifier.matchParentSize().background(MavTheme.palette.tintNeutral))
            }
        }
        Column(Modifier.padding(horizontal = MavTheme.tilePadding, vertical = 17.dp)) {
            Text(name, style = MavType.label, color = palette.ink)
            Text(
                reason,
                style = MavType.body,
                color = palette.inkSecondary,
                modifier = Modifier.padding(top = 6.dp),
            )
        }
    }
}

/** A row — Material's `ListItem`, so its slots, text styles and heights are the platform's. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MavRow(
    title: String,
    detail: String? = null,
    modifier: Modifier = Modifier,
    trailing: @Composable (() -> Unit)? = null,
) {
    val palette = MavTheme.palette
    ListItem(
        headlineContent = { Text(title, style = MavType.label) },
        supportingContent = detail?.let { { Text(it, style = MavType.sub) } },
        trailingContent = trailing,
        colors = ListItemDefaults.colors(
            containerColor = Color.Transparent,
            headlineColor = palette.ink,
            supportingColor = palette.inkSecondary,
            trailingIconColor = palette.inkSecondary,
        ),
        modifier = modifier.fillMaxWidth(),
    )
}

/** A row that pushes somewhere. */
@Composable
fun MavNavRow(title: String, detail: String? = null, onClick: () -> Unit) {
    MavRow(
        title = title,
        detail = detail,
        modifier = Modifier.clickable(onClick = onClick),
        trailing = {
            Icon(MavIcons.chevronRight, contentDescription = null, modifier = Modifier.size(20.dp))
        },
    )
}

/** A switch row. Material's `Switch` carries the on/off state for accessibility services. */
@Composable
fun MavToggleRow(
    title: String,
    detail: String? = null,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
) {
    val palette = MavTheme.palette
    MavRow(
        title = title,
        detail = detail,
        trailing = {
            Switch(
                checked = checked,
                onCheckedChange = onCheckedChange,
                colors = SwitchDefaults.colors(
                    checkedTrackColor = palette.accent,
                    checkedThumbColor = palette.onAccent,
                    uncheckedTrackColor = palette.raised,
                    // The default off-thumb is Material's `outline`, which is invisible on this
                    // canvas - an unreadable switch is worse than no switch.
                    uncheckedThumbColor = palette.inkSecondary,
                    uncheckedBorderColor = palette.hairlineStrong,
                ),
            )
        },
    )
}

/**
 * A capability chip — Material's `AssistChip`. `enabled = false` reads as struck through *and* says
 * so, because a strikethrough alone is a colour-free but still purely visual signal.
 */
@Composable
fun MavChip(text: String, enabled: Boolean = true) {
    val palette = MavTheme.palette
    Surface(
        shape = RoundedCornerShape(MavTheme.chipRadius),
        color = palette.raised,
        contentColor = palette.inkSecondary,
        border = BorderStroke(1.dp, palette.hairline),
        modifier = Modifier.semantics {
            contentDescription = if (enabled) text else "$text, not provided"
        },
    ) {
            Text(
                text,
                style = MavType.sub.copy(
                    textDecoration = if (enabled) null else TextDecoration.LineThrough,
                ),
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
            )
    }
}

/** A compact secondary marker. */
@Composable
fun MavBadge(text: String) {
    val palette = MavTheme.palette
    Text(
        text = text,
        style = MavType.caption,
        color = palette.inkSecondary,
        modifier = Modifier
            .clip(RoundedCornerShape(MavTheme.chipRadius))
            .background(palette.raised)
            .padding(horizontal = 9.dp, vertical = 5.dp),
    )
}

@Composable
fun MavDivider() {
    HorizontalDivider(
        color = MavTheme.palette.hairline,
        modifier = Modifier.padding(start = MavTheme.tilePadding),
    )
}

/**
 * The primary affordance. Material's filled `Button`, which is where the accent belongs — one
 * affirmative action per screen and nothing else in that hue.
 */
@Composable
fun MavPrimaryButton(title: String, detail: String? = null, onClick: () -> Unit) {
    val palette = MavTheme.palette
    Button(
        onClick = onClick,
        modifier = Modifier
            .fillMaxWidth()
            .semantics(mergeDescendants = true) {
                contentDescription = if (detail == null) title else "$title. $detail"
            },
        shape = RoundedCornerShape(MavTheme.pillRadius),
        colors = ButtonDefaults.buttonColors(
            containerColor = palette.accent,
            contentColor = palette.onAccent,
        ),
        contentPadding = PaddingValues(horizontal = 18.dp, vertical = 14.dp),
    ) {
        Icon(MavIcons.play, contentDescription = null, modifier = Modifier.size(20.dp))
        Column(Modifier.padding(start = 14.dp).weight(1f)) {
            Text(title, style = MavType.label, color = palette.onAccent)
            if (detail != null) {
                Text(detail, style = MavType.sub, color = palette.onAccent.copy(alpha = 0.75f))
            }
        }
        Icon(MavIcons.chevronRight, contentDescription = null, modifier = Modifier.size(20.dp))
    }
}

/** A quiet secondary action — Material's `TextButton`. */
@Composable
fun MavQuietButton(title: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    TextButton(
        onClick = onClick,
        modifier = modifier,
        colors = ButtonDefaults.textButtonColors(contentColor = MavTheme.palette.inkSecondary),
    ) {
        Text(title, style = MavType.body)
    }
}

/**
 * A full-width action. Outlined by default; filled when it is the affirmative one. Both are
 * Material's own, so the disabled state and the error role come from the platform.
 */
@Composable
fun MavWideButton(
    title: String,
    modifier: Modifier = Modifier,
    destructive: Boolean = false,
    prominent: Boolean = false,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    val palette = MavTheme.palette
    if (prominent) {
        Button(
            onClick = onClick,
            modifier = modifier,
            enabled = enabled,
            shape = RoundedCornerShape(MavTheme.tileRadius),
            colors = ButtonDefaults.buttonColors(
                containerColor = palette.accent,
                contentColor = palette.onAccent,
            ),
        ) { Text(title, style = MavType.label) }
    } else {
        OutlinedButton(
            onClick = onClick,
            modifier = modifier,
            enabled = enabled,
            shape = RoundedCornerShape(MavTheme.tileRadius),
            colors = ButtonDefaults.outlinedButtonColors(
                contentColor = if (destructive) MaterialTheme.colorScheme.error else palette.ink,
            ),
            border = BorderStroke(1.dp, palette.hairlineStrong),
        ) { Text(title, style = MavType.label) }
    }
}

/**
 * The only two places a status hue may touch ink rather than a surface. Both are cases where there
 * is no surface underneath to tint and the meaning *is* the colour: a destructive action's label,
 * and the live-link dot on the strap glyph. Any third use is a bug.
 */
@Composable
fun destructiveInk(): Color = MaterialTheme.colorScheme.error

@Composable
fun mavLiveInk(): Color =
    if (MavTheme.palette.dark) {
        Color(0xFF000000L or com.sennnen.mav.ui.aura.AuraTokens.goodDark)
    } else {
        Color(0xFF000000L or com.sennnen.mav.ui.aura.AuraTokens.goodLight)
    }

/** A flowing row of chips — Material's `FlowRow`, so it wraps by measurement, not by guesswork. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun MavFlowRow(items: List<String>, chip: @Composable (String) -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
        items.forEach { chip(it) }
    }
}
