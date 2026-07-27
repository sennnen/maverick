import Foundation

/// Heart-rate zone math, resolved by the shared core.
///
/// There used to be three ladders in this app — Tanaka in the profile store, `220 − age` in the
/// strain view, and another `220 − age` in the settings stepper — beside a fourth in Rust that has
/// fixtures. Four implementations of one formula are four answers waiting to disagree, and the app
/// was showing two of them on the same screen. This defers to `mav-analytic::hr_zones` through the
/// FFI, exactly as the Android twin does, and holds no arithmetic of its own beyond the rounding
/// the display wants.
enum AuraZoneMath {
  /// Set once by the connector manager when the runtime opens.
  nonisolated(unsafe) static var runtime: MavRuntime?

  /// The Tanaka ceiling for an age. Falls back to the published formula only while the runtime is
  /// still opening — the same number, computed here rather than not at all, and it converges the
  /// moment the core is available.
  static func tanakaMaxHr(age: Int) -> Int {
    if let runtime {
      return Int(runtime.heartRateZones(age: Double(age), maxHrOverride: nil).maxHr.rounded())
    }
    return Int((208.0 - 0.7 * Double(age)).rounded())
  }

  /// The effective ceiling: a manual override when the wearer has set one, else the estimate.
  static func maxHr(age: Int, override manualOverride: Int) -> Int {
    manualOverride > 0 ? manualOverride : tanakaMaxHr(age: age)
  }

  /// The zone (1...5) a reading falls in; 0 below zone one.
  static func zone(bpm: Int, age: Int, maxHrOverride: Int?) -> Int {
    guard let runtime else { return 0 }
    return Int(
      runtime.heartRateZoneFor(
        bpm: Double(bpm), age: Double(age),
        maxHrOverride: maxHrOverride.map(Double.init)))
  }
}
