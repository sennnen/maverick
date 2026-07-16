import SwiftUI

// The one metric flyout: glow hero with status + baseline delta, an
// interactive scrubbable history graph with 1W/1M/6M ranges, range statistics,
// contributors, and provenance. Always closable from the sheet bar.

struct AuraDetailData: Identifiable {
  let id = UUID()
  let family: AuraDesign.Family
  let title: String
  let value: Double?
  let unit: String
  var decimals: Int = 0
  /// Baseline (21-day) to show the delta against; nil hides the delta.
  var baseline: Double?
  var status: AuraStatus = .none
  let caption: String
  /// Full day-keyed history (oldest → newest); the range picker clips it.
  var points: [(day: String, value: Double)] = []
  var barStyle = false
  /// 0–1 for the hero slider; nil hides it.
  var heroFraction: Double?
  var contributors: [Contributor] = []
  var provenance = "Computed on-device from your strap's raw signals."

  struct Contributor: Identifiable {
    let id = UUID()
    let label: String
    let value: String
    let level: Double
    let tint: Color
  }
}

struct AuraMetricDetailView: View {
  let data: AuraDetailData
  @State private var range: AuraTrendRange = .month

  var body: some View {
    AuraSheet(title: data.title, family: data.family) {
      hero

      VStack(alignment: .leading, spacing: 14) {
        HStack {
          AuraSectionHeader(title: "History")
          Spacer()
          AuraRangePicker(selection: $range)
        }
        AuraGraph(points: ranged, tint: data.family.glow, unit: data.unit,
                  style: data.barStyle ? .bars : .line, decimals: data.decimals)
          .auraDarkCard()
      }

      if !rangedValues.isEmpty { statsCard }
      if !data.contributors.isEmpty { contributorCard }

      Text(data.provenance)
        .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.45))
        .fixedSize(horizontal: false, vertical: true)
        .padding(.horizontal, 4)
    }
  }

  private var ranged: [(day: String, value: Double)] { Array(data.points.suffix(range.days)) }
  private var rangedValues: [Double] { ranged.map(\.value) }

  // MARK: Hero

  private var hero: some View {
    VStack(alignment: .leading, spacing: 18) {
      HStack {
        Text(data.title).auraLabel()
        Spacer()
        if data.status != .none {
          AuraStatusChip(text: data.status.word, kind: data.status.chipKind)
        } else if let d = delta {
          AuraDelta(value: d)
        }
      }
      HStack(alignment: .firstTextBaseline, spacing: 6) {
        Text(fmt(data.value))
          .font(AuraDesign.mega(76)).foregroundStyle(AuraDesign.ink)
          .lineLimit(1).minimumScaleFactor(0.4)
        if data.value != nil, !data.unit.isEmpty {
          Text(data.unit).font(AuraDesign.number(26)).foregroundStyle(AuraDesign.ink.opacity(0.66))
        }
      }
      if data.status != .none, let d = delta {
        AuraDelta(value: d)
      }
      if let f = data.heroFraction {
        AuraSlider(value: f, glow: data.family.glow)
      }
      Text(data.caption)
        .font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.8))
        .fixedSize(horizontal: false, vertical: true)
    }
    .frame(maxWidth: .infinity, minHeight: 210, alignment: .leading)
    .auraGlowTile(data.family, padding: 22, radius: 34)
  }

  private var delta: Double? {
    guard let v = data.value, let b = data.baseline else { return nil }
    return v - b
  }

  // MARK: Range stats

  private var statsCard: some View {
    let v = rangedValues
    let avg = v.reduce(0, +) / Double(v.count)
    return HStack(spacing: 18) {
      stat("Average", fmt(avg))
      stat("Lowest", fmt(v.min()))
      stat("Highest", fmt(v.max()))
      stat("Days", "\(v.count)")
    }
    .auraDarkCard(padding: 20)
  }

  private func stat(_ label: String, _ value: String) -> some View {
    VStack(alignment: .leading, spacing: 4) {
      Text(label).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
      Text(value).font(AuraDesign.number(22)).foregroundStyle(AuraDesign.ink)
        .monospacedDigit().lineLimit(1).minimumScaleFactor(0.6)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  // MARK: Contributors

  private var contributorCard: some View {
    VStack(alignment: .leading, spacing: 14) {
      AuraSectionHeader(title: "What feeds this")
      LazyVGrid(columns: [GridItem(.flexible(), spacing: 20), GridItem(.flexible(), spacing: 20)], spacing: 22) {
        ForEach(data.contributors) { c in
          AuraMiniStat(value: c.value, label: c.label, level: c.level, tint: c.tint)
        }
      }
      .auraDarkCard(padding: 20)
    }
  }

  private func fmt(_ v: Double?) -> String {
    guard let v else { return "--" }
    return data.decimals == 0 ? String(Int(v.rounded())) : String(format: "%.\(data.decimals)f", v)
  }
}
