import SwiftUI

// Multi-horizon trends: 1w / 1m / 6m range switching with real date ticks,
// one labelled dark-card chart per metric. Surfaced from Today.

struct AuraTrendsView: View {
  @EnvironmentObject private var repo: Repository
  @Environment(\.dismiss) private var dismiss
  var embedded = false   // true when pushed inside another stack (hides the X)

  @State private var range: AuraTrendRange = .month
  @State private var restSeries: [(day: String, value: Double)] = []

  private struct Metric: Identifiable {
    let id: String
    let title: String
    let family: AuraDesign.Family
    let unit: String
    let points: [(day: String, value: Double)]
  }

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 20) {
        HStack {
          AuraRangePicker(selection: $range)
          Spacer()
        }

        ForEach(metrics) { m in
          VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .firstTextBaseline) {
              Text(m.title).font(AuraDesign.heading(17)).foregroundStyle(AuraDesign.ink)
              Spacer()
              Text(m.points.last.map { String(Int($0.value.rounded())) } ?? "--")
                .font(AuraDesign.mega(40)).foregroundStyle(m.family.glow)
              if !m.unit.isEmpty {
                Text(m.unit).font(AuraDesign.number(16)).foregroundStyle(AuraDesign.ink.opacity(0.55))
              }
            }
            AuraGraph(points: m.points, tint: m.family.glow, unit: m.unit,
                      style: m.id == "str" ? .bars : .line)
          }
          .auraDarkCard(padding: 18)
        }
      }
      .padding(.horizontal, AuraDesign.screenMargin)
      .padding(.bottom, 48)
    }
    .scrollIndicators(.hidden)
    .auraScreen(.charge)
    .safeAreaInset(edge: .top) { bar }
    .task(id: repo.refreshSeq) {
      restSeries = await repo.exploreSeries(key: "sleep_performance", source: Repository.activeDeviceSource)
    }
  }

  private var bar: some View {
    HStack {
      Text("Trends").font(AuraDesign.heading(20)).foregroundStyle(AuraDesign.ink)
      Spacer()
      if !embedded {
        Button { dismiss() } label: {
          Image(systemName: "xmark")
            .font(.system(size: 15, weight: .bold)).foregroundStyle(AuraDesign.ink)
            .frame(width: 40, height: 40)
            .background(.ultraThinMaterial, in: Circle())
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
      }
    }
    .padding(.horizontal, AuraDesign.screenMargin)
    .padding(.top, 10).padding(.bottom, 8)
  }

  private var metrics: [Metric] {
    let n = range.days
    func pts(_ path: KeyPath<DailyMetric, Double?>) -> [(day: String, value: Double)] {
      repo.days.suffix(n).compactMap { d in d[keyPath: path].map { (d.day, $0) } }
    }
    let rhr = repo.days.suffix(n).compactMap { d in d.restingHr.map { (d.day, Double($0)) } }
    return [
      Metric(id: "rec", title: "Charge", family: .charge, unit: "%", points: pts(\.recovery)),
      Metric(id: "rest", title: "Rest", family: .rest, unit: "%", points: Array(restSeries.suffix(n))),
      Metric(id: "str", title: "Effort", family: .effort, unit: "",
             points: pts(\.strain).map { ($0.day, $0.value * UnitPrefs.currentEffortDisplayFactor()) }),
      Metric(id: "hrv", title: auraVariabilityTitle(repo.days.suffix(n).last(where: { $0.hrvLabel != nil })?.hrvLabel),
             family: .charge, unit: "ms", points: pts(\.avgHrv)),
      Metric(id: "rhr", title: "Resting HR", family: .heart, unit: "bpm", points: rhr),
    ]
  }
}
