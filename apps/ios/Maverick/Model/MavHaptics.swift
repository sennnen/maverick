import Foundation

// The haptic vocabulary, host side. See ADR-032.
//
// A signal is a *meaning*, never a byte pattern. The app asks for "the goal is complete"; the
// connected connector decides what its device does about that, because which characteristic to
// write and with what opcode is device knowledge and does not belong in this binary.
//
// The vocabulary is closed and owned here. A connector declares which of these it can render; it
// cannot invent a new one, because a signal the host cannot name is a signal the host cannot decide
// to send, rate-limit, or explain to the wearer.
//
// The Kotlin twin is `MavHaptics.kt`.

/// One signal in the closed vocabulary.
enum MavHapticSignal: Hashable, Sendable, CaseIterable {
  /// A light tap: a distance marker passed, or a halfway point reached.
  case milestone
  /// A hard buzz: the end condition is met.
  case goalComplete
  /// A light tap confirming a strength set was recorded.
  case setLogged
  /// A hard buzz: the rest timer is done, start the next set.
  case restComplete
  /// A zone boundary was crossed. The zone names itself in the pattern.
  case zoneAlert(zone: Int)

  static var allCases: [MavHapticSignal] {
    [.milestone, .goalComplete, .setLogged, .restComplete]
      + (1...5).map { .zoneAlert(zone: $0) }
  }

  /// The stable wire name, which is what a manifest declares and what the snapshot lists.
  var id: String {
    switch self {
    case .milestone: "milestone"
    case .goalComplete: "goal_complete"
    case .setLogged: "set_logged"
    case .restComplete: "rest_complete"
    case .zoneAlert(let zone): "zone_alert_\(zone)"
    }
  }

  /// How the signal is described to the wearer, in the one place it is described.
  var explanation: String {
    switch self {
    case .milestone: "A light tap at each milestone"
    case .goalComplete: "A strong buzz when you reach your goal"
    case .setLogged: "A light tap when a set is recorded"
    case .restComplete: "A strong buzz when rest is over"
    case .zoneAlert(let zone): "A buzz when you cross into zone \(zone)"
    }
  }
}

/// What the *connected* connector said it can do.
///
/// Availability is negotiated rather than assumed. A strap that cannot buzz must never appear to
/// have agreed to: every setting built on a signal reads this first and renders the honest
/// unavailable state when the signal is absent.
struct MavHapticSupport: Equatable, Sendable {
  let signals: Set<String>

  /// Nothing declared. This is the current state of every shipped artifact — the Generic HR Monitor
  /// has no haptic characteristic at all — and it stays the value until the `haptics/v1` snapshot
  /// block from ADR-032 is plumbed through the core.
  static let none = MavHapticSupport(signals: [])

  var canBuzz: Bool { !signals.isEmpty }

  func supports(_ signal: MavHapticSignal) -> Bool { signals.contains(signal.id) }

  /// The sentence shown where a haptic setting would have been. It names *why*, in the same voice
  /// the unavailable-analytic component uses, rather than hiding the control and leaving the wearer
  /// wondering where it went.
  func reason(deviceName: String?) -> String {
    guard let deviceName, !deviceName.isEmpty else {
      return "No strap is connected, so there is nothing to buzz."
    }
    return "\(deviceName) does not report a haptic motor, so it cannot buzz."
  }
}
