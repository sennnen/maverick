import SwiftUI

// The Terrain design language, iOS side.
//
// This file owns exactly what a token cannot: the two font families, the type roles and how they
// scale with Dynamic Type, and the status-to-tint lookup. Every value it hands out comes from
// `AuraTokens.generated.swift`, which comes from `tokens/aura.json`. There is no colour literal
// below and there may never be one — `tools/check_a11y.py` reasons about the token file, so a
// colour written here would be a colour nothing checks.
//
// The rule the whole language rests on: **it is monochromatic**. There is one hue — a deep stone
// teal — and everything on screen is a weight of it. Surfaces are near-black with the same cool
// cast, ink is a cool bone, and every data mark is that teal at a different luminance. Hierarchy
// comes from weight, size, material and spacing, never from a second colour.
//
// Family stays semantic data for choosing an icon and its copy; it never selects a colour. A pass
// that gave each of the seven families its own pigment turned a calm screen into a chart of
// unrelated colours, and `MavThemeTests` now asserts the families resolve to one value so that
// cannot come back by accident.
//
// There is exactly **one** exception, and it is a safety affordance rather than decoration:
// `destructiveInk()` is red. When it matched body text, "Delete device" was indistinguishable from
// a caption. Any second exception is a bug.

// MARK: - Metric families

/// A metric's identity. Seven of them, and `cycle` is the newest.
enum MavFamily: String, CaseIterable, Sendable {
  case charge, rest, effort, heart, energy, vitals, cycle

  /// The core hands back a category string; anything unrecognised reads as a general vital
  /// rather than inventing a family for it.
  init(category: String) {
    switch category {
    case "Charge", "Recovery": self = .charge
    case "Rest", "Sleep": self = .rest
    case "Effort", "Strain": self = .effort
    case "Heart", "Cardio": self = .heart
    case "Nutrition", "Mind", "Energy": self = .energy
    case "Cycle": self = .cycle
    default: self = .vitals
    }
  }

  fileprivate var glowToken: (dark: UInt32, light: UInt32) {
    switch self {
    case .charge: AuraTokens.chargeGlow
    case .rest: AuraTokens.restGlow
    case .effort: AuraTokens.effortGlow
    case .heart: AuraTokens.heartGlow
    case .energy: AuraTokens.energyGlow
    case .vitals: AuraTokens.vitalsGlow
    case .cycle: AuraTokens.cycleGlow
    }
  }

  fileprivate var tintToken: (dark: (UInt32, CGFloat), light: (UInt32, CGFloat)) {
    switch self {
    case .charge: AuraTokens.tintCharge
    case .rest: AuraTokens.tintRest
    case .effort: AuraTokens.tintEffort
    case .heart: AuraTokens.tintHeart
    case .energy: AuraTokens.tintEnergy
    case .vitals: AuraTokens.tintVitals
    case .cycle: AuraTokens.tintCycle
    }
  }

  fileprivate var edgeToken: (dark: UInt32, light: UInt32) {
    switch self {
    case .charge: AuraTokens.chargeEdge
    case .rest: AuraTokens.restEdge
    case .effort: AuraTokens.effortEdge
    case .heart: AuraTokens.heartEdge
    case .energy: AuraTokens.energyEdge
    case .vitals: AuraTokens.vitalsEdge
    case .cycle: AuraTokens.cycleEdge
    }
  }

  /// The family's own pigment, for the data mark and nothing else. Each one clears 3:1 against
  /// the card in both schemes, which `tools/check_a11y.py` asserts rather than trusts.
  var hue: Color { MavTheme.dynamic(glowToken) }

  /// The deep wash the hue blooms out of. A backdrop, never a text surface.
  var wash: Color { MavTheme.dynamic(edgeToken) }
}

// MARK: - Status

/// A judgement, at the only granularity the surface tint can express.
///
/// The *word* shown beside a value is not derived from this — it comes from the core's band, so
/// "In range", "Elevated", "Building" and "Provisional" all reach the screen as text the core
/// supplied. This enum decides one thing: which wash the card's surface carries.
enum MavStatus: String, CaseIterable, Sendable {
  case optimal, fair, low, neutral

  /// The last-resort word, used only where the core supplied no band. A metric that has a band
  /// shows the core's wording instead, because a status word is a claim about the value.
  var fallbackWord: String {
    switch self {
    case .optimal: "Optimal"
    case .fair: "Fair"
    case .low: "Pay attention"
    case .neutral: "No data"
    }
  }
}

// MARK: - Palette

enum MavTheme {

  // Surfaces, canvas outward.
  static var canvas: Color { dynamic(AuraTokens.bg) }
  static var surface: Color { dynamic(AuraTokens.card) }
  static var raised: Color { dynamic(AuraTokens.cardEdge) }
  static var sunken: Color { dynamic(AuraTokens.sunken) }

  // Ink. Two weights, and that is a contrast finding rather than a preference: a third, fainter
  // weight cannot clear 4.5:1 on these surfaces, and every string here carries information.
  // Hierarchy comes from size, case and tracking.
  static var ink: Color { dynamic(AuraTokens.ink) }
  static var inkSecondary: Color { dynamicAlpha(AuraTokens.inkSecondary) }

  /// The single interaction hue — a pale lichen on dark, a deep moss on light.
  ///
  /// The rule: **at most one accent element per screen**, and it is always the one affirmative
  /// action — start a workout, approve a connector, log a period. Selection is *not* an
  /// affirmative action, so a selected tab, a selected range, and a selected day are ink. An early
  /// pass tinted the whole app with it and every screen ended up shouting in the same voice.
  static var accent: Color { dynamic(AuraTokens.accentInk) }
  /// Ink for content sitting *on* the accent.
  static var onAccent: Color { dynamic(AuraTokens.bg) }
  /// The focus ring, deliberately not the accent so a focused accent control stays visible.
  static var focus: Color { dynamic(AuraTokens.focus) }

  // Lines and washes.
  static var hairline: Color { dynamicAlpha(AuraTokens.hairline) }
  static var hairlineStrong: Color { dynamicAlpha(AuraTokens.hairlineStrong) }
  static var glass: Color { dynamicAlpha(AuraTokens.glass) }
  static var glassLine: Color { dynamicAlpha(AuraTokens.glassLine) }
  static var grid: Color { dynamicAlpha(AuraTokens.grid) }
  /// Dim behind a presented sheet.
  static var scrim: Color { dynamicAlpha(AuraTokens.scrim) }
  /// The wash over a photograph carrying white copy, which makes contrast a constant rather than
  /// a hope. Used by `MavScene` for the `.story` treatment.
  static var photoScrim: Color { dynamicAlpha(AuraTokens.photoScrim) }
  /// The heavier veil that knocks a photograph back far enough for *ordinary ink* to sit on it,
  /// so a landscape can back a metric row without the row becoming a poster. Checked against a
  /// worst-case photograph in `tools/check_a11y.py`, so no landscape can defeat it.
  static var photoVeil: Color { dynamicAlpha(AuraTokens.photoVeil) }

  // The atmosphere behind a tab root. See `MavAtmosphere`.
  static var bloomTop: Color { dynamicAlpha(AuraTokens.bloomTop) }
  static var bloomBottom: Color { dynamicAlpha(AuraTokens.bloomBottom) }

  /// The wash a metric's card carries. It names **which metric**, not how it is doing.
  ///
  /// Colouring a surface by verdict was the wrong idea twice over: it made a bad night look
  /// alarming before the number was read, and it meant the same card changed colour day to day, so
  /// nothing on the screen was recognisable by sight. Identity is constant, so identity gets the
  /// colour. The verdict is the baseline bar and the word beside it, which is where a claim about
  /// a value belongs.
  static func tint(_ family: MavFamily) -> Color { dynamicAlpha(family.tintToken) }

  /// For a card that is not a metric at all — a connector, a device, a prompt.
  static var neutralTint: Color { dynamicAlpha(AuraTokens.tintNeutral) }

  /// The only two places a status hue may touch ink rather than a surface. Both are cases where
  /// there is no surface underneath to tint and the meaning *is* the colour:
  ///
  ///  - a destructive action's label, where the danger is the point;
  ///  - the live-link dot on the strap glyph, which sits in the toolbar and marks a connection
  ///    rather than a health verdict.
  ///
  /// Any third use is a bug. Status belongs to the background.
  static func destructiveInk() -> Color { dynamic(AuraTokens.bad) }
  static func liveInk() -> Color { dynamic(AuraTokens.good) }

  // MARK: Shape

  static let screenMargin = AuraTokens.screenMargin
  static let cardSpacing = AuraTokens.cardSpacing
  static let sectionGap = AuraTokens.sectionGap
  static let tilePadding = AuraTokens.tilePadding
  static let railGap = AuraTokens.railGap

  static var cardShape: RoundedRectangle {
    RoundedRectangle(cornerRadius: AuraTokens.cardRadius, style: .continuous)
  }
  static var tileShape: RoundedRectangle {
    RoundedRectangle(cornerRadius: AuraTokens.tileRadius, style: .continuous)
  }
  static var pillShape: Capsule { Capsule(style: .continuous) }
  static var chipShape: RoundedRectangle {
    RoundedRectangle(cornerRadius: AuraTokens.chipRadius, style: .continuous)
  }

  // MARK: Motion

  /// The one easing curve. 240 ms, and it stays calm.
  static let calm = Animation.timingCurve(0.22, 1, 0.36, 1, duration: 0.24)

  // MARK: Token resolution

  fileprivate static func dynamic(_ token: (dark: UInt32, light: UInt32)) -> Color {
    Color(UIColor { $0.userInterfaceStyle == .dark ? rgb(token.dark) : rgb(token.light) })
  }

  fileprivate static func dynamicAlpha(
    _ token: (dark: (UInt32, CGFloat), light: (UInt32, CGFloat))
  ) -> Color {
    Color(UIColor {
      $0.userInterfaceStyle == .dark
        ? rgb(token.dark.0).withAlphaComponent(token.dark.1)
        : rgb(token.light.0).withAlphaComponent(token.light.1)
    })
  }

  private static func rgb(_ hex: UInt32) -> UIColor {
    UIColor(
      red: CGFloat((hex >> 16) & 0xFF) / 255,
      green: CGFloat((hex >> 8) & 0xFF) / 255,
      blue: CGFloat(hex & 0xFF) / 255,
      alpha: 1
    )
  }
}

// MARK: - Type

/// The two faces, and nothing else may be used.
///
/// Serif is New York, sans is SF Pro. Both are Apple system faces reached through the font
/// *design* rather than by name, so nothing is bundled and nothing can fail to load.
enum MavFace: Sendable {
  case serif
  case sans

  fileprivate var design: Font.Design {
    switch self {
    case .serif: .serif
    case .sans: .default
    }
  }
}

/// New York is an editorial accent, not the app's default voice. Oura and WHOOP keep dense health
/// data in a clean sans; Maverick does the same. Only the two display roles use serif.
enum MavType: Sendable {
  case displayLarge
  case display
  case numeralXL
  case numeralLarge
  case numeralMedium
  case numeralSmall
  case title
  case label
  case body
  case sub
  case caption
  /// Compact metadata. Callers keep it sentence case.
  case eyebrow

  fileprivate var face: MavFace {
    switch self {
    case .displayLarge, .display: .serif
    case .numeralXL, .numeralLarge, .numeralMedium, .numeralSmall, .title, .label, .body,
      .sub, .caption, .eyebrow: .sans
    }
  }

  fileprivate var size: CGFloat {
    switch self {
    case .displayLarge: AuraTokens.displayLarge
    case .display: AuraTokens.display
    case .numeralXL: AuraTokens.numeralXL
    case .numeralLarge: AuraTokens.numeralLarge
    case .numeralMedium: AuraTokens.numeralMedium
    case .numeralSmall: AuraTokens.numeralSmall
    case .title: AuraTokens.title
    case .label: AuraTokens.label
    case .body: AuraTokens.body
    case .sub: AuraTokens.sub
    case .caption: AuraTokens.caption
    case .eyebrow: AuraTokens.eyebrow
    }
  }

  fileprivate var weight: Font.Weight {
    switch self {
    case .displayLarge, .display: .regular
    case .numeralXL, .numeralLarge: .semibold
    case .numeralMedium, .numeralSmall, .title: .medium
    case .label: .medium
    case .body, .sub: .regular
    case .caption, .eyebrow: .semibold
    }
  }

  /// Tracking in points at the default content size.
  var tracking: CGFloat {
    switch self {
    case .eyebrow: 0.2
    case .caption: 0.1
    case .displayLarge, .display, .title: -0.4
    case .numeralXL: -2.2
    case .numeralLarge: -1.3
    case .numeralMedium, .numeralSmall: -0.5
    case .label, .body, .sub: 0
    }
  }

  /// The text style each role scales against. Keeping the numerals on `.largeTitle` and the
  /// chrome on `.caption1` is what stops a large accessibility size turning a screen into three
  /// numbers.
  fileprivate var metric: UIFont.TextStyle {
    switch self {
    case .displayLarge, .display: .title1
    case .numeralXL, .numeralLarge: .largeTitle
    case .numeralMedium, .numeralSmall, .title: .title3
    case .label: .body
    case .body, .sub: .callout
    case .caption, .eyebrow: .caption1
    }
  }

  /// A pure function of the role and the content size, so a test can assert that every role grows
  /// monotonically across the whole Dynamic Type range without rendering anything.
  func pointSize(for typeSize: DynamicTypeSize) -> CGFloat {
    UIFontMetrics(forTextStyle: metric).scaledValue(
      for: size,
      compatibleWith: UITraitCollection(preferredContentSizeCategory: typeSize.contentSizeCategory)
    )
  }

  func font(for typeSize: DynamicTypeSize) -> Font {
    .system(size: pointSize(for: typeSize), weight: weight, design: face.design)
  }
}

extension DynamicTypeSize {
  /// The UIKit category each SwiftUI size corresponds to. Needed because `UIFontMetrics` scales
  /// against a trait collection, and a trait collection speaks in categories.
  var contentSizeCategory: UIContentSizeCategory {
    switch self {
    case .xSmall: .extraSmall
    case .small: .small
    case .medium: .medium
    case .large: .large
    case .xLarge: .extraLarge
    case .xxLarge: .extraExtraLarge
    case .xxxLarge: .extraExtraExtraLarge
    case .accessibility1: .accessibilityMedium
    case .accessibility2: .accessibilityLarge
    case .accessibility3: .accessibilityExtraLarge
    case .accessibility4: .accessibilityExtraExtraLarge
    case .accessibility5: .accessibilityExtraExtraExtraLarge
    @unknown default: .large
    }
  }
}

// MARK: - Applying a role

private struct MavTypeModifier: ViewModifier {
  let role: MavType
  @Environment(\.dynamicTypeSize) private var typeSize

  func body(content: Content) -> some View {
    content
      .font(role.font(for: typeSize))
      .tracking(role.tracking)
  }
}

extension View {
  /// Set a type role. Reading `dynamicTypeSize` from the environment here is what makes the text
  /// re-resolve when the user changes their content size, rather than at first layout only.
  func mavType(_ role: MavType) -> some View {
    modifier(MavTypeModifier(role: role))
  }

  /// The focus ring, in the one shape the whole app uses.
  func mavFocusRing(_ visible: Bool, cornerRadius: CGFloat = 14) -> some View {
    overlay {
      if visible {
        RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
          .inset(by: -3)
          .strokeBorder(MavTheme.focus, lineWidth: 2.5)
      }
    }
  }
}
