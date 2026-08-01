import SwiftUI

// The pushed destination from a `Vitals` row.
//
// No tab bar, no bottom switcher: a back button, a title, and the metric. You back out to move
// sideways. The provenance block at the bottom is not a debug affordance — it is the answer to
// "where did this number come from", and every number in this app is expected to be able to answer.

struct MavMetricDetailView: View {
  let metric: MavMetric
  @EnvironmentObject private var model: AppModel
  @EnvironmentObject private var repo: Repository
  @EnvironmentObject private var live: LiveState
  @State private var range = MavRange.month
  @State private var selection: Int?

  private let narrative: MavNarrativeProviding = MavStubNarrativeProvider()

  enum MavRange: String, CaseIterable, Identifiable {
    case week = "1W", month = "1M", quarter = "3M", half = "6M", year = "1Y"
    var id: String { rawValue }
    var days: Int {
      switch self {
      case .week: 7
      case .month: 30
      case .quarter: 90
      case .half: 182
      case .year: 365
      }
    }
  }

  /// The same crop the Vitals row used, so opening a metric grows its card into the page instead
  /// of swapping in a different photograph. Kept in step with `MavVitalRow.sceneCrop`.
  private var sceneCrop: MavScene.Crop {
    switch metric.family {
    case .charge, .heart: .high
    case .rest, .energy: .middle
    case .effort, .vitals, .cycle: .low
    }
  }

  private var state: MavMetricState {
    MavMetricMapper.state(of: metric, in: model.dailySnapshot)
  }

  var body: some View {
    MavDetailScaffold(title: metric.name, scene: sceneCrop) {
      narrativeCard

      if metric.id == "heart_rate" {
        liveHeartRate
      }

      MavRangePicker(selection: $range)

      chart

      contributors

      MavSectionHeader(title: "Where this number came from")
      provenance
    }
  }

  // MARK: Narrative

  @ViewBuilder private var narrativeCard: some View {
    switch state {
    case .value(let text, _, let band, _, let word):
      MavStatusCard(family: metric.family) {
        VStack(alignment: .leading, spacing: 11) {
          Text(word).mavType(.caption).foregroundStyle(MavTheme.inkSecondary)
          Text("\(text)\(metric.unit.map { " \($0)" } ?? "")")
            .mavType(.numeralXL)
            .foregroundStyle(MavTheme.ink)
          if let band {
            Text(
              "Your normal range is \(MavMetricMapper.decimal(band.low, places: 0)) to "
              + "\(MavMetricMapper.decimal(band.high, places: 0)) \(metric.unit ?? ""), "
              + "measured from your own last seven days."
            )
            .mavType(.body)
            .foregroundStyle(MavTheme.inkSecondary)
            .fixedSize(horizontal: false, vertical: true)
          } else {
            Text("The core has not published a normal range for this metric yet.")
              .mavType(.body)
              .foregroundStyle(MavTheme.inkSecondary)
              .fixedSize(horizontal: false, vertical: true)
          }
        }
      }
    case .unavailable(let reason):
      MavUnavailableCard(name: metric.name, reason: reason)
    }
  }

  // MARK: Chart

  private var liveHeartRate: some View {
    MavTile {
      HStack {
        VStack(alignment: .leading, spacing: 5) {
          Text("Live").mavType(.caption).foregroundStyle(MavTheme.inkSecondary)
          HStack(alignment: .firstTextBaseline, spacing: 4) {
            Text(model.bpm.map(String.init) ?? "—")
              .mavType(.numeralLarge)
              .monospacedDigit()
              .foregroundStyle(MavTheme.ink)
            Text("bpm").mavType(.sub).foregroundStyle(MavTheme.inkSecondary)
          }
        }
        Spacer()
        Text(
          live.connected
            ? "From \(live.advertisingName ?? "your device")"
            : "Connect a device for a live reading")
          .mavType(.sub)
          .foregroundStyle(MavTheme.inkSecondary)
          .multilineTextAlignment(.trailing)
          .frame(maxWidth: 150, alignment: .trailing)
      }
      .accessibilityElement(children: .combine)
    }
  }

  @ViewBuilder private var chart: some View {
    let points = series
    if points.count > 1 {
      MavTile {
        VStack(alignment: .leading, spacing: 8) {
          let selectedPoint = selection.flatMap {
            points.indices.contains($0) ? points[$0] : nil
          } ?? points.last
          HStack(alignment: .firstTextBaseline) {
            Text(
              MavMetricMapper.decimal(
                selectedPoint?.value ?? 0,
                places: metric.unit == "°C" ? 1 : 0))
              .mavType(.numeralMedium)
              .foregroundStyle(MavTheme.ink)
            if let unit = metric.unit {
              Text(unit).mavType(.sub).foregroundStyle(MavTheme.inkSecondary)
            }
            Spacer()
            Text(selectedPoint?.label ?? "")
              .mavType(.sub)
              .foregroundStyle(MavTheme.inkSecondary)
          }
          MavSeriesChart(
            points: points,
            band: bandBounds,
            family: metric.family,
            accessibilitySummary: chartSummary(points),
            selection: $selection)
          HStack {
            Text(points.first?.label ?? "")
            Spacer()
            Text(points.last?.label ?? "")
          }
          .mavType(.sub)
          .foregroundStyle(MavTheme.inkSecondary)
        }
      }
    } else {
      MavTile {
        VStack(alignment: .leading, spacing: 6) {
          Text("Not enough history").mavType(.title).foregroundStyle(MavTheme.ink)
          Text(
            "A chart needs at least two scored days. The core has "
            + "\(points.count == 1 ? "one so far" : "none for this metric yet")."
          )
          .mavType(.body)
          .foregroundStyle(MavTheme.inkSecondary)
          .fixedSize(horizontal: false, vertical: true)
        }
      }
    }
  }

  /// The series comes from the days the core has actually scored. A gap in the history is a fact
  /// about the recording, so a day with no value is simply not a point rather than a zero.
  private var series: [MavSeriesChart.Point] {
    #if DEBUG
      if model.usingDebugFixture {
        if metric.id == "variability" {
          return repo.days.suffix(range.days).compactMap { day in
            day.avgHrv.map {
              MavSeriesChart.Point(label: String(day.day.suffix(5)), value: $0)
            }
          }
        }
        let base: Double
        switch state {
        case .value(_, let numeric, _, _, _): base = numeric
        case .unavailable: base = 50
        }
        let days = Array(repo.days.suffix(range.days))
        let shape = Array(MavDebugFixture.scoreHistory.suffix(days.count))
        let amplitude = max(abs(base) * 0.045, 0.12)
        return Array(zip(days, shape)).map { day, score in
          MavSeriesChart.Point(
            label: String(day.day.suffix(5)),
            value: base + (score - 82) * amplitude / 4)
        }
      }
    #endif
    return repo.days.suffix(range.days).compactMap { day in
      let value: Double?
      switch metric.id {
      case "variability": value = day.avgHrv
      case "heart_rate": value = day.restingHr.map(Double.init)
      default: value = nil
      }
      return value.map {
        MavSeriesChart.Point(label: String(day.day.suffix(5)), value: $0)
      }
    }
  }

  private var bandBounds: (low: Double, high: Double)? {
    guard case .value(_, _, let band, _, _) = state, let band else { return nil }
    return (band.low, band.high)
  }

  private func chartSummary(_ points: [MavSeriesChart.Point]) -> String {
    let values = points.map(\.value)
    let low = values.min() ?? 0
    let high = values.max() ?? 0
    let latest = values.last ?? 0
    return
      "\(metric.name) over \(range.rawValue). \(points.count) days, from "
      + "\(MavMetricMapper.decimal(low, places: 0)) to \(MavMetricMapper.decimal(high, places: 0)), "
      + "latest \(MavMetricMapper.decimal(latest, places: 0)). Swipe up or down to scrub."
  }

  // MARK: Contributors

  @ViewBuilder private var contributors: some View {
    if let hrv = model.dailySnapshot?.hrv, metric.id == "variability" {
      MavSectionHeader(title: "Inside the number")
      VStack(spacing: 0) {
        detailRow("RMSSD", MavMetricMapper.decimal(hrv.rmssdMs, places: 1) + " ms")
        MavDivider()
        detailRow("SDNN", MavMetricMapper.decimal(hrv.sdnnMs, places: 1) + " ms")
        MavDivider()
        detailRow("Mean interval", MavMetricMapper.decimal(hrv.meanIntervalMs, places: 1) + " ms")
        MavDivider()
        detailRow("pNN50", MavMetricMapper.decimal(hrv.pnn50Percent, places: 1) + "%")
        MavDivider()
        detailRow(
          "Intervals used", "\(hrv.intervalCount), \(hrv.excludedCount) excluded")
      }
      .mavSurface(MavTheme.tileShape)
    } else if metric.id == "heart_rate", let snapshot = model.dailySnapshot {
      MavSectionHeader(title: "Inside the number")
      VStack(spacing: 0) {
        detailRow("Current", model.bpm.map { "\($0) bpm" } ?? "No live reading")
        MavDivider()
        detailRow(
          "Day average",
          snapshot.meanBpm.map { MavMetricMapper.decimal($0, places: 0) + " bpm" }
            ?? "No day average")
        MavDivider()
        detailRow(
          "Samples", "\(snapshot.hrSampleCount), \(snapshot.hrExcludedCount) excluded")
      }
      .mavSurface(MavTheme.tileShape)
    }
  }

  private func detailRow(_ title: String, _ value: String) -> some View {
    MavRow(title: title) {
      Text(value)
        .mavType(.label)
        .monospacedDigit()
        .foregroundStyle(MavTheme.inkSecondary)
    }
    .accessibilityElement(children: .combine)
  }

  // MARK: Provenance

  private var provenance: some View {
    MavTile {
      VStack(alignment: .leading, spacing: 7) {
        if let snapshot = model.dailySnapshot {
          provenanceLine("Day", snapshot.day)
          if !snapshot.algorithms.isEmpty {
            provenanceLine("Algorithms", snapshot.algorithms.joined(separator: ", "))
          }
          if let hrv = model.dailySnapshot?.hrv {
            provenanceLine("Interval label", hrv.label)
          }
          provenanceLine(
            "Heart-rate samples",
            "\(snapshot.hrSampleCount) used, \(snapshot.hrExcludedCount) excluded")
          provenanceLine("Snapshot", snapshot.snapshotHash)
        } else {
          Text("No snapshot loaded.").mavType(.body).foregroundStyle(MavTheme.inkSecondary)
        }
      }
    }
  }

  private func provenanceLine(_ key: String, _ value: String) -> some View {
    HStack(alignment: .top, spacing: 10) {
      Text(key).mavType(.sub).foregroundStyle(MavTheme.inkSecondary).frame(width: 132, alignment: .leading)
      Text(value)
        .mavType(.sub)
        .foregroundStyle(MavTheme.ink)
        .fixedSize(horizontal: false, vertical: true)
      Spacer(minLength: 0)
    }
    .accessibilityElement(children: .combine)
  }
}

/// The range picker — a real segmented `Picker`, not a row of buttons dressed as one. The system
/// control brings its own keyboard traversal, its own VoiceOver phrasing ("2 of 5"), and its own
/// Liquid Glass selection, none of which a hand-rolled version gets.
struct MavRangePicker: View {
  @Binding var selection: MavMetricDetailView.MavRange

  var body: some View {
    Picker("Range", selection: $selection) {
      ForEach(MavMetricDetailView.MavRange.allCases) { range in
        Text(range.rawValue)
          .accessibilityLabel(label(range))
          .tag(range)
      }
    }
    .pickerStyle(.segmented)
    .accessibilityLabel("Time range")
  }

  private func label(_ range: MavMetricDetailView.MavRange) -> String {
    switch range {
    case .week: "One week"
    case .month: "One month"
    case .quarter: "Three months"
    case .half: "Six months"
    case .year: "One year"
    }
  }
}
