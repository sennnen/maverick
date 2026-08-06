package com.sennnen.mav.ui.mav

import androidx.compose.ui.text.font.FontFamily
import com.sennnen.mav.ui.aura.AuraTokens
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The Terrain theme's testable half.
 *
 * Contrast is checked by `tools/check_a11y.py`, which reads `tokens/aura.json` directly so a scheme
 * that exists on one platform only cannot hide. What is left for this file is the wiring: that both
 * palettes are resolved from tokens rather than from literals, that each family carries a pigment
 * that is both distinct and legible, that the serif role reaches the platform's own face rather
 * than a bundled one, and that the type roles keep the order the language depends on.
 */
class MavThemeTest {

    // -------------------------------------------------------------------------------------------
    // Palette
    // -------------------------------------------------------------------------------------------

    @Test
    fun `both schemes resolve and differ`() {
        val dark = mavPalette(dark = true)
        val light = mavPalette(dark = false)

        assertTrue(dark.dark)
        assertTrue(!light.dark)

        // Every surface and ink differs between schemes. A token copied across by accident would
        // make one of these equal, and the screen would then be unreadable in one mode.
        assertNotEquals(dark.canvas, light.canvas)
        assertNotEquals(dark.surface, light.surface)
        assertNotEquals(dark.raised, light.raised)
        assertNotEquals(dark.sunken, light.sunken)
        assertNotEquals(dark.ink, light.ink)
        assertNotEquals(dark.accent, light.accent)
        assertNotEquals(dark.focus, light.focus)
    }

    @Test
    fun `palette values come from the generated tokens`() {
        val dark = mavPalette(dark = true)
        assertEquals(AuraTokens.bgDark, rgbOf(dark.canvas))
        assertEquals(AuraTokens.cardDark, rgbOf(dark.surface))
        assertEquals(AuraTokens.cardEdgeDark, rgbOf(dark.raised))
        assertEquals(AuraTokens.inkDark, rgbOf(dark.ink))
        assertEquals(AuraTokens.accentInkDark, rgbOf(dark.accent))
        assertEquals(AuraTokens.focusDark, rgbOf(dark.focus))

        val light = mavPalette(dark = false)
        assertEquals(AuraTokens.bgLight, rgbOf(light.canvas))
        assertEquals(AuraTokens.cardLight, rgbOf(light.surface))
        assertEquals(AuraTokens.inkLight, rgbOf(light.ink))
    }

    @Test
    fun `the focus ring is not the accent`() {
        // A focus ring drawn in the interaction hue disappears the moment it lands on an
        // interactive control, which is the only place it is ever drawn.
        for (dark in listOf(true, false)) {
            val palette = mavPalette(dark)
            assertNotEquals(palette.accent, palette.focus)
        }
    }

    @Test
    fun `secondary ink is translucent and primary ink is not`() {
        for (dark in listOf(true, false)) {
            val palette = mavPalette(dark)
            assertEquals(1f, palette.ink.alpha, 0.0001f)
            assertTrue(palette.inkSecondary.alpha < 1f)
            // The lowest alpha that still clears 4.5:1 on every surface; see check_a11y.py. The
            // delta is one 8-bit step, because Compose quantises alpha on the way into a Color.
            val expected =
                if (dark) AuraTokens.inkSecondaryDarkAlpha else AuraTokens.inkSecondaryLightAlpha
            assertEquals(expected, palette.inkSecondary.alpha, 1f / 255f)
        }
    }

    // -------------------------------------------------------------------------------------------
    // Status and family
    // -------------------------------------------------------------------------------------------

    @Test
    fun `every metric resolves a distinct surface wash`() {
        // A wash names which metric, not how it is doing. The seven must be told apart, and every
        // one has to stay a wash — an opaque fill would swallow the ink on top of it.
        for (dark in listOf(true, false)) {
            val palette = mavPalette(dark)
            val washes = MavFamily.entries.map { it.tint(palette) }
            assertEquals(
                "two metrics share a surface wash, so a row is not recognisable by sight",
                MavFamily.entries.size,
                washes.toSet().size,
            )
            for (wash in washes) assertTrue(wash.alpha < 0.3f)
            assertTrue(palette.tintNeutral.alpha < 0.3f)
        }
    }

    @Test
    fun `the washes descend in a deliberate order`() {
        // Charge is the headline metric and sits lightest; cycle sits darkest. The ordering is the
        // thing that makes seven steps of one hue tellable apart, so it is asserted rather than
        // left to whoever edits the token file next.
        for (dark in listOf(true, false)) {
            val palette = mavPalette(dark)
            val alphas = MavFamily.entries.map { it.tint(palette).alpha }
            assertEquals("the metric washes are no longer in descending order", alphas.sortedDescending(), alphas)
        }
    }

    @Test
    fun `every family is a step of the one hue and stays legible on the card`() {
        // Monochromatic does not mean identical: the seven metrics are seven *steps* of a single
        // hue, so a row is recognisable by sight without any of them becoming a second colour.
        // This asserts both halves — every step sits within a few degrees of the same hue, and
        // every step is still a mark you can see. 3:1 is the WCAG non-text ratio.
        for (dark in listOf(true, false)) {
            val hues = MavFamily.entries.map { hueDegrees(it.hue(dark)) }
            val spread = hues.max() - hues.min()
            assertTrue(
                "family hues span ${"%.1f".format(spread)} degrees, so one is a second colour",
                spread <= 12f,
            )

            // Steps, not duplicates: all seven must actually differ.
            assertEquals(
                "two families resolve to the same step",
                7,
                MavFamily.entries.map { it.hue(dark) }.toSet().size,
            )

            val card = mavPalette(dark).surface
            for (family in MavFamily.entries) {
                val ratio = contrastRatio(family.hue(dark), card)
                assertTrue(
                    "$family glow is ${"%.2f".format(ratio)}:1 on the card, needs 3:1",
                    ratio >= 3f,
                )
            }
        }
    }

    @Test
    fun `the family steps run in a deliberate order`() {
        // Charge is the headline metric and sits lightest against a dark card; cycle sits darkest.
        // The ordering is what makes seven steps of one hue tellable apart at a glance.
        val card = mavPalette(true).surface
        val ratios = MavFamily.entries.map { contrastRatio(it.hue(true), card) }
        assertEquals("the family steps are no longer ordered", ratios.sortedDescending(), ratios)
    }

    @Test
    fun `the accent is the same hue as every data mark`() {
        // Monochromatic means exactly this: the one affirmative action and every data mark belong
        // to one hue. If the accent drifts out of that band the app has two colours.
        for (dark in listOf(true, false)) {
            val accentHue = hueDegrees(mavPalette(dark).accent)
            for (family in MavFamily.entries) {
                val delta = kotlin.math.abs(accentHue - hueDegrees(family.hue(dark)))
                assertTrue(
                    "the accent is ${"%.1f".format(delta)} degrees off $family",
                    delta <= 12f,
                )
            }
        }
    }

    @Test
    fun `destructive ink is the one deliberate exception`() {
        // Delete and integrity failures stay red. It is a safety affordance, not decoration, and
        // when it matched body text the delete label was indistinguishable from a caption.
        for (dark in listOf(true, false)) {
            val destructive = if (dark) AuraTokens.badDark else AuraTokens.badLight
            val palette = mavPalette(dark)
            assertNotEquals(
                "a destructive label renders as ordinary body text",
                destructive,
                rgbOf(palette.ink),
            )
            assertNotEquals(
                "destructive and affirmative actions look the same",
                destructive,
                rgbOf(palette.accent),
            )
        }
    }

    @Test
    fun `cycle is a family with a wash of its own`() {
        assertTrue(MavFamily.entries.contains(MavFamily.CYCLE))
        for (dark in listOf(true, false)) {
            val palette = mavPalette(dark)
            for (family in MavFamily.entries - MavFamily.CYCLE) {
                assertNotEquals(palette.tintCycle, family.tint(palette))
            }
        }
    }

    @Test
    fun `unrecognised categories fall back to vitals rather than inventing a family`() {
        assertEquals(MavFamily.CHARGE, MavFamily.of("Recovery"))
        assertEquals(MavFamily.REST, MavFamily.of("Sleep"))
        assertEquals(MavFamily.EFFORT, MavFamily.of("Strain"))
        assertEquals(MavFamily.CYCLE, MavFamily.of("Cycle"))
        assertEquals(MavFamily.VITALS, MavFamily.of("something the core added last week"))
    }

    // -------------------------------------------------------------------------------------------
    // Type
    // -------------------------------------------------------------------------------------------

    @Test
    fun `serif is reserved for editorial display and product UI stays native sans`() {
        // The platform serif carries the two photographic story headlines. Scores, metrics,
        // settings and navigation intentionally use Roboto.
        assertNotEquals(FontFamily.Default, MavSerif)
        assertEquals(FontFamily.Default, MavSans)

        for (style in listOf(
            MavType.displayLarge,
            MavType.display,
        )) {
            assertEquals("an editorial display role is not set in the serif", MavSerif, style.fontFamily)
        }

        for (style in listOf(
            MavType.numeralXL,
            MavType.numeralLarge,
            MavType.numeralMedium,
            MavType.numeralSmall,
            MavType.title,
            MavType.label,
            MavType.body,
            MavType.sub,
            MavType.caption,
            MavType.eyebrow,
        )) {
            assertEquals("product UI is not set in the platform sans", MavSans, style.fontFamily)
        }
    }

    @Test
    fun `the serif is the platform's own and nothing is bundled`() {
        // Old Standard TT used to ship inside the APK. It was a Didone revival, which reads thin
        // and academic at display sizes, and it made the one role carrying the brand look like a
        // different product from iOS. Apple's faces cannot legally ship here, so each platform now
        // uses its own system serif. A bundled face reappearing would make this a
        // FontListFontFamily rather than the platform's generic one.
        assertEquals(
            "the serif is bundled again rather than the platform's",
            FontFamily.Serif,
            MavSerif,
        )
    }

    @Test
    fun `the numeral ramp descends and every size is positive`() {
        val ramp = listOf(
            MavType.numeralXL,
            MavType.numeralLarge,
            MavType.numeralMedium,
            MavType.numeralSmall,
        ).map { it.fontSize.value }

        assertEquals(ramp.sortedDescending(), ramp)
        for (size in ramp) assertTrue(size > 0f)
    }

    @Test
    fun `chrome roles are smaller than content roles`() {
        assertTrue(MavType.eyebrow.fontSize.value < MavType.body.fontSize.value)
        assertTrue(MavType.caption.fontSize.value < MavType.label.fontSize.value)
        assertTrue(MavType.body.fontSize.value < MavType.title.fontSize.value)
        assertTrue(MavType.title.fontSize.value < MavType.display.fontSize.value)
    }

    @Test
    fun `metadata stays natural while display and numerals tighten`() {
        // Sentence-case metadata should read naturally. The old tracked, uppercase treatment made
        // Android feel like a custom dashboard instead of a calm Material product.
        assertEquals(0f, MavType.eyebrow.letterSpacing.value, 0.0001f)
        assertEquals(0f, MavType.body.letterSpacing.value, 0.0001f)
        assertEquals(0f, MavType.label.letterSpacing.value, 0.0001f)
        // Display and numeral roles retain only the optical tightening they need.
        assertTrue(MavType.numeralXL.letterSpacing.value < 0f)
        assertTrue(MavType.display.letterSpacing.value < 0f)
    }

    @Test
    fun `every role sets a line height above its size`() {
        val all = listOf(
            MavType.displayLarge, MavType.display, MavType.numeralXL, MavType.numeralLarge,
            MavType.numeralMedium, MavType.numeralSmall, MavType.title, MavType.label,
            MavType.body, MavType.sub, MavType.caption, MavType.eyebrow,
        )
        for (style in all) {
            assertTrue(
                "a role has no line height, so a wrapped string will collide with itself",
                style.lineHeight.value >= style.fontSize.value,
            )
        }
    }

    @Test
    fun `strength library includes routines and every set type`() {
        assertTrue(MavStrengthLibrary.starterRoutines.size >= 3)
        assertTrue(MavStrengthLibrary.categories.size >= 6)
        assertEquals(
            setOf(
                MavStrengthSetKind.WARMUP,
                MavStrengthSetKind.WORKING,
                MavStrengthSetKind.DROP,
                MavStrengthSetKind.FAILURE,
            ),
            MavStrengthSetKind.entries.toSet(),
        )
        assertTrue(MavStrengthLibrary.starterRoutines.first().exercises.isNotEmpty())
        assertTrue(
            MavStrengthLibrary.starterRoutines.first().exercises.all { it.sets.isNotEmpty() },
        )
    }

    /**
     * WCAG 2.2 relative luminance and contrast, so a family pigment is checked by computation
     * rather than by eye. Mirrors `tools/check_a11y.py`, which does the same for the token file.
     */
    private fun contrastRatio(
        a: androidx.compose.ui.graphics.Color,
        b: androidx.compose.ui.graphics.Color,
    ): Float {
        fun channel(v: Float) =
            if (v <= 0.04045f) v / 12.92f else Math.pow(((v + 0.055f) / 1.055f).toDouble(), 2.4)
                .toFloat()

        fun luminance(c: androidx.compose.ui.graphics.Color) =
            0.2126f * channel(c.red) + 0.7152f * channel(c.green) + 0.0722f * channel(c.blue)

        val la = luminance(a)
        val lb = luminance(b)
        return (maxOf(la, lb) + 0.05f) / (minOf(la, lb) + 0.05f)
    }

    /** Hue in degrees, so "one hue, several steps" can be asserted rather than eyeballed. */
    private fun hueDegrees(colour: androidx.compose.ui.graphics.Color): Float {
        val r = colour.red
        val g = colour.green
        val b = colour.blue
        val high = maxOf(r, g, b)
        val low = minOf(r, g, b)
        val delta = high - low
        if (delta < 1e-6f) return 0f
        val h = when (high) {
            r -> ((g - b) / delta) % 6f
            g -> (b - r) / delta + 2f
            else -> (r - g) / delta + 4f
        }
        return ((h * 60f) + 360f) % 360f
    }

    /** The RGB triple behind a Compose colour, so a test can compare against a raw token. */
    private fun rgbOf(colour: androidx.compose.ui.graphics.Color): Long {
        val r = (colour.red * 255f + 0.5f).toLong()
        val g = (colour.green * 255f + 0.5f).toLong()
        val b = (colour.blue * 255f + 0.5f).toLong()
        return (r shl 16) or (g shl 8) or b
    }
}
