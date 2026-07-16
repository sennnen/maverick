package com.sennnen.mav.aura

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.scale
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

// Surfaces + sheet chrome + motion (Android port of Strand/UI/AuraKit.swift).
// Luminous radial-glow content tiles, a near-black screen backdrop; "glass"
// chrome is a translucent scrim capsule (no haze library vendored).

// MARK: - Glass chrome (pills, tab bar, FAB only)

/** Chrome surface for pills/chips/nav/FAB. Material 3 direction: a solid tonal `surfaceContainerHigh`
 *  fill (no translucent "glass" — Android is native Material, not an iOS Liquid-Glass clone). */
@Composable
fun Modifier.auraGlass(shape: Shape = CircleShape): Modifier {
    val scheme = MaterialTheme.colorScheme
    return this
        .clip(shape)
        .background(scheme.surfaceContainerHigh, shape)
}

// MARK: - Radial-glow tile — Material3 Card with glow drawBehind

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AuraGlowTile(
    family: AuraFamily? = null,
    modifier: Modifier = Modifier,
    padding: Dp = Aura.tilePadding,
    radius: Dp = Aura.tileRadius,
    onClick: (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    // An M3 `ElevatedCard` on tonal `surfaceContainer` wearing the family's SUBTLE accent glow via
    // `drawBehind` (the Aura direction: M3 component, themed paint — never a full-bleed gradient):
    // a soft halo blooming past the card edge onto the black canvas, plus a faint interior tint
    // rising from the top-leading corner.
    val shape = RoundedCornerShape(radius)
    val scheme = MaterialTheme.colorScheme
    val dark = Aura.palette.dark
    val glow = family?.glow(dark)
    val colors = CardDefaults.elevatedCardColors(
        containerColor = scheme.surfaceContainer,
        contentColor = scheme.onSurface,
    )
    val cardMod = modifier
        .fillMaxWidth()
        .then(
            if (glow != null) Modifier.drawBehind {
                // Halo past the card bounds — the card's opaque surface covers the interior,
                // so only the bloom onto the backdrop reads.
                val r = size.maxDimension * 0.72f
                drawCircle(
                    Brush.radialGradient(
                        listOf(glow.copy(alpha = if (dark) 0.28f else 0.18f), Color.Transparent),
                        center = center, radius = r,
                    ),
                    radius = r, center = center,
                )
            } else Modifier,
        )
    val inner: @Composable () -> Unit = {
        Column(
            Modifier
                .then(
                    if (glow != null) Modifier.drawBehind {
                        // Faint interior tint from the top-leading corner (clipped by the card shape).
                        val r = size.maxDimension * 1.05f
                        drawRect(
                            Brush.radialGradient(
                                listOf(glow.copy(alpha = if (dark) 0.20f else 0.10f), Color.Transparent),
                                center = Offset(size.width * 0.12f, 0f), radius = r,
                            ),
                        )
                    } else Modifier,
                )
                .padding(padding),
            content = content,
        )
    }
    if (onClick != null) {
        ElevatedCard(onClick = onClick, modifier = cardMod, shape = shape, colors = colors) { inner() }
    } else {
        ElevatedCard(modifier = cardMod, shape = shape, colors = colors) { inner() }
    }
}

// MARK: - Dark card (Material3 Card, Aura tokens via ColorScheme)
//
// Now reads Aura colours from the Material3 ColorScheme (surfaceContainerLow
// and surfaceVariant are already mapped to p.card in auraColorScheme), so the
// card gets M3 elevation/shadow/ripple semantics for free. The explicit colour
// override remains as a guard for when no AuraTheme wrapper is active.

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AuraDarkCard(
    modifier: Modifier = Modifier,
    padding: Dp = 18.dp,
    onClick: (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    val shape = RoundedCornerShape(Aura.tileRadius)
    val scheme = MaterialTheme.colorScheme
    // Material 3: a plain filled Card on `surfaceContainerLow` (M3 tonal elevation, no hairline border).
    val colors = CardDefaults.cardColors(
        containerColor = scheme.surfaceContainerLow,
        contentColor = scheme.onSurface,
    )
    if (onClick != null) {
        Card(
            onClick = onClick, modifier = modifier.fillMaxWidth(),
            shape = shape, colors = colors,
        ) { Column(Modifier.padding(padding), content = content) }
    } else {
        Card(
            modifier = modifier.fillMaxWidth(),
            shape = shape, colors = colors,
        ) { Column(Modifier.padding(padding), content = content) }
    }
}

// MARK: - Screen backdrop

/** The ONE custom Aura component M3 has no slot for: a `surface` canvas carrying a
 *  faint radial glow in the hub's lead hue, bleeding down from above the status bar
 *  and breathing slowly (additive blend, so it reads as emitted light on black).
 *  Kept subtle — the content stays stock M3; only the room's lighting changes. */
@Composable
fun AuraScreen(
    lead: AuraFamily? = null,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val dark = Aura.palette.dark
    val glow = lead?.glow(dark)
    val breathe = rememberInfiniteTransition(label = "auraBreathe")
    val glowLevel by breathe.animateFloat(
        initialValue = 0.7f, targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(6000), RepeatMode.Reverse),
        label = "auraBreatheLevel",
    )
    Box(
        modifier
            .fillMaxSize()
            .background(scheme.surface)
            .drawBehind {
                if (glow != null) {
                    val c = Offset(size.width / 2f, -size.width * 0.30f)
                    val r = size.width * 1.05f
                    drawCircle(
                        Brush.radialGradient(
                            listOf(
                                glow.copy(alpha = (if (dark) 0.16f else 0.08f) * glowLevel),
                                Color.Transparent,
                            ),
                            center = c, radius = r,
                        ),
                        radius = r, center = c,
                        blendMode = if (dark) BlendMode.Plus else BlendMode.SrcOver,
                    )
                }
            },
    ) { content() }
}

// MARK: - Sheet chrome (every flyout gets a title + close, no exceptions)

/**
 * The shared flyout container: `ModalBottomSheet` with themed backdrop, title
 * bar with an always-present ✕, scrollable content. Every sheet sits in one of
 * these so no flyout can trap the user.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AuraSheet(
    title: String,
    onDismiss: () -> Unit,
    family: AuraFamily? = null,
    scrolls: Boolean = true,
    content: @Composable ColumnScope.() -> Unit,
) {
    val p = Aura.palette
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp),
        containerColor = p.bg,
        contentColor = p.ink,
        dragHandle = {
            Box(
                Modifier
                    .padding(top = 10.dp)
                    .size(width = 36.dp, height = 4.dp)
                    .background(p.ink.copy(alpha = 0.25f), CircleShape),
            )
        },
    ) {
        AuraScreen(lead = family) {
            Column(Modifier.fillMaxWidth()) {
                AuraSheetBar(title = title, onClose = onDismiss)
                if (scrolls) {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .verticalScroll(rememberScrollState())
                            .padding(horizontal = Aura.screenMargin)
                            .padding(top = 4.dp, bottom = 48.dp),
                        verticalArrangement = Arrangement.spacedBy(Aura.sectionGap),
                        content = content,
                    )
                } else {
                    Column(Modifier.fillMaxWidth(), content = content)
                }
            }
        }
    }
}

/** Title + ✕ bar used by every sheet. */
@Composable
fun AuraSheetBar(title: String, onClose: () -> Unit) {
    val p = Aura.palette
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = Aura.screenMargin)
            .padding(top = 10.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = AuraType.heading(20.sp), color = p.ink)
        Spacer(Modifier.weight(1f))
        IconButton(
            onClick = onClose,
            modifier = Modifier
                .size(40.dp)
                .auraGlass(CircleShape)
                .semantics { contentDescription = "Close" },
        ) {
            Icon(Icons.Filled.Close, contentDescription = null, tint = p.ink, modifier = Modifier.size(18.dp))
        }
    }
}

// MARK: - Motion

/** Staggered card entrance: fade + 18dp rise, both SPRING-driven (feels alive, not
 *  eased), staggered [AuraMotion.STAGGER_MS] per index. */
@Composable
fun Modifier.auraReveal(revealed: Boolean, index: Int): Modifier {
    val alpha = remember { Animatable(0f) }
    val rise = remember { Animatable(1f) }
    LaunchedEffect(revealed) {
        if (revealed) {
            delay(index * AuraMotion.STAGGER_MS.toLong())
            launch { alpha.animateTo(1f, spring(dampingRatio = 0.9f, stiffness = Spring.StiffnessMediumLow)) }
            rise.animateTo(0f, spring(dampingRatio = 0.8f, stiffness = Spring.StiffnessMediumLow))
        } else {
            alpha.snapTo(0f)
            rise.snapTo(1f)
        }
    }
    val risePx = with(LocalDensity.current) { 18.dp.toPx() }
    return this.graphicsLayer {
        this.alpha = alpha.value
        translationY = rise.value * risePx
    }
}

/** Press scale (AuraPressStyle equivalent): 0.97 while pressed, springy — WITH the
 *  platform ripple (Material indication), so taps read as taps. */
@Composable
fun Modifier.auraPressable(
    interactionSource: MutableInteractionSource = remember { MutableInteractionSource() },
    onClick: () -> Unit,
): Modifier {
    val pressed by interactionSource.collectIsPressedAsState()
    val scale by animateFloatAsState(
        targetValue = if (pressed) 0.97f else 1f,
        animationSpec = spring(dampingRatio = 0.62f),
        label = "auraPress",
    )
    return this
        .scale(scale)
        .clickable(
            interactionSource = interactionSource,
            indication = androidx.compose.foundation.LocalIndication.current,
            onClick = onClick,
        )
}
