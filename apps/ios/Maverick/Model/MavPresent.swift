import Foundation

enum MavPresent {
  /// A live sample older than this while the link is up is presented as stale (PL-P5/PL-P7).
  static let freshSampleMs: Int64 = 15_000

  /// The stale-data label for the live surface, from the snapshot's own observation time — the
  /// platform formats age, it never decides freshness semantics beyond this display threshold.
  /// Nil means nothing to show: a fresh streaming sample, or no samples and no link.
  static func sampleAgeLabel(asOfUnixMs: Int64, lastSampleUnixMs: Int64?, connected: Bool) -> String? {
    guard let lastSampleUnixMs else { return connected ? "Waiting for first sample" : nil }
    let ageMs = max(asOfUnixMs - lastSampleUnixMs, 0)
    if connected, ageMs <= freshSampleMs { return nil }
    return "Last sample \(relativeAge(ageMs: ageMs))"
  }

  private static func relativeAge(ageMs: Int64) -> String {
    switch ageMs {
    case ..<60_000: "\(ageMs / 1_000) s ago"
    case ..<3_600_000: "\(ageMs / 60_000) m ago"
    case ..<86_400_000: "\(ageMs / 3_600_000) h ago"
    default: "\(ageMs / 86_400_000) d ago"
    }
  }

  /// Fixed-point micros → "67.5 ms". Display formatting only; the value stays the core's.
  static func microsAsMs(_ micros: Int64, locale: Locale = .current) -> String {
    String(format: "%.1f ms", locale: locale, Double(micros) / 1_000)
  }

  /// Fixed-point milli-percent → "50.0%".
  static func milliPercentAsPercent(_ milliPercent: Int64, locale: Locale = .current) -> String {
    String(format: "%.1f%%", locale: locale, Double(milliPercent) / 1_000)
  }
}
