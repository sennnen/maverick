import Foundation

// The written half of `Today`.
//
// This is the one surface in the app where placeholder copy is allowed, and it is fenced: sample
// text exists only in a debug build, and it is always badged on screen. A release build with no
// model wired shows the honest not-yet-generated state. On-device generation (Foundation Models on
// iOS, Gemini Nano on Android) and bring-your-own-key advisor chat are a later lane; this is the
// surface and the empty state, nothing more.

enum MavNarrativeState: Equatable, Sendable {
  /// Written on-device from the day's own read models.
  case generated(headline: String, body: String)
  /// Fixture copy. Debug builds only, and always rendered behind a visible SAMPLE badge.
  case sample(headline: String, body: String)
  /// Nothing has been generated. Says so.
  case unavailable(reason: String)

  var headline: String? {
    switch self {
    case .generated(let headline, _), .sample(let headline, _): headline
    case .unavailable: nil
    }
  }

  var body: String? {
    switch self {
    case .generated(_, let body), .sample(_, let body): body
    case .unavailable: nil
    }
  }

  var isSample: Bool {
    if case .sample = self { return true }
    return false
  }
}

/// What a narrative is asked for. Keeping this a protocol is what lets the model lane land later
/// without touching a screen.
protocol MavNarrativeProviding: Sendable {
  func daily(day: String, rows: [MavMetricRow]) -> MavNarrativeState
  func trend(id: String, rows: [MavMetricRow]) -> MavNarrativeState
}

/// The provider until a model is wired.
///
/// In release it returns `.unavailable` for everything, which is the truth. In debug it returns
/// fixture copy so the layout can be judged — and the screen badges it, so a screenshot can never
/// be mistaken for a working feature.
struct MavStubNarrativeProvider: MavNarrativeProviding {
  static let notGeneratedYet =
    "On-device summaries are not wired up yet. When they are, this is where the day gets explained "
    + "in words, written on your phone from your own data."

  func daily(day: String, rows: [MavMetricRow]) -> MavNarrativeState {
    #if DEBUG
      return .sample(
        headline: "Your resting pulse is trending lower",
        body: "Your three-week average is down while overnight variability remains steady.")
    #else
      return .unavailable(reason: Self.notGeneratedYet)
    #endif
  }

  func trend(id: String, rows: [MavMetricRow]) -> MavNarrativeState {
    #if DEBUG
      switch id {
      case "resilience":
        return .sample(
          headline: "Recovery has stayed consistent over the past month.", body: "")
      case "cardio_load":
        return .sample(
          headline: "Training load has increased gradually over eight weeks.", body: "")
      default:
        return .unavailable(reason: Self.notGeneratedYet)
      }
    #else
      return .unavailable(reason: Self.notGeneratedYet)
    #endif
  }
}
