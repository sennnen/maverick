import SwiftUI

// MARK: - Design system (Maverick WHOOP-only rebuild) — editorial, Veri-inspired
//
// Pure-black editorial canvas (with a light variant), rounded tiles carrying a
// luminous RADIAL COLOUR GLOW, and clean Helvetica Neue typography — thin, large
// numerals; title-case labels; high contrast throughout. No dot-matrix. iOS
// Liquid Glass is reserved for chrome (tab bar, pills, FAB).
//
// Types are prefixed `Aura*`.

enum AuraDesign {

  // MARK: Metric families (radial glow hues)
  enum Family: String, CaseIterable {
    case charge      // recovery — jade green
    case rest        // sleep — deep ocean blue
    case effort      // strain — floral magenta
    case heart       // cardio/HR — rose
    case energy      // amber
    case vitals      // teal

    init(category: String) {
      switch category {
      case "Charge": self = .charge
      case "Rest": self = .rest
      case "Effort": self = .effort
      case "Heart": self = .heart
      case "Nutrition", "Mind": self = .energy
      default: self = .vitals
      }
    }

    /// Luminous glow centre — saturated on dark so the tile emits light; on light
    /// a vivid-but-soft core that reads as lit against the off-white canvas.
    var glow: Color {
      switch self {
      case .charge: AuraDesign.dyn(AuraTokens.chargeGlow)   // Rare Jade
      case .rest:   AuraDesign.dyn(AuraTokens.restGlow)     // Deep Ocean
      case .effort: AuraDesign.dyn(AuraTokens.effortGlow)   // Floral Magenta
      case .heart:  AuraDesign.dyn(AuraTokens.heartGlow)
      case .energy: AuraDesign.dyn(AuraTokens.energyGlow)
      case .vitals: AuraDesign.dyn(AuraTokens.vitalsGlow)
      }
    }

    /// Deep, still-tinted edge the glow blooms out of (a paler tint of the hue on
    /// light so the card keeps a soft radial falloff, not a flat fill).
    var glowEdge: Color {
      switch self {
      case .charge: AuraDesign.dyn(AuraTokens.chargeEdge)
      case .rest:   AuraDesign.dyn(AuraTokens.restEdge)
      case .effort: AuraDesign.dyn(AuraTokens.effortEdge)
      case .heart:  AuraDesign.dyn(AuraTokens.heartEdge)
      case .energy: AuraDesign.dyn(AuraTokens.energyEdge)
      case .vitals: AuraDesign.dyn(AuraTokens.vitalsEdge)
      }
    }
  }

  // MARK: Palette

  static let bg = dyn(AuraTokens.bg)
  static let card = dyn(AuraTokens.card)   // neutral (non-glow) card
  static let cardEdge = dyn(AuraTokens.cardEdge)

  /// Value / label ink. `ink` is the strong text colour, `inkDim` the muted one —
  /// both adapt to scheme, so they read on black and on the colour tiles alike.
  static let ink = dyn(AuraTokens.ink)

  /// Starship — the single INTERACTIVE hue (slider markers, range selection,
  /// tick emphasis). Never used as a status or decorative colour.
  static let accent = Color(UIColor(hex: AuraTokens.accent))
  /// Starship as TEXT/ICON ink: the raw hue on dark, an olive shift on light so
  /// small glyphs keep contrast on pale glass.
  static let accentInk = dyn(AuraTokens.accentInk)
  static let good = dyn(AuraTokens.good)
  static let fair = dyn(AuraTokens.fair)
  static let bad = dyn(AuraTokens.bad)

  // MARK: Shape

  static let screenMargin = AuraTokens.screenMargin
  static let cardSpacing = AuraTokens.cardSpacing
  static let sectionGap = AuraTokens.sectionGap
  static let tilePadding = AuraTokens.tilePadding

  static let cardRadius = AuraTokens.cardRadius
  static let tileRadius = AuraTokens.tileRadius

  static var cardShape: RoundedRectangle { RoundedRectangle(cornerRadius: cardRadius, style: .continuous) }
  static var tileShape: RoundedRectangle { RoundedRectangle(cornerRadius: tileRadius, style: .continuous) }

  // MARK: Type — Helvetica Neue

  /// Elegant thin numerals — the hero look ("73", "62%").
  static func mega(_ size: CGFloat) -> Font { .custom("HelveticaNeue-UltraLight", size: size) }
  static func number(_ size: CGFloat) -> Font { .custom("HelveticaNeue-Thin", size: size) }
  /// Roman display headings ("Good evening", screen titles).
  static func display(_ size: CGFloat) -> Font { .custom("HelveticaNeue", size: size) }
  static func heading(_ size: CGFloat) -> Font { .custom("HelveticaNeue-Medium", size: size) }
  static let title = Font.custom("HelveticaNeue-Medium", size: AuraTokens.title)
  static let label = Font.custom("HelveticaNeue-Medium", size: AuraTokens.label)
  static let sub = Font.custom("HelveticaNeue", size: AuraTokens.sub)
  static let caption = Font.custom("HelveticaNeue-Medium", size: AuraTokens.caption)

  // MARK: Contrast tokens
  //
  // Every hairline / scrim / grid uses one of these — never a hard-coded white
  // or black — so both schemes clear the contrast floor.

  /// Card hairline border. Ink-based so it's visible on light too.
  static let hairline = dynAlpha(AuraTokens.hairline)
  /// Translucent pill scrim that reads on BOTH the glow tiles and dark cards.
  static let scrim = dynAlpha(AuraTokens.scrim)
  /// Chart gridline.
  static let grid = dynAlpha(AuraTokens.grid)

  // MARK: Helpers

  /// Resolve a generated (dark, light) token against the active scheme.
  static func dyn(_ token: (dark: UInt32, light: UInt32)) -> Color {
    Color(UIColor {
      $0.userInterfaceStyle == .dark ? UIColor(hex: token.dark) : UIColor(hex: token.light)
    })
  }

  static func dynAlpha(_ token: (dark: (UInt32, CGFloat), light: (UInt32, CGFloat))) -> Color {
    Color(UIColor {
      $0.userInterfaceStyle == .dark
        ? UIColor(hex: token.dark.0).withAlphaComponent(token.dark.1)
        : UIColor(hex: token.light.0).withAlphaComponent(token.light.1)
    })
  }
}

// MARK: - Effort display (stored 0–100; WHOOP 0–21 is display-only, #268)

enum AuraEffort {
  /// Render a STORED 0–100 effort value on the user's chosen axis: integer on
  /// the native 0–100, one decimal on WHOOP 0–21 where the tenth matters.
  static func text(_ stored: Double?) -> String {
    guard let stored else { return "--" }
    let f = UnitPrefs.currentEffortDisplayFactor()
    let v = stored * f
    return f == 1.0 ? String(Int(v.rounded())) : String(format: "%.1f", v)
  }
}

// MARK: - Status semantics (the WHOOP colour language)
//
// ONE green/yellow/red mapping shared by every ring, chip and number. Family
// hues carry a metric's identity; AuraStatus carries its judgement. An element
// is owned by exactly one of the two systems, never both.

enum AuraStatus {
  case good, fair, low, none

  var color: Color {
    switch self {
    case .good: AuraDesign.good
    case .fair: AuraDesign.fair
    case .low:  AuraDesign.bad
    case .none: AuraDesign.ink.opacity(0.45)
    }
  }

  var chipKind: AuraStatusChip.Kind {
    switch self {
    case .good: .positive
    case .fair: .caution
    case .low:  .negative
    case .none: .neutral
    }
  }

  /// Recovery / Charge %, WHOOP bands: 67+ green, 34–66 yellow, <34 red.
  static func recovery(_ v: Double?) -> AuraStatus {
    guard let v else { return .none }
    return v >= 67 ? .good : v >= 34 ? .fair : .low
  }

  /// Sleep performance %.
  static func sleep(_ v: Double?) -> AuraStatus {
    guard let v else { return .none }
    return v >= 85 ? .good : v >= 70 ? .fair : .low
  }

  /// Day strain (0–21 Borg-ish scale): informational — high isn't "bad", so it
  /// maps light→fair only when very low vs. an active day.
  static func strain(_ v: Double?) -> AuraStatus {
    guard v != nil else { return .none }
    return .good
  }

  /// A vital vs. its baseline: |z|-style banding on a fractional deviation.
  static func deviation(_ frac: Double?, tolerance: Double = 0.10) -> AuraStatus {
    guard let f = frac else { return .none }
    let a = abs(f)
    return a <= tolerance ? .good : a <= tolerance * 2 ? .fair : .low
  }

  var word: String {
    switch self {
    case .good: "Good"
    case .fair: "Fair"
    case .low:  "Low"
    case .none: "No data"
    }
  }
}

extension UIColor {
  convenience init(hex: UInt32) {
    self.init(
      red: CGFloat((hex >> 16) & 0xFF) / 255,
      green: CGFloat((hex >> 8) & 0xFF) / 255,
      blue: CGFloat(hex & 0xFF) / 255,
      alpha: 1
    )
  }
}
