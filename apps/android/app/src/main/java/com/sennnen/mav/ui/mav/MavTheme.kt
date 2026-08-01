package com.sennnen.mav.ui.mav

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.sennnen.mav.ui.AppearanceMode
import com.sennnen.mav.ui.AppearancePrefs
import com.sennnen.mav.ui.aura.AuraTokens
import androidx.core.view.WindowCompat
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

// The Terrain design language, Android side. The iOS counterpart is UI/MavTheme.swift.
//
// The two platforms share shape, rhythm, type roles and meaning. They deliberately do NOT share
// colour: iOS renders the designed palette from tokens/aura.json, Android takes its surfaces, ink
// and accent from Material You. See the Material You section below for why, and for what that
// costs. Tokens remain the source for everything that encodes meaning rather than taste - status
// washes, family pigments, the photographic veils - and the whole palette on API < 31.
//
// This file owns exactly what a token cannot: the two font families, the type roles, the
// status-to-tint lookup, and the runtime contrast clamp that stands in for the build-time gate on
// the one platform the gate cannot see.
//
// The rule the whole language rests on: it is monochromatic. There is one hue - a deep stone teal -
// and everything on screen is a weight of it. Family stays semantic data for choosing an icon and
// its copy; it never selects a colour. A pass that gave each of the seven families its own pigment
// turned a calm screen into a chart of unrelated colours, and MavThemeTest now asserts the families
// resolve to one value so that cannot come back by accident.
//
// There is exactly one exception, and it is a safety affordance rather than decoration: the
// destructive hue is red. When it matched body text, "Delete device" was indistinguishable from a
// caption. Any second exception is a bug.

private fun hex(v: Long): Color = Color(0xFF000000L or v)

private fun hexA(v: Long, alpha: Float): Color = hex(v).copy(alpha = alpha)

// ---------------------------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------------------------

@Immutable
data class MavPalette(
    val dark: Boolean,
    // Surfaces, canvas outward.
    val canvas: Color,
    val surface: Color,
    val raised: Color,
    val sunken: Color,
    // Ink. Two weights, and that is a contrast finding rather than a preference: a third, fainter
    // weight cannot clear 4.5:1 on these surfaces, and every string here carries information.
    // Hierarchy comes from size, case and tracking.
    val ink: Color,
    val inkSecondary: Color,
    /** The single interaction hue. Never a status, never decoration. */
    val accent: Color,
    /**
     * The hue every data mark is drawn in - chart traces, bars, family glyphs.
     *
     * It lives on the palette rather than on `MavFamily` because under Material You it has to
     * follow the system's primary. A mark read straight from the token file would stay teal on a
     * wallpaper-derived surface, and the app would have two hues - which is exactly the failure the
     * monochromatic rule exists to prevent.
     */
    val mark: Color,
    /** Ink for content sitting on the accent. */
    val onAccent: Color,
    /** Deliberately not the accent, so a focused accent control stays visible. */
    val focus: Color,
    val hairline: Color,
    val hairlineStrong: Color,
    val glass: Color,
    val glassLine: Color,
    val grid: Color,
    /** Dim behind a presented sheet. */
    val scrim: Color,
    /** The wash over a photograph carrying white copy, which makes contrast a constant. */
    val photoScrim: Color,
    /** The heavier veil that lets ordinary ink sit on a landscape, so a metric row can carry one. */
    val photoVeil: Color,
    // The atmosphere behind a tab root. See MavAtmosphere.
    val bloomTop: Color,
    val bloomBottom: Color,
    /**
     * The seven metric washes. A surface is the only legitimate place for these, and each names
     * **which metric** rather than how it is doing. Colouring a surface by verdict made a bad night
     * look alarming before the number was read, and made the same card change colour day to day so
     * nothing was recognisable by sight.
     */
    val tintCharge: Color,
    val tintRest: Color,
    val tintEffort: Color,
    val tintHeart: Color,
    val tintEnergy: Color,
    val tintVitals: Color,
    val tintCycle: Color,
    /** For a card that is not a metric at all - a connector, a device, a prompt. */
    val tintNeutral: Color,
)

private val MavDarkPalette = MavPalette(
    dark = true,
    canvas = hex(AuraTokens.bgDark),
    surface = hex(AuraTokens.cardDark),
    raised = hex(AuraTokens.cardEdgeDark),
    sunken = hex(AuraTokens.sunkenDark),
    ink = hex(AuraTokens.inkDark),
    inkSecondary = hexA(AuraTokens.inkSecondaryDark, AuraTokens.inkSecondaryDarkAlpha),
    accent = hex(AuraTokens.accentInkDark),
    mark = hex(AuraTokens.vitalsGlowDark),
    onAccent = hex(AuraTokens.bgDark),
    focus = hex(AuraTokens.focusDark),
    hairline = hexA(AuraTokens.hairlineDark, AuraTokens.hairlineDarkAlpha),
    hairlineStrong = hexA(AuraTokens.hairlineStrongDark, AuraTokens.hairlineStrongDarkAlpha),
    glass = hexA(AuraTokens.glassDark, AuraTokens.glassDarkAlpha),
    glassLine = hexA(AuraTokens.glassLineDark, AuraTokens.glassLineDarkAlpha),
    grid = hexA(AuraTokens.gridDark, AuraTokens.gridDarkAlpha),
    scrim = hexA(AuraTokens.scrimDark, AuraTokens.scrimDarkAlpha),
    photoScrim = hexA(AuraTokens.photoScrimDark, AuraTokens.photoScrimDarkAlpha),
    photoVeil = hexA(AuraTokens.photoVeilDark, AuraTokens.photoVeilDarkAlpha),
    bloomTop = hexA(AuraTokens.bloomTopDark, AuraTokens.bloomTopDarkAlpha),
    bloomBottom = hexA(AuraTokens.bloomBottomDark, AuraTokens.bloomBottomDarkAlpha),
    tintCharge = hexA(AuraTokens.tintChargeDark, AuraTokens.tintChargeDarkAlpha),
    tintRest = hexA(AuraTokens.tintRestDark, AuraTokens.tintRestDarkAlpha),
    tintEffort = hexA(AuraTokens.tintEffortDark, AuraTokens.tintEffortDarkAlpha),
    tintHeart = hexA(AuraTokens.tintHeartDark, AuraTokens.tintHeartDarkAlpha),
    tintEnergy = hexA(AuraTokens.tintEnergyDark, AuraTokens.tintEnergyDarkAlpha),
    tintVitals = hexA(AuraTokens.tintVitalsDark, AuraTokens.tintVitalsDarkAlpha),
    tintCycle = hexA(AuraTokens.tintCycleDark, AuraTokens.tintCycleDarkAlpha),
    tintNeutral = hexA(AuraTokens.tintNeutralDark, AuraTokens.tintNeutralDarkAlpha),
)

private val MavLightPalette = MavPalette(
    dark = false,
    canvas = hex(AuraTokens.bgLight),
    surface = hex(AuraTokens.cardLight),
    raised = hex(AuraTokens.cardEdgeLight),
    sunken = hex(AuraTokens.sunkenLight),
    ink = hex(AuraTokens.inkLight),
    inkSecondary = hexA(AuraTokens.inkSecondaryLight, AuraTokens.inkSecondaryLightAlpha),
    accent = hex(AuraTokens.accentInkLight),
    mark = hex(AuraTokens.vitalsGlowLight),
    onAccent = hex(AuraTokens.bgLight),
    focus = hex(AuraTokens.focusLight),
    hairline = hexA(AuraTokens.hairlineLight, AuraTokens.hairlineLightAlpha),
    hairlineStrong = hexA(AuraTokens.hairlineStrongLight, AuraTokens.hairlineStrongLightAlpha),
    glass = hexA(AuraTokens.glassLight, AuraTokens.glassLightAlpha),
    glassLine = hexA(AuraTokens.glassLineLight, AuraTokens.glassLineLightAlpha),
    grid = hexA(AuraTokens.gridLight, AuraTokens.gridLightAlpha),
    scrim = hexA(AuraTokens.scrimLight, AuraTokens.scrimLightAlpha),
    photoScrim = hexA(AuraTokens.photoScrimLight, AuraTokens.photoScrimLightAlpha),
    photoVeil = hexA(AuraTokens.photoVeilLight, AuraTokens.photoVeilLightAlpha),
    bloomTop = hexA(AuraTokens.bloomTopLight, AuraTokens.bloomTopLightAlpha),
    bloomBottom = hexA(AuraTokens.bloomBottomLight, AuraTokens.bloomBottomLightAlpha),
    tintCharge = hexA(AuraTokens.tintChargeLight, AuraTokens.tintChargeLightAlpha),
    tintRest = hexA(AuraTokens.tintRestLight, AuraTokens.tintRestLightAlpha),
    tintEffort = hexA(AuraTokens.tintEffortLight, AuraTokens.tintEffortLightAlpha),
    tintHeart = hexA(AuraTokens.tintHeartLight, AuraTokens.tintHeartLightAlpha),
    tintEnergy = hexA(AuraTokens.tintEnergyLight, AuraTokens.tintEnergyLightAlpha),
    tintVitals = hexA(AuraTokens.tintVitalsLight, AuraTokens.tintVitalsLightAlpha),
    tintCycle = hexA(AuraTokens.tintCycleLight, AuraTokens.tintCycleLightAlpha),
    tintNeutral = hexA(AuraTokens.tintNeutralLight, AuraTokens.tintNeutralLightAlpha),
)

fun mavPalette(dark: Boolean): MavPalette = if (dark) MavDarkPalette else MavLightPalette

// ---------------------------------------------------------------------------------------------
// Material You
// ---------------------------------------------------------------------------------------------
//
// Android takes its surfaces, its ink and its accent from the system - the user's wallpaper and
// theme - rather than from tokens/aura.json. That is a deliberate divergence from iOS and the one
// place the two platforms are allowed to disagree on colour: a designed palette is right on a
// platform with one look, and native theming is right on a platform where the user picked one.
//
// The consequence has to be handled rather than hoped away. tools/check_a11y.py reasons about the
// token file, so it cannot see a wallpaper, and a dynamic scheme carries no promise that its ink
// clears 7:1 on its own background. Everything derived below is therefore passed through
// [clampInk] / [clampAlphaInk], which push a weight toward black or white until it clears the same
// ratio the gate enforces on iOS. The gate checks the palette we ship; this checks the one the
// phone hands us.
//
// What does NOT come from the system: status washes and family pigments. Material You has no
// notion of "this recovery score is low", and a wallpaper-derived tertiary would say it in a
// colour that means nothing. Those stay Terrain's, over system surfaces.

private fun channel(v: Float): Float =
    if (v <= 0.04045f) v / 12.92f else ((v + 0.055f) / 1.055f).toDouble().pow(2.4).toFloat()

private fun luminance(c: Color): Float =
    0.2126f * channel(c.red) + 0.7152f * channel(c.green) + 0.0722f * channel(c.blue)

private fun contrastRatio(a: Color, b: Color): Float {
    val la = luminance(a)
    val lb = luminance(b)
    return (max(la, lb) + 0.05f) / (min(la, lb) + 0.05f)
}

/** Source-over, which is what the renderer does when it draws a translucent fill. */
private fun compositeOver(fg: Color, bg: Color): Color = Color(
    red = fg.red * fg.alpha + bg.red * (1f - fg.alpha),
    green = fg.green * fg.alpha + bg.green * (1f - fg.alpha),
    blue = fg.blue * fg.alpha + bg.blue * (1f - fg.alpha),
    alpha = 1f,
)

/** The surface an ink weight has the hardest time on, which is the only one worth clamping to. */
private fun hardestSurface(ink: Color, surfaces: List<Color>): Color =
    surfaces.minByOrNull { contrastRatio(ink, it) } ?: surfaces.first()

/**
 * Push [ink] toward whichever pole its surface is not, until it clears [minimum].
 *
 * Bisection rather than a fixed step: it converges on the *least* correction that satisfies the
 * ratio, so a wallpaper that was already close keeps almost all of its character.
 */
private fun clampInk(ink: Color, on: Color, minimum: Float): Color {
    if (contrastRatio(ink, on) >= minimum) return ink
    val pole = if (luminance(on) > 0.18f) Color.Black else Color.White
    var lo = 0f
    var hi = 1f
    repeat(12) {
        val mid = (lo + hi) / 2f
        if (contrastRatio(lerp(ink, pole, mid), on) >= minimum) hi = mid else lo = mid
    }
    return lerp(ink, pole, hi)
}

/**
 * Raise a translucent weight's alpha until its composite clears [minimum].
 *
 * The hue stays the system's; only how much of the surface shows through changes. Terminates
 * because [ink] itself has already been clamped, so alpha 1 always satisfies the ratio.
 */
private fun clampAlphaInk(ink: Color, on: Color, alpha: Float, minimum: Float): Color {
    var a = alpha
    repeat(16) {
        if (contrastRatio(compositeOver(ink.copy(alpha = a), on), on) >= minimum) {
            return ink.copy(alpha = a)
        }
        a = min(1f, a + 0.03f)
    }
    return ink.copy(alpha = 1f)
}

/**
 * Terrain's shape, the system's colour.
 *
 * Surfaces, ink and accent are read from [scheme]; washes, pigments and the photographic veils
 * stay Terrain's, because they encode meaning the system does not have. [fallback] supplies those
 * and is also the whole palette on API < 31, where there is no dynamic scheme to read.
 */
private fun dynamicPalette(scheme: ColorScheme, fallback: MavPalette): MavPalette {
    val canvas = scheme.background
    val surface = scheme.surfaceContainer
    val raised = scheme.surfaceContainerHigh
    val sunken = scheme.surfaceContainerLowest
    val surfaces = listOf(canvas, surface, raised, sunken)

    val ink = clampInk(scheme.onBackground, hardestSurface(scheme.onBackground, surfaces), 7.0f)
    val inkSecondary = clampAlphaInk(
        ink,
        hardestSurface(ink, surfaces),
        fallback.inkSecondary.alpha,
        4.5f,
    )
    // The ring must clear its surface without being the accent, so a focused accent control stays
    // visible. Ink already clears every surface by a wide margin, so it is the safe ring.
    val focus = ink

    return fallback.copy(
        canvas = canvas,
        surface = surface,
        raised = raised,
        sunken = sunken,
        ink = ink,
        inkSecondary = inkSecondary,
        accent = scheme.primary,
        // One hue: the mark is the accent. Clamped to 3:1 on the card so a pale wallpaper
        // cannot produce a chart line nobody can see.
        mark = clampInk(scheme.primary, surface, 3.0f),
        onAccent = scheme.onPrimary,
        focus = focus,
        hairline = ink.copy(alpha = fallback.hairline.alpha),
        hairlineStrong = ink.copy(alpha = fallback.hairlineStrong.alpha),
        glass = ink.copy(alpha = fallback.glass.alpha),
        glassLine = ink.copy(alpha = fallback.glassLine.alpha),
        grid = ink.copy(alpha = fallback.grid.alpha),
        // The veil composites toward the canvas, so it has to follow the canvas rather than the
        // token's own near-black. Its alpha is what the iOS gate proved safe.
        photoVeil = canvas.copy(alpha = fallback.photoVeil.alpha),
        bloomTop = scheme.primary.copy(alpha = fallback.bloomTop.alpha),
        bloomBottom = scheme.primary.copy(alpha = fallback.bloomBottom.alpha),
        // A photograph has to belong to the palette rather than sit outside it, so the hero scrim
        // is the system primary darkened rather than a neutral black. Compositing toward black
        // keeps white copy legible whatever the wallpaper turned out to be.
        photoScrim = lerp(scheme.primary, Color.Black, 0.72f)
            .copy(alpha = fallback.photoScrim.alpha),
        // The seven washes are the system primary at the token's alphas, so metric identity stays
        // readable as one hue with the rest of the theme.
        tintCharge = scheme.primary.copy(alpha = fallback.tintCharge.alpha),
        tintRest = scheme.primary.copy(alpha = fallback.tintRest.alpha),
        tintEffort = scheme.primary.copy(alpha = fallback.tintEffort.alpha),
        tintHeart = scheme.primary.copy(alpha = fallback.tintHeart.alpha),
        tintEnergy = scheme.primary.copy(alpha = fallback.tintEnergy.alpha),
        tintVitals = scheme.primary.copy(alpha = fallback.tintVitals.alpha),
        tintCycle = scheme.primary.copy(alpha = fallback.tintCycle.alpha),
    )
}

val LocalMavPalette = staticCompositionLocalOf { MavDarkPalette }

object MavTheme {
    val palette: MavPalette
        @Composable @ReadOnlyComposable get() = LocalMavPalette.current

    // Shape and rhythm.
    val screenMargin = AuraTokens.screenMargin
    val cardSpacing = AuraTokens.cardSpacing
    val sectionGap = AuraTokens.sectionGap
    val tilePadding = AuraTokens.tilePadding
    val railGap = AuraTokens.railGap
    val cardRadius = AuraTokens.cardRadius
    val tileRadius = AuraTokens.tileRadius
    val pillRadius = AuraTokens.pillRadius
    val chipRadius = AuraTokens.chipRadius

    /** The one focus ring width, matched to iOS. */
    val focusRingWidth = 2.5.dp
    val focusRingInset = 3.dp
}

// ---------------------------------------------------------------------------------------------
// Metric families
// ---------------------------------------------------------------------------------------------

/** A metric's identity. Seven of them, and `CYCLE` is the newest. */
enum class MavFamily {
    CHARGE, REST, EFFORT, HEART, ENERGY, VITALS, CYCLE;

    /** The family's own pigment, for the data mark and nothing else. Cleared against the card
     *  in both schemes by tools/check_a11y.py. */
    fun hue(dark: Boolean): Color = when (this) {
        CHARGE -> if (dark) hex(AuraTokens.chargeGlowDark) else hex(AuraTokens.chargeGlowLight)
        REST -> if (dark) hex(AuraTokens.restGlowDark) else hex(AuraTokens.restGlowLight)
        EFFORT -> if (dark) hex(AuraTokens.effortGlowDark) else hex(AuraTokens.effortGlowLight)
        HEART -> if (dark) hex(AuraTokens.heartGlowDark) else hex(AuraTokens.heartGlowLight)
        ENERGY -> if (dark) hex(AuraTokens.energyGlowDark) else hex(AuraTokens.energyGlowLight)
        VITALS -> if (dark) hex(AuraTokens.vitalsGlowDark) else hex(AuraTokens.vitalsGlowLight)
        CYCLE -> if (dark) hex(AuraTokens.cycleGlowDark) else hex(AuraTokens.cycleGlowLight)
    }

    /** The deep wash the pigment blooms out of. A backdrop, never a text surface. */
    fun wash(dark: Boolean): Color = when (this) {
        CHARGE -> if (dark) hex(AuraTokens.chargeEdgeDark) else hex(AuraTokens.chargeEdgeLight)
        REST -> if (dark) hex(AuraTokens.restEdgeDark) else hex(AuraTokens.restEdgeLight)
        EFFORT -> if (dark) hex(AuraTokens.effortEdgeDark) else hex(AuraTokens.effortEdgeLight)
        HEART -> if (dark) hex(AuraTokens.heartEdgeDark) else hex(AuraTokens.heartEdgeLight)
        ENERGY -> if (dark) hex(AuraTokens.energyEdgeDark) else hex(AuraTokens.energyEdgeLight)
        VITALS -> if (dark) hex(AuraTokens.vitalsEdgeDark) else hex(AuraTokens.vitalsEdgeLight)
        CYCLE -> if (dark) hex(AuraTokens.cycleEdgeDark) else hex(AuraTokens.cycleEdgeLight)
    }

    /**
     * The one hue, from the palette - so it follows Material You on Android instead of staying
     * the token teal on a wallpaper-derived surface. `hue(dark)` remains the token-file answer and
     * is what the tests assert against.
     */
    val hue: Color @Composable @ReadOnlyComposable get() = LocalMavPalette.current.mark

    /** The wash this metric's card carries. Identity, never a verdict. */
    fun tint(palette: MavPalette): Color = when (this) {
        CHARGE -> palette.tintCharge
        REST -> palette.tintRest
        EFFORT -> palette.tintEffort
        HEART -> palette.tintHeart
        ENERGY -> palette.tintEnergy
        VITALS -> palette.tintVitals
        CYCLE -> palette.tintCycle
    }

    val tint: Color @Composable @ReadOnlyComposable get() = tint(LocalMavPalette.current)
    val wash: Color @Composable @ReadOnlyComposable get() = wash(LocalMavPalette.current.dark)

    companion object {
        /**
         * The core hands back a category string; anything unrecognised reads as a general vital
         * rather than inventing a family for it.
         */
        fun of(category: String): MavFamily = when (category) {
            "Charge", "Recovery" -> CHARGE
            "Rest", "Sleep" -> REST
            "Effort", "Strain" -> EFFORT
            "Heart", "Cardio" -> HEART
            "Nutrition", "Mind", "Energy" -> ENERGY
            "Cycle" -> CYCLE
            else -> VITALS
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------------------------

/**
 * A judgement, at the only granularity a surface tint can express.
 *
 * The *word* shown beside a value is not derived from this - it comes from the core's band, so
 * "In range", "Elevated", "Building" and "Provisional" all reach the screen as text the core
 * supplied. This enum decides one thing: which wash the card's surface carries.
 */
enum class MavStatus {
    OPTIMAL, FAIR, LOW, NEUTRAL;

    /**
     * The last-resort word, used only where the core supplied no band. A metric that has a band
     * shows the core's wording instead, because a status word is a claim about the value.
     */
    val fallbackWord: String
        get() = when (this) {
            OPTIMAL -> "Optimal"
            FAIR -> "Fair"
            LOW -> "Pay attention"
            NEUTRAL -> "No data"
        }
}

// ---------------------------------------------------------------------------------------------
// Type
// ---------------------------------------------------------------------------------------------

/**
 * The two faces, and nothing else may be used. Both belong to the platform, and nothing ships
 * inside the APK.
 *
 * Serif was briefly Old Standard TT, bundled. It went for two reasons. It is a Didone revival -
 * hairline strokes and high stroke contrast - which at display sizes on a phone reads thin and
 * academic rather than modern; and iOS uses New York, so the one role carrying the brand looked
 * like a different product on each platform. Apple's faces are licensed for Apple platforms only,
 * so matching them here is not an option. Each platform now uses its own system serif, which is
 * the honest version of parity: same role, same weight, each in its own native voice.
 */
val MavSerif = FontFamily.Serif

val MavSans = FontFamily.Default

/**
 * Serif is a rare editorial accent. Health data, controls and navigation use the platform sans,
 * matching Material 3's native voice and keeping compact screens readable.
 *
 * Sizes are `sp`, so they scale with the system font scale without any further plumbing. Letter
 * spacing is expressed in `em` so it scales with them.
 */
@Immutable
data class MavTypography(
    val displayLarge: TextStyle,
    val display: TextStyle,
    val numeralXL: TextStyle,
    val numeralLarge: TextStyle,
    val numeralMedium: TextStyle,
    val numeralSmall: TextStyle,
    val title: TextStyle,
    val label: TextStyle,
    val body: TextStyle,
    val sub: TextStyle,
    val caption: TextStyle,
    /** Compact metadata. Callers keep it sentence case. */
    val eyebrow: TextStyle,
)

private fun serif(size: TextUnit, weight: FontWeight, tracking: Float, lineHeight: Float) =
    TextStyle(
        fontFamily = MavSerif,
        fontWeight = weight,
        fontSize = size,
        letterSpacing = tracking.em,
        lineHeight = size * lineHeight,
    )

private fun sans(size: TextUnit, weight: FontWeight, tracking: Float, lineHeight: Float) =
    TextStyle(
        fontFamily = MavSans,
        fontWeight = weight,
        fontSize = size,
        letterSpacing = tracking.em,
        lineHeight = size * lineHeight,
    )

val MavType = MavTypography(
    displayLarge = serif(AuraTokens.displayLargeSize, FontWeight.Normal, -0.01f, 1.12f),
    display = serif(AuraTokens.displaySize, FontWeight.Normal, -0.012f, 1.16f),
    numeralXL = sans(AuraTokens.numeralXLSize, FontWeight.SemiBold, -0.04f, 1.0f),
    numeralLarge = sans(AuraTokens.numeralLargeSize, FontWeight.SemiBold, -0.03f, 1.0f),
    numeralMedium = sans(AuraTokens.numeralMediumSize, FontWeight.Medium, -0.02f, 1.1f),
    numeralSmall = sans(AuraTokens.numeralSmallSize, FontWeight.Medium, -0.02f, 1.1f),
    title = sans(AuraTokens.titleSize, FontWeight.Medium, -0.01f, 1.24f),
    label = sans(AuraTokens.labelSize, FontWeight.Medium, 0f, 1.28f),
    body = sans(AuraTokens.bodySize, FontWeight.Normal, 0f, 1.45f),
    sub = sans(AuraTokens.subSize, FontWeight.Normal, 0f, 1.38f),
    caption = sans(AuraTokens.captionSize, FontWeight.Medium, 0.005f, 1.25f),
    eyebrow = sans(AuraTokens.eyebrowSize, FontWeight.Medium, 0f, 1.25f),
)

// ---------------------------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------------------------

/**
 * Terrain expressed as a Material 3 [ColorScheme].
 *
 * This is not decoration. Every Material component a screen uses - Switch, SegmentedButton,
 * TopAppBar, NavigationBar, ModalBottomSheet, ListItem - reads its colours from here, so using the
 * real components means getting the real palette rather than a hand-tinted lookalike. The mapping is
 * deliberate: `primary` is the single interaction hue, `error` is the destructive hue, and every
 * container role resolves to one of the three Terrain surfaces rather than to a Material default.
 */
private fun mavColorScheme(palette: MavPalette): ColorScheme {
    val base = if (palette.dark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = palette.accent,
        onPrimary = palette.onAccent,
        primaryContainer = palette.raised,
        onPrimaryContainer = palette.ink,
        secondary = palette.accent,
        onSecondary = palette.onAccent,
        // Left unmapped, these fall back to Material's own purple - which is what tinted the
        // segmented button's selection. Every container role resolves to a Terrain surface.
        secondaryContainer = palette.raised,
        onSecondaryContainer = palette.ink,
        tertiary = palette.accent,
        onTertiary = palette.onAccent,
        tertiaryContainer = palette.raised,
        onTertiaryContainer = palette.ink,
        inverseSurface = palette.ink,
        inverseOnSurface = palette.canvas,
        inversePrimary = palette.accent,
        errorContainer = palette.raised,
        background = palette.canvas,
        onBackground = palette.ink,
        surface = palette.surface,
        onSurface = palette.ink,
        surfaceVariant = palette.raised,
        onSurfaceVariant = palette.inkSecondary,
        surfaceContainerLowest = palette.sunken,
        surfaceContainerLow = palette.canvas,
        surfaceContainer = palette.surface,
        surfaceContainerHigh = palette.raised,
        surfaceContainerHighest = palette.raised,
        outline = palette.hairlineStrong,
        outlineVariant = palette.hairline,
        scrim = palette.scrim,
        error = if (palette.dark) {
            hex(AuraTokens.badDark)
        } else {
            hex(AuraTokens.badLight)
        },
        onError = palette.onAccent,
    )
}

/** The Terrain type roles, mapped onto Material's slot names so M3 components inherit them. */
private fun mavMaterialTypography(): Typography = Typography(
    displayLarge = MavType.displayLarge,
    displayMedium = MavType.display,
    displaySmall = MavType.display,
    headlineLarge = MavType.display,
    headlineMedium = MavType.title,
    headlineSmall = MavType.title,
    titleLarge = MavType.title,
    titleMedium = MavType.label,
    titleSmall = MavType.label,
    bodyLarge = MavType.body,
    bodyMedium = MavType.body,
    bodySmall = MavType.sub,
    labelLarge = MavType.label,
    labelMedium = MavType.caption,
    labelSmall = MavType.eyebrow,
)

@Composable
fun MavTheme(content: @Composable () -> Unit) {
    val dark = when (AppearancePrefs.mode) {
        AppearanceMode.DARK -> true
        AppearanceMode.LIGHT -> false
        AppearanceMode.SYSTEM -> isSystemInDarkTheme()
    }
    val context = LocalContext.current
    val fallback = mavPalette(dark)
    // Material You from API 31. Below it there is no dynamic scheme to read, and the token palette
    // is the whole answer rather than a degraded one.
    val palette = remember(dark, context) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val scheme = if (dark) {
                dynamicDarkColorScheme(context)
            } else {
                dynamicLightColorScheme(context)
            }
            dynamicPalette(scheme, fallback)
        } else {
            fallback
        }
    }
    val view = LocalView.current
    SideEffect {
        view.context.findActivity()?.let { activity ->
            WindowCompat.getInsetsController(activity.window, view).apply {
                isAppearanceLightStatusBars = !dark
                isAppearanceLightNavigationBars = !dark
            }
        }
    }
    CompositionLocalProvider(LocalMavPalette provides palette) {
        MaterialTheme(
            colorScheme = mavColorScheme(palette),
            typography = mavMaterialTypography(),
            content = content,
        )
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
