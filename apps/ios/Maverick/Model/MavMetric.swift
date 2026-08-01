import Foundation

// The metric catalogue and the mapping from the core's read models to what a screen draws.
//
// This file is the whole reason `Vitals` can be honest. It computes nothing: every number, band,
// and reason below is lifted out of a `DailySnapshotReport` exactly as the core produced it. What
// it *does* own is the presentation decisions the platform is allowed to make — which metrics the
// UI knows how to lay out, what each is called, which family it belongs to, and how a value is
// formatted for a locale.
//
// The rule it exists to enforce: a row is present because the core's availability set says the
// analytic is available, and absent-with-a-reason otherwise. There is no hardcoded list of rows
// with values filled in where they happen to exist.

// MARK: - The catalogue

/// A metric the UI knows how to draw. `analytic` is the core's own id, and matching against it is
/// the only link between this catalogue and the availability set.
struct MavMetric: Identifiable, Hashable, Sendable {
  enum Group: String, CaseIterable, Sendable {
    case scores = "Scores"
    case cycle = "Cycle"
    case vitals = "Vitals"
  }

  let id: String
  /// The core's analytic id. Nil for a metric that is a direct read of a stream rather than the
  /// output of an admitted analytic, which today is none of them.
  let analytic: String?
  let name: String
  let family: MavFamily
  let group: Group
  let unit: String?
  /// What the score rail calls it. A gauge is 74pt wide and "Resting heart rate" ellipsises to
  /// nonsense in that space, so every metric carries a name that fits.
  var shortName: String { short ?? name }
  private let short: String?

  init(
    id: String, analytic: String?, name: String, family: MavFamily, group: Group, unit: String?,
    short: String? = nil
  ) {
    self.id = id
    self.analytic = analytic
    self.name = name
    self.family = family
    self.group = group
    self.unit = unit
    self.short = short
  }

  static let catalogue: [MavMetric] = [
    MavMetric(
      id: "recovery", analytic: "recovery", name: "Recovery", family: .charge, group: .scores,
      unit: "%"),
    MavMetric(
      id: "sleep", analytic: "sleep_performance", name: "Sleep", family: .rest, group: .scores,
      unit: "%"),
    MavMetric(
      id: "effort", analytic: "daily_effort", name: "Activity", family: .effort, group: .scores,
      unit: nil),
    MavMetric(
      id: "variability", analytic: "time_domain_hrv", name: "Variability", family: .charge,
      group: .vitals, unit: "ms"),
    MavMetric(
      id: "heart_rate", analytic: nil, name: "Heart rate", family: .heart, group: .vitals,
      unit: "bpm"),
    MavMetric(
      id: "respiration", analytic: "respiration_rate", name: "Respiratory rate", family: .vitals,
      group: .vitals, unit: "brpm", short: "Respiration"),
    MavMetric(
      id: "blood_oxygen", analytic: "blood_oxygen", name: "Blood oxygen", family: .vitals,
      group: .vitals, unit: "%", short: "Blood O₂"),
    MavMetric(
      id: "skin_temperature", analytic: "skin_temperature", name: "Skin temperature",
      family: .energy, group: .vitals, unit: "°C", short: "Skin temp"),
    MavMetric(
      id: "illness_risk", analytic: "illness_risk", name: "Illness signals", family: .vitals,
      group: .vitals, unit: nil, short: "Illness"),
    MavMetric(
      id: "cycle_phase", analytic: "cycle_phase", name: "Cycle phase", family: .cycle,
      group: .cycle, unit: nil, short: "Cycle"),
  ]

  static func named(_ id: String) -> MavMetric? { catalogue.first { $0.id == id } }
}

// MARK: - A row

/// What a `Vitals` row draws. Either the core produced a value, or it said why it could not.
enum MavMetricState: Equatable, Sendable {
  /// `band` is the core's own normal range, and it is the only thing a range bar may be drawn
  /// from. A metric whose analytic publishes no band renders its value with no bar rather than a
  /// bar the app invented.
  case value(
    text: String, numeric: Double, band: MavBand?, status: MavStatus, word: String)
  case unavailable(reason: String)
}

/// The core's own normal range for a metric, plus where today sits inside it.
struct MavBand: Equatable, Sendable {
  let low: Double
  let high: Double
  let value: Double

  /// Where the marker sits, 0...1, over a track padded to 25% either side of the band so a value
  /// outside the range is still visible rather than clamped onto the end cap.
  var markerFraction: Double {
    let span = high - low
    guard span > 0 else { return 0.5 }
    let padded = span * 0.25
    let lo = low - padded
    let hi = high + padded
    return min(max((value - lo) / (hi - lo), 0), 1)
  }

  var lowFraction: Double {
    let span = high - low
    guard span > 0 else { return 0 }
    return 0.25 / 1.5
  }

  var highFraction: Double {
    let span = high - low
    guard span > 0 else { return 1 }
    return 1 - 0.25 / 1.5
  }
}

struct MavMetricRow: Identifiable, Equatable, Sendable {
  let metric: MavMetric
  let state: MavMetricState
  var id: String { metric.id }

  var isAvailable: Bool {
    if case .value = state { return true }
    return false
  }
}

// MARK: - The mapping

enum MavMetricMapper {

  /// Availability keys reach the app in two spellings: the host snapshot's JSON uses
  /// `time_domain_hrv`, while the FFI report derives its string from a Rust `Debug` and produces
  /// `timedomainhrv`. Normalising here is a presentation tolerance, not a computation — the app
  /// still reads whatever the core said, it just accepts both spellings of the same id.
  static func normalise(_ key: String) -> String {
    key.lowercased().replacingOccurrences(of: "_", with: "")
  }

  static func availability(
    _ reports: [AnalyticAvailabilityReport], for analytic: String
  ) -> AnalyticAvailabilityReport? {
    let wanted = normalise(analytic)
    return reports.first { normalise($0.analytic) == wanted }
  }

  /// The core's reason, worded for a person. The *kind* and the stream names are the core's; only
  /// the sentence around them is ours.
  static func reasonText(_ report: AnalyticAvailabilityReport?, metric: MavMetric) -> String {
    guard let report else {
      return "\(metric.name) is not something this core version reports."
    }
    switch report.reason {
    case "missing_streams":
      let streams = report.missingStreams.map(streamName).joined(separator: ", ")
      return streams.isEmpty
        ? "Waiting on a signal your strap has not sent yet."
        : "Waiting on \(streams) from your strap."
    case "algorithm_not_admitted":
      return "Not published yet — the calculation has no reference we can stand behind."
    case .some(let other):
      return "Unavailable: \(other)."
    case nil:
      return "Unavailable."
    }
  }

  /// Stream ids are the core's vocabulary; these are the same streams said out loud.
  static func streamName(_ stream: String) -> String {
    switch normalise(stream) {
    case "heartrate": "heart rate"
    case "rrinterval": "electrical beat intervals"
    case "pulseinterval": "optical beat intervals"
    case "skintemperature", "skintemperatureraw": "skin temperature"
    case "spo2", "bloodoxygen": "blood oxygen"
    case "respiration", "respirationrate": "respiration"
    case "sleepstage", "sleepstate": "sleep staging"
    case "accelerometer", "imu": "motion"
    default: stream.replacingOccurrences(of: "_", with: " ").lowercased()
    }
  }

  /// The whole `Vitals` list for a day, in catalogue order, with every metric accounted for.
  static func rows(
    from snapshot: DailySnapshotReport?, cycleEnabled: Bool, locale: Locale = .current
  ) -> [MavMetricRow] {
    MavMetric.catalogue.compactMap { metric in
      // Cycle follows the body profile: it appears automatically for a female profile and is
      // absent entirely for other profiles, rather than adding a second duplicate preference.
      if metric.group == .cycle, !cycleEnabled { return nil }
      return MavMetricRow(metric: metric, state: state(of: metric, in: snapshot, locale: locale))
    }
  }

  static func state(
    of metric: MavMetric, in snapshot: DailySnapshotReport?, locale: Locale = .current
  ) -> MavMetricState {
    guard let snapshot else {
      return .unavailable(reason: "No day loaded yet.")
    }

    #if DEBUG
      if snapshot.snapshotHash.hasPrefix("fixture-"), let sample = debugState(metric, snapshot) {
        return sample
      }
    #endif

    let report = metric.analytic.flatMap { availability(snapshot.availability, for: $0) }

    // Variability is the one metric the core fully produces today: a value, and a band from the
    // readiness baseline. Everything else in the catalogue is honestly unavailable until its
    // analytic is admitted.
    if metric.id == "variability", report?.available == true, let hrv = snapshot.hrv {
      let band = snapshot.readiness.map {
        MavBand(low: $0.normalLowMs, high: $0.normalHighMs, value: hrv.rmssdMs)
      }
      let tier = snapshot.readiness?.tier
      return .value(
        text: decimal(hrv.rmssdMs, places: 0, locale: locale),
        numeric: hrv.rmssdMs,
        band: band,
        status: status(forTier: tier),
        word: word(forTier: tier, label: hrv.label))
    }

    if metric.id == "heart_rate", snapshot.hrSampleCount > 0,
      let meanBpm = snapshot.meanBpm
    {
      return .value(
        text: decimal(meanBpm, places: 0, locale: locale),
        numeric: meanBpm,
        band: nil,
        status: .neutral,
        word: "Recorded")
    }

    if report?.available == true {
      // An analytic the core reports as available but this build has no value extractor for. Say
      // so rather than drawing an empty row; a blank number is the one thing that must never
      // reach a screen.
      return .unavailable(reason: "\(metric.name) is available but this app build cannot read it yet.")
    }

    return .unavailable(reason: reasonText(report, metric: metric))
  }

  #if DEBUG
    private static func debugState(
      _ metric: MavMetric, _ snapshot: DailySnapshotReport
    ) -> MavMetricState? {
      switch metric.id {
      case "recovery":
        return sampleValue("82", 82, 65, 88, .optimal, "Optimal")
      case "sleep":
        return sampleValue("78", 78, 72, 90, .fair, "Fair")
      case "effort":
        return sampleValue("11.6", 11.6, 8, 15, .optimal, "Balanced")
      case "variability":
        if let hrv = snapshot.hrv {
          return .value(
            text: decimal(hrv.rmssdMs, places: 0),
            numeric: hrv.rmssdMs,
            band: snapshot.readiness.map {
              MavBand(low: $0.normalLowMs, high: $0.normalHighMs, value: hrv.rmssdMs)
            },
            status: status(forTier: snapshot.readiness?.tier),
            word: word(forTier: snapshot.readiness?.tier, label: hrv.label))
        }
        return nil
      case "heart_rate":
        return sampleValue("68", 68, 58, 74, .optimal, "In range")
      case "respiration":
        return sampleValue("14.2", 14.2, 12.4, 15.8, .optimal, "In range")
      case "blood_oxygen":
        return sampleValue("97", 97, 95, 100, .optimal, "Optimal")
      case "skin_temperature":
        return sampleValue("+0.1", 0.1, -0.3, 0.4, .optimal, "Stable")
      case "illness_risk":
        return .value(
          text: "Low", numeric: 0.12, band: nil, status: .optimal, word: "No change")
      case "cycle_phase":
        return .value(
          text: "Day 15", numeric: 15, band: nil, status: .neutral, word: "Follicular")
      default:
        return nil
      }
    }

    private static func sampleValue(
      _ text: String, _ value: Double, _ low: Double, _ high: Double, _ status: MavStatus,
      _ word: String
    ) -> MavMetricState {
      .value(
        text: text,
        numeric: value,
        band: MavBand(low: low, high: high, value: value),
        status: status,
        word: word)
    }
  #endif

  /// The readiness tier is the core's judgement, mapped onto the only granularity a surface tint
  /// can express. Nothing here re-scores anything.
  static func status(forTier tier: String?) -> MavStatus {
    switch tier {
    case "primed": .optimal
    case "normal": .optimal
    case "suppressed": .low
    default: .neutral
    }
  }

  /// The status *word* comes from the core's tier, not from the tint. A metric with no tier says
  /// so instead of borrowing a verdict.
  static func word(forTier tier: String?, label: String?) -> String {
    switch tier {
    case "primed": "Primed"
    case "normal": "In range"
    case "suppressed": "Suppressed"
    default: label == "heart_rate_variability" ? "Measured" : "Provisional"
    }
  }

  /// What the core is willing to call a variability figure. Only beats timed from the heart's
  /// electrical signal are HRV; an optical pulse is a different event and reads as PRV.
  static func variabilityTitle(_ label: String?) -> String {
    label == "heart_rate_variability" ? "Heart-rate variability" : "Pulse-rate variability"
  }

  static func decimal(_ value: Double, places: Int, locale: Locale = .current) -> String {
    let formatter = NumberFormatter()
    formatter.locale = locale
    formatter.numberStyle = .decimal
    formatter.minimumFractionDigits = places
    formatter.maximumFractionDigits = places
    return formatter.string(from: NSNumber(value: value)) ?? "—"
  }
}

// MARK: - The score rail

/// One gauge on `Today`'s rail. An unavailable score is a dashed arc and an em dash — never a zero,
/// because a zero is a claim.
struct MavRailItem: Identifiable, Equatable, Sendable {
  let metric: MavMetric
  let text: String
  /// 0...1 of the arc to fill, or nil when there is nothing to fill it with.
  let fraction: Double?
  var id: String { metric.id }

  static func rail(from rows: [MavMetricRow]) -> [MavRailItem] {
    rows.map { row in
      switch row.state {
      case .value(let text, let numeric, let band, _, _):
        let fraction: Double
        switch row.metric.id {
        case "recovery", "sleep", "blood_oxygen": fraction = numeric / 100
        case "effort": fraction = numeric / 21
        case "cycle_phase": fraction = numeric / 30
        default: fraction = band?.markerFraction ?? 0.66
        }
        return MavRailItem(metric: row.metric, text: text, fraction: min(max(fraction, 0), 1))
      case .unavailable:
        return MavRailItem(metric: row.metric, text: "—", fraction: nil)
      }
    }
  }
}
