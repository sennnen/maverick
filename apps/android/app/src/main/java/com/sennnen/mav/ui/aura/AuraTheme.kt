package com.sennnen.mav.ui.aura

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.sennnen.mav.ui.AppearanceMode
import com.sennnen.mav.ui.AppearancePrefs
import com.sennnen.mav.ui.UnitPrefs
import com.sennnen.mav.ui.effortDisplayFactor
import java.util.Locale
import kotlin.math.abs
import kotlin.math.roundToInt

// MARK: - Aura design system (Android port of Strand/UI/AuraDesign.swift)
//
// Pure-black editorial canvas (with a light variant), rounded tiles carrying a
// luminous RADIAL COLOUR GLOW, thin large numerals, title-case labels, high
// contrast throughout. The type ramp keeps the iOS ROLES on the platform sans
// (Roboto); Helvetica Neue is iOS-only. Types are prefixed `Aura*`.

private fun hex(v: Long): Color = Color(0xFF000000L or v)
private fun hexA(v: Long, alpha: Float): Color = hex(v).copy(alpha = alpha)

// MARK: Palette (scheme-resolved token set)

@Immutable
data class AuraPalette(
    val dark: Boolean,
    val bg: Color,
    val card: Color,          // neutral (non-glow) card
    val cardEdge: Color,
    /** Value / label ink — strong text colour, reads on black and colour tiles alike. */
    val ink: Color,
    /** Starship — the single INTERACTIVE hue. Never a status or decorative colour. */
    val accent: Color,
    /** Starship as TEXT/ICON ink: raw hue on dark, olive shift on light. */
    val accentInk: Color,
    val good: Color,
    val fair: Color,
    val bad: Color,
    /** Card hairline border. Ink-based so it's visible on light too. */
    val hairline: Color,
    /** Translucent pill scrim that reads on BOTH the glow tiles and dark cards. */
    val scrim: Color,
    /** Chart gridline. */
    val grid: Color,
)

private val Starship = Color(0xFFE7FE55)

val AuraDarkPalette = AuraPalette(
    dark = true,
    bg = hex(0x000000), card = hex(0x141416), cardEdge = hex(0x0B0B0C),
    ink = hex(0xFFFFFF),
    accent = Starship, accentInk = hex(0xE7FE55),
    good = hex(0x3DE383), fair = hex(0xF5B840), bad = hex(0xFF5A66),
    hairline = hexA(0xFFFFFF, 0.08f),
    scrim = hexA(0x000000, 0.28f),
    grid = hexA(0xFFFFFF, 0.07f),
)

val AuraLightPalette = AuraPalette(
    dark = false,
    bg = hex(0xF5F4F1), card = hex(0xEDECE8), cardEdge = hex(0xF4F3F0),
    ink = hex(0x0C0C0D),
    accent = Starship, accentInk = hex(0x74731A),
    good = hex(0x1F9E57), fair = hex(0xC4841A), bad = hex(0xD83A44),
    hairline = hexA(0x0C0C0D, 0.10f),
    scrim = hexA(0xFFFFFF, 0.60f),
    grid = hexA(0x0C0C0D, 0.08f),
)

val LocalAuraPalette = staticCompositionLocalOf { AuraDarkPalette }

/** Token accessors + shape/spacing constants (AuraDesign.* equivalents). */
object Aura {
    val palette: AuraPalette
        @Composable @ReadOnlyComposable get() = LocalAuraPalette.current

    val screenMargin: Dp = 20.dp
    val cardSpacing: Dp = 12.dp
    val sectionGap: Dp = 24.dp
    val tilePadding: Dp = 20.dp

    val cardRadius: Dp = 32.dp
    val tileRadius: Dp = 28.dp
}

// MARK: Metric families (radial glow hues)

enum class AuraFamily(
    private val glowDark: Color, private val glowLight: Color,
    private val edgeDark: Color, private val edgeLight: Color,
) {
    /** recovery — jade green */
    CHARGE(hex(0x14C078), hex(0x6FD3A2), hex(0x05241A), hex(0xDDF0E6)),
    /** sleep — deep ocean blue */
    REST(hex(0x3E7BFF), hex(0x8FB2FB), hex(0x061634), hex(0xE2EAFC)),
    /** strain — floral magenta */
    EFFORT(hex(0xF52E9C), hex(0xFB7EC2), hex(0x2A0A1E), hex(0xFCE2F0)),
    /** cardio/HR — rose */
    HEART(hex(0xF5476A), hex(0xFB8194), hex(0x2A0912), hex(0xFCE4E8)),
    ENERGY(hex(0xE0A81E), hex(0xF3CE5A), hex(0x241C06), hex(0xFAF0D8)),
    VITALS(hex(0x12AEBE), hex(0x76D3DF), hex(0x042426), hex(0xDFF3F6));

    /** Luminous glow centre — saturated on dark so the tile emits light. */
    fun glow(dark: Boolean): Color = if (dark) glowDark else glowLight

    /** Deep, still-tinted edge the glow blooms out of. */
    fun glowEdge(dark: Boolean): Color = if (dark) edgeDark else edgeLight

    val glow: Color @Composable @ReadOnlyComposable get() = glow(LocalAuraPalette.current.dark)
    val glowEdge: Color @Composable @ReadOnlyComposable get() = glowEdge(LocalAuraPalette.current.dark)

    companion object {
        fun fromCategory(category: String): AuraFamily = when (category) {
            "Charge" -> CHARGE
            "Rest" -> REST
            "Effort" -> EFFORT
            "Heart" -> HEART
            "Nutrition", "Mind" -> ENERGY
            else -> VITALS
        }
    }
}

// MARK: Type — platform sans (Roboto) on the iOS ramp roles

object AuraType {
    /** Elegant thin hero numerals — the "73" / "62%" look. */
    fun mega(size: TextUnit) = TextStyle(fontWeight = FontWeight.Thin, fontSize = size)
    fun number(size: TextUnit) = TextStyle(fontWeight = FontWeight.ExtraLight, fontSize = size)
    /** Roman display headings ("Good evening", screen titles). */
    fun display(size: TextUnit) = TextStyle(fontWeight = FontWeight.Normal, fontSize = size)
    fun heading(size: TextUnit) = TextStyle(fontWeight = FontWeight.Medium, fontSize = size)
    val title = heading(19.sp)
    val label = TextStyle(fontWeight = FontWeight.Medium, fontSize = 15.sp)
    val sub = TextStyle(fontWeight = FontWeight.Normal, fontSize = 14.sp)
    val caption = TextStyle(fontWeight = FontWeight.Medium, fontSize = 12.sp)
}

// MARK: - Status semantics (the WHOOP colour language)
//
// ONE green/yellow/red mapping shared by every ring, chip and number. Family
// hues carry a metric's identity; AuraStatus carries its judgement. An element
// is owned by exactly one of the two systems, never both.

enum class AuraStatus {
    GOOD, FAIR, LOW, NONE;

    val color: Color
        @Composable @ReadOnlyComposable get() = when (this) {
            GOOD -> LocalAuraPalette.current.good
            FAIR -> LocalAuraPalette.current.fair
            LOW -> LocalAuraPalette.current.bad
            NONE -> LocalAuraPalette.current.ink.copy(alpha = 0.45f)
        }

    val word: String
        get() = when (this) {
            GOOD -> "Good"; FAIR -> "Fair"; LOW -> "Low"; NONE -> "No data"
        }

    companion object {
        /** Recovery / Charge %, WHOOP bands: 67+ green, 34–66 yellow, <34 red. */
        fun recovery(v: Double?): AuraStatus =
            when { v == null -> NONE; v >= 67 -> GOOD; v >= 34 -> FAIR; else -> LOW }

        /** Sleep performance %. */
        fun sleep(v: Double?): AuraStatus =
            when { v == null -> NONE; v >= 85 -> GOOD; v >= 70 -> FAIR; else -> LOW }

        /** Day strain: informational — high isn't "bad". */
        fun strain(v: Double?): AuraStatus = if (v == null) NONE else GOOD

        /** A vital vs. its baseline: |z|-style banding on a fractional deviation. */
        fun deviation(frac: Double?, tolerance: Double = 0.10): AuraStatus {
            val a = frac?.let { abs(it) } ?: return NONE
            return when {
                a <= tolerance -> GOOD
                a <= tolerance * 2 -> FAIR
                else -> LOW
            }
        }
    }
}

// MARK: - Effort display (stored 0–100; WHOOP 0–21 is display-only, #268)

object AuraEffort {
    /** Render a STORED 0–100 effort value on the given display factor. */
    fun text(stored: Double?, factor: Double): String {
        if (stored == null) return "--"
        val v = stored * factor
        return if (factor == 1.0) v.roundToInt().toString()
        else String.format(Locale.US, "%.1f", v)
    }

    /** Factor from the user's Effort-scale preference (mirrors `UnitPrefs.currentEffortDisplayFactor`). */
    @Composable
    fun displayFactor(): Double =
        effortDisplayFactor(UnitPrefs.effortScale(LocalContext.current))

    @Composable
    fun text(stored: Double?): String = text(stored, displayFactor())
}

// MARK: - Motion (the app-wide movement language)
//
// One easing, one spring family, one stagger — every screen animates the same way.

object AuraMotion {
    /** Calm shell easing, cubic-bezier(0.22, 1, 0.36, 1) — the iOS shell's ease. */
    val ease = androidx.compose.animation.core.CubicBezierEasing(0.22f, 1f, 0.36f, 1f)

    /** Standard opacity/travel tween on the calm ease. */
    fun <T> calm(durationMs: Int = 240): androidx.compose.animation.core.TweenSpec<T> =
        androidx.compose.animation.core.tween(durationMs, easing = ease)

    /** The default "alive" spring for reveals, rings and presses. */
    fun <T> soft(): androidx.compose.animation.core.SpringSpec<T> =
        androidx.compose.animation.core.spring(
            dampingRatio = 0.85f,
            stiffness = androidx.compose.animation.core.Spring.StiffnessMediumLow,
        )

    /** Per-card entrance stagger. */
    const val STAGGER_MS = 70
}

// MARK: - M3 Typography & Shapes (the Aura ramp on Material slots)

/** The AuraType roles mapped onto the full M3 scale, so STOCK components
 *  (buttons, chips, app bars, dialogs) speak the Aura voice with no per-call
 *  style plumbing. Thin numerals stay display-only; UI text is the label/sub ramp. */
private val AuraTypography = androidx.compose.material3.Typography(
    displayLarge = AuraType.mega(57.sp),
    displayMedium = AuraType.number(45.sp),
    displaySmall = AuraType.display(36.sp),
    headlineLarge = AuraType.display(32.sp),
    headlineMedium = AuraType.display(28.sp),
    headlineSmall = AuraType.heading(24.sp),
    titleLarge = AuraType.heading(22.sp),
    titleMedium = AuraType.title,
    titleSmall = AuraType.label,
    bodyLarge = TextStyle(fontWeight = FontWeight.Normal, fontSize = 16.sp),
    bodyMedium = AuraType.sub,
    bodySmall = TextStyle(fontWeight = FontWeight.Normal, fontSize = 12.sp),
    labelLarge = AuraType.label,
    labelMedium = AuraType.caption,
    labelSmall = TextStyle(fontWeight = FontWeight.Medium, fontSize = 11.sp),
)

/** Aura corner language on the M3 shape scale (tiles are 28, chrome rounds down from there). */
private val AuraShapes = androidx.compose.material3.Shapes(
    extraSmall = androidx.compose.foundation.shape.RoundedCornerShape(8.dp),
    small = androidx.compose.foundation.shape.RoundedCornerShape(12.dp),
    medium = androidx.compose.foundation.shape.RoundedCornerShape(16.dp),
    large = androidx.compose.foundation.shape.RoundedCornerShape(24.dp),
    extraLarge = androidx.compose.foundation.shape.RoundedCornerShape(28.dp),
)

// MARK: - Theme provider
//
// Division of labour (2026-07-09 Material pass): MATERIAL CARRIES THE PAINT —
// a ColorScheme built from the Aura tokens, so Scaffold / NavigationBar / Card /
// ListItem / Switch etc. render in Aura colours with no per-call-site colour
// plumbing. LocalAuraPalette remains ONLY for what Material has no slot for:
// the family glow/glowEdge pairs and the hand-drawn tiles/rings/charts.

/** The Material colour scheme derived from an [AuraPalette]. */
fun auraColorScheme(p: AuraPalette): androidx.compose.material3.ColorScheme {
    val base = if (p.dark) androidx.compose.material3.darkColorScheme()
    else androidx.compose.material3.lightColorScheme()
    return base.copy(
        primary = p.accent,
        onPrimary = Color.Black,
        surfaceTint = p.accent,
        primaryContainer = p.accent.copy(alpha = 0.22f),
        onPrimaryContainer = p.accentInk,
        secondary = p.accentInk,
        onSecondary = if (p.dark) Color.Black else Color.White,
        secondaryContainer = p.accent.copy(alpha = 0.22f),
        onSecondaryContainer = p.accentInk,
        background = p.bg,
        onBackground = p.ink,
        surface = p.bg,
        onSurface = p.ink,
        surfaceVariant = p.card,
        onSurfaceVariant = p.ink.copy(alpha = 0.7f),
        surfaceContainerLowest = p.bg,
        surfaceContainerLow = p.cardEdge,
        surfaceContainer = p.card,
        surfaceContainerHigh = p.card,
        surfaceContainerHighest = p.card,
        outline = p.hairline,
        outlineVariant = p.hairline,
        error = p.bad,
        onError = if (p.dark) Color.Black else Color.White,
    )
}

/** Provides the Aura palette + the Material scheme derived from it. */
@Composable
fun AuraTheme(content: @Composable () -> Unit) {
    val dark = when (AppearancePrefs.mode) {
        AppearanceMode.LIGHT -> false
        AppearanceMode.DARK -> true
        AppearanceMode.SYSTEM -> isSystemInDarkTheme()
    }
    val palette = if (dark) AuraDarkPalette else AuraLightPalette
    CompositionLocalProvider(LocalAuraPalette provides palette) {
        androidx.compose.material3.MaterialTheme(
            colorScheme = auraColorScheme(palette),
            typography = AuraTypography,
            shapes = AuraShapes,
            content = content,
        )
    }
}
