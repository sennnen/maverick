import SwiftUI
import XCTest

@testable import Mav

/// The Terrain theme's testable half, iOS side. The Android twin is `MavThemeTest.kt` and the two
/// assert the same properties, because a language that holds on one platform and not the other is
/// not a language.
///
/// Contrast is checked by `tools/check_a11y.py`, which reads `tokens/aura.json` directly. What is
/// left here is the wiring: that both schemes resolve, that the type roles pick the right face,
/// and that every role grows monotonically across the whole Dynamic Type range.
final class MavThemeTests: XCTestCase {

  private let dark = UITraitCollection(userInterfaceStyle: .dark)
  private let light = UITraitCollection(userInterfaceStyle: .light)

  private func rgba(_ color: Color, _ traits: UITraitCollection) -> (
    r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat
  ) {
    let resolved = UIColor(color).resolvedColor(with: traits)
    var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
    resolved.getRed(&r, green: &g, blue: &b, alpha: &a)
    return (r, g, b, a)
  }

  private func hex(_ color: Color, _ traits: UITraitCollection) -> UInt32 {
    let c = rgba(color, traits)
    return (UInt32(c.r * 255 + 0.5) << 16) | (UInt32(c.g * 255 + 0.5) << 8)
      | UInt32(c.b * 255 + 0.5)
  }

  /// WCAG 2.2 relative luminance and contrast, so a family pigment is checked by computation
  /// rather than by eye. Mirrors `tools/check_a11y.py`, which does the same for the token file.
  private func contrastRatio(
    _ a: Color, _ b: Color, _ traits: UITraitCollection
  ) -> CGFloat {
    func luminance(_ colour: (r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat)) -> CGFloat {
      func channel(_ v: CGFloat) -> CGFloat {
        v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4)
      }
      return 0.2126 * channel(colour.r) + 0.7152 * channel(colour.g) + 0.0722 * channel(colour.b)
    }
    let la = luminance(rgba(a, traits))
    let lb = luminance(rgba(b, traits))
    return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
  }

  /// Hue in degrees, so "one hue, several steps" can be asserted rather than eyeballed.
  private func hueDegrees(_ colour: Color, _ traits: UITraitCollection) -> CGFloat {
    let c = rgba(colour, traits)
    let high = max(c.r, c.g, c.b)
    let low = min(c.r, c.g, c.b)
    let delta = high - low
    guard delta > 1e-6 else { return 0 }
    let h: CGFloat
    if high == c.r {
      h = ((c.g - c.b) / delta).truncatingRemainder(dividingBy: 6)
    } else if high == c.g {
      h = (c.b - c.r) / delta + 2
    } else {
      h = (c.r - c.g) / delta + 4
    }
    return (h * 60 + 360).truncatingRemainder(dividingBy: 360)
  }

  // MARK: - Palette

  func testBothSchemesResolveAndDiffer() {
    for pair in [
      (MavTheme.canvas, "canvas"), (MavTheme.surface, "surface"), (MavTheme.raised, "raised"),
      (MavTheme.sunken, "sunken"), (MavTheme.ink, "ink"), (MavTheme.accent, "accent"),
      (MavTheme.focus, "focus"),
    ] {
      XCTAssertNotEqual(
        hex(pair.0, dark), hex(pair.0, light),
        "\(pair.1) is the same in both schemes, so one of them is unreadable")
    }
  }

  func testPaletteValuesComeFromGeneratedTokens() {
    XCTAssertEqual(hex(MavTheme.canvas, dark), AuraTokens.bg.dark)
    XCTAssertEqual(hex(MavTheme.canvas, light), AuraTokens.bg.light)
    XCTAssertEqual(hex(MavTheme.surface, dark), AuraTokens.card.dark)
    XCTAssertEqual(hex(MavTheme.raised, dark), AuraTokens.cardEdge.dark)
    XCTAssertEqual(hex(MavTheme.ink, dark), AuraTokens.ink.dark)
    XCTAssertEqual(hex(MavTheme.ink, light), AuraTokens.ink.light)
    XCTAssertEqual(hex(MavTheme.accent, dark), AuraTokens.accentInk.dark)
    XCTAssertEqual(hex(MavTheme.focus, dark), AuraTokens.focus.dark)
  }

  func testFocusRingIsNotTheAccent() {
    // A focus ring drawn in the interaction hue disappears on the only kind of thing it is ever
    // drawn around.
    for traits in [dark, light] {
      XCTAssertNotEqual(hex(MavTheme.focus, traits), hex(MavTheme.accent, traits))
    }
  }

  func testInkHasExactlyTwoWeightsAndTheSecondIsTranslucent() {
    for traits in [dark, light] {
      XCTAssertEqual(rgba(MavTheme.ink, traits).a, 1, accuracy: 0.001)
      let secondary = rgba(MavTheme.inkSecondary, traits).a
      XCTAssertLessThan(secondary, 1)
      // The lowest alpha that still clears 4.5:1 on every surface; see tools/check_a11y.py.
      XCTAssertGreaterThanOrEqual(secondary, 0.65)
    }
  }

  // MARK: - Status and family

  func testEveryMetricResolvesADistinctSurfaceWash() {
    // A wash names which metric, not how it is doing. The seven must be told apart, and every one
    // has to stay a wash — an opaque fill would swallow the ink on top of it.
    for traits in [dark, light] {
      let washes = MavFamily.allCases.map { rgba(MavTheme.tint($0), traits) }
      for (i, a) in washes.enumerated() {
        for b in washes[(i + 1)...] {
          XCTAssertFalse(
            abs(a.r - b.r) < 0.002 && abs(a.g - b.g) < 0.002 && abs(a.b - b.b) < 0.002
              && abs(a.a - b.a) < 0.002,
            "two metrics share a surface wash, so a row is not recognisable by sight")
        }
      }
      for wash in washes { XCTAssertLessThan(wash.a, 0.3) }
      XCTAssertLessThan(rgba(MavTheme.neutralTint, traits).a, 0.3)
    }
  }

  func testTheWashesDescendInADeliberateOrder() {
    // Charge is the headline metric and sits lightest; cycle sits darkest. The ordering is what
    // makes seven steps of one hue tellable apart, so it is asserted rather than left to whoever
    // edits the token file next.
    for traits in [dark, light] {
      let alphas = MavFamily.allCases.map { rgba(MavTheme.tint($0), traits).a }
      XCTAssertEqual(
        alphas, alphas.sorted(by: >), "the metric washes are no longer in descending order")
    }
  }

  func testEveryFamilyIsAStepOfTheOneHueAndStaysLegible() {
    // Monochromatic does not mean identical: the seven metrics are seven *steps* of a single hue,
    // so a row is recognisable by sight without any of them becoming a second colour. This asserts
    // both halves — every step sits within a few degrees of the same hue, and every step is still a
    // mark you can see. 3:1 is the WCAG non-text ratio, and a family glow is always a data mark.
    for traits in [dark, light] {
      let hues = MavFamily.allCases.map { hueDegrees($0.hue, traits) }
      let spread = (hues.max() ?? 0) - (hues.min() ?? 0)
      XCTAssertLessThanOrEqual(
        spread, 12,
        "family hues span \(String(format: "%.1f", spread)) degrees, so one is a second colour")

      // Steps, not duplicates: all seven must actually differ.
      XCTAssertEqual(
        Set(MavFamily.allCases.map { hex($0.hue, traits) }).count, 7,
        "two families resolve to the same step")

      for family in MavFamily.allCases {
        let value = contrastRatio(family.hue, MavTheme.surface, traits)
        XCTAssertGreaterThanOrEqual(
          value, 3.0,
          "\(family) glow is \(String(format: "%.2f", value)):1 on the card, needs 3:1")
      }
    }
  }

  func testTheFamilyStepsRunInADeliberateOrder() {
    let ratios = MavFamily.allCases.map { contrastRatio($0.hue, MavTheme.surface, dark) }
    XCTAssertEqual(ratios, ratios.sorted(by: >), "the family steps are no longer ordered")
  }

  func testTheAccentIsTheSameHueAsEveryDataMark() {
    // Monochromatic means exactly this: the one affirmative action and every data mark belong to
    // one hue. If the accent drifts out of that band the app has two colours.
    for traits in [dark, light] {
      let accentHue = hueDegrees(MavTheme.accent, traits)
      for family in MavFamily.allCases {
        let delta = abs(accentHue - hueDegrees(family.hue, traits))
        XCTAssertLessThanOrEqual(
          delta, 12, "the accent is \(String(format: "%.1f", delta)) degrees off \(family)")
      }
    }
  }

  func testDestructiveInkIsTheOneDeliberateException() {
    // Delete and integrity failures stay red. It is a safety affordance, not decoration, and when
    // it matched body text the delete label was indistinguishable from a caption.
    for traits in [dark, light] {
      XCTAssertNotEqual(
        hex(MavTheme.destructiveInk(), traits), hex(MavTheme.ink, traits),
        "a destructive label renders as ordinary body text")
      XCTAssertNotEqual(
        hex(MavTheme.destructiveInk(), traits), hex(MavTheme.accent, traits),
        "destructive and affirmative actions look the same")
    }
  }

  func testCycleIsAFamilyWithAWashOfItsOwn() {
    XCTAssertTrue(MavFamily.allCases.contains(.cycle))
    for traits in [dark, light] {
      let cycle = rgba(MavTheme.tint(.cycle), traits)
      for family in MavFamily.allCases where family != .cycle {
        XCTAssertNotEqual(cycle.a, rgba(MavTheme.tint(family), traits).a, accuracy: 0.001)
      }
    }
  }

  func testUnrecognisedCategoryFallsBackToVitals() {
    XCTAssertEqual(MavFamily(category: "Recovery"), .charge)
    XCTAssertEqual(MavFamily(category: "Sleep"), .rest)
    XCTAssertEqual(MavFamily(category: "Strain"), .effort)
    XCTAssertEqual(MavFamily(category: "Cycle"), .cycle)
    XCTAssertEqual(MavFamily(category: "something the core added last week"), .vitals)
  }

  // MARK: - Type

  private let allRoles: [MavType] = [
    .displayLarge, .display, .numeralXL, .numeralLarge, .numeralMedium, .numeralSmall, .title,
    .label, .body, .sub, .caption, .eyebrow,
  ]

  func testEveryRoleGrowsMonotonicallyAcrossTheDynamicTypeRange() {
    let sizes: [DynamicTypeSize] = [
      .xSmall, .small, .medium, .large, .xLarge, .xxLarge, .xxxLarge,
      .accessibility1, .accessibility2, .accessibility3, .accessibility4, .accessibility5,
    ]
    for role in allRoles {
      let points = sizes.map { role.pointSize(for: $0) }
      for (i, value) in points.enumerated() where i > 0 {
        XCTAssertGreaterThanOrEqual(
          value, points[i - 1],
          "\(role) shrinks between \(sizes[i - 1]) and \(sizes[i])")
      }
      // The largest accessibility size must actually be larger, or the role is not scaling at all.
      XCTAssertGreaterThan(
        points.last ?? 0, points.first ?? 0,
        "\(role) does not respond to Dynamic Type")
    }
  }

  func testTheNumeralRampDescendsAtEverySize() {
    for size in [DynamicTypeSize.large, .accessibility5] {
      let ramp = [MavType.numeralXL, .numeralLarge, .numeralMedium, .numeralSmall]
        .map { $0.pointSize(for: size) }
      XCTAssertEqual(ramp, ramp.sorted(by: >), "the numeral ramp is out of order at \(size)")
    }
  }

  func testChromeRolesStaySmallerThanContentRoles() {
    for size in [DynamicTypeSize.large, .accessibility5] {
      XCTAssertLessThan(MavType.eyebrow.pointSize(for: size), MavType.body.pointSize(for: size))
      XCTAssertLessThan(MavType.body.pointSize(for: size), MavType.title.pointSize(for: size))
      XCTAssertLessThan(MavType.title.pointSize(for: size), MavType.display.pointSize(for: size))
    }
  }

  func testTrackedRolesAreTheOnlyTrackedRoles() {
    // Metadata gets only a breath of tracking. Wide, uppercase tech labels are deliberately absent
    // from Terrain's quieter editorial language.
    XCTAssertGreaterThan(MavType.eyebrow.tracking, 0)
    XCTAssertLessThanOrEqual(MavType.eyebrow.tracking, 0.25)
    XCTAssertEqual(MavType.body.tracking, 0)
    XCTAssertEqual(MavType.label.tracking, 0)
    // Display and numeral roles are tightened, not tracked.
    XCTAssertLessThan(MavType.numeralXL.tracking, 0)
    XCTAssertLessThan(MavType.display.tracking, 0)
  }

  func testEveryDynamicTypeSizeMapsToAContentSizeCategory() {
    // A size that fell through to `.large` would make Dynamic Type silently stop working for
    // whoever set it, and nothing on screen would say so.
    let mapped: [(DynamicTypeSize, UIContentSizeCategory)] = [
      (.xSmall, .extraSmall), (.small, .small), (.medium, .medium), (.large, .large),
      (.xLarge, .extraLarge), (.xxLarge, .extraExtraLarge), (.xxxLarge, .extraExtraExtraLarge),
      (.accessibility1, .accessibilityMedium), (.accessibility2, .accessibilityLarge),
      (.accessibility3, .accessibilityExtraLarge),
      (.accessibility4, .accessibilityExtraExtraLarge),
      (.accessibility5, .accessibilityExtraExtraExtraLarge),
    ]
    for (size, category) in mapped {
      XCTAssertEqual(size.contentSizeCategory, category)
    }
  }

  func testStrengthLibraryIncludesRoutinesAndEverySetType() {
    XCTAssertGreaterThanOrEqual(MavStrengthLibrary.starterRoutines.count, 3)
    XCTAssertGreaterThanOrEqual(MavStrengthLibrary.categories.count, 6)
    XCTAssertEqual(
      Set(MavStrengthSetKind.allCases),
      Set([.warmup, .working, .drop, .failure]))

    let session = MavStrengthLibrary.starterRoutines[0].exercises
    XCTAssertFalse(session.isEmpty)
    XCTAssertTrue(session.allSatisfy { !$0.sets.isEmpty })
  }
}
