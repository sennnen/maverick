import SwiftUI

// Recovery hub — the Charge deep-dive AND the Health Monitor: recovery score
// with contributors, the five vitals with status vs baseline (tap → full
// interactive history), and illness / cycle signals.

struct AuraRecoveryView: View {
  @EnvironmentObject private var repo: Repository
  @EnvironmentObject private var model: AppModel

  @State private var editing = false
  @AppStorage(AuraHubCards.storageKey("recovery")) private var hiddenCSV = ""

  @State private var day: DailyMetric?
  @State private var vitalsDay: DailyMetric?
  @State private var revealed = false
  @State private var selected: AuraDetailData?

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: AuraDesign.sectionGap) {
        AuraHubHeader(title: "Recovery",
                      subtitle: "How ready your body is today",
                      editing: $editing)
          .auraReveal(revealed, index: 0)

        Button { selected = chargeDetail } label: { hero }
          .buttonStyle(AuraPressStyle())
          .auraReveal(revealed, index: 1)

        AuraEditableCard(key: "monitor", hiddenCSV: $hiddenCSV, editing: editing) {
          healthMonitor
        }
        .auraReveal(revealed, index: 2)

        AuraEditableCard(key: "signals", hiddenCSV: $hiddenCSV, editing: editing) {
          signals
        }
        .auraReveal(revealed, index: 3)

        AuraEditableCard(key: "mlsignals", hiddenCSV: $hiddenCSV, editing: editing) {
          AuraMLSignalsCard(engine: model.strandML)
        }
        .auraReveal(revealed, index: 4)

        AuraEditableCard(key: "trend", hiddenCSV: $hiddenCSV, editing: editing) {
          trendCard
        }
        .auraReveal(revealed, index: 5)
      }
      .padding(.horizontal, AuraDesign.screenMargin)
      .padding(.top, 8)
      .padding(.bottom, 128)
    }
    .scrollIndicators(.hidden)
    .auraScreen(.charge)
    .sheet(item: $selected) { AuraMetricDetailView(data: $0).presentationDragIndicator(.visible) }
    .task(id: repo.refreshSeq) { await load(); withAnimation { revealed = true } }
    .refreshable { await repo.refresh() }
  }

  private func load() {
    day = Repository.widgetAnchor(days: repo.days)
    vitalsDay = Repository.lastVitalsDay(days: repo.days)
  }

  // MARK: Hero

  private var recovery: Double? { day?.recovery }
  private var status: AuraStatus { .recovery(recovery) }

  private var hero: some View {
    VStack(alignment: .leading, spacing: 0) {
      HStack(alignment: .firstTextBaseline) {
        Text("Charge")
          .font(AuraDesign.number(44)).foregroundStyle(AuraDesign.ink)
        Spacer()
        HStack(alignment: .firstTextBaseline, spacing: 3) {
          Text(recovery.map { "\(Int($0.rounded()))" } ?? "--")
            .font(AuraDesign.number(44)).foregroundStyle(AuraDesign.ink)
            .lineLimit(1).minimumScaleFactor(0.5)
          if recovery != nil {
            Text("%")
              .font(AuraDesign.number(24)).foregroundStyle(AuraDesign.ink.opacity(0.66))
          }
        }
      }
      .frame(maxWidth: .infinity, minHeight: 88)
      .auraGlowTile(.charge, padding: 22, radius: 34)
      Text(insight)
        .font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.8))
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 22)
        .padding(.vertical, 18)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(AuraDesign.card, in: AuraDesign.cardShape)
    .overlay(AuraDesign.cardShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
  }

  private var statusLine: String {
    switch status {
    case .good: "Recovered"
    case .fair: "Adequate"
    case .low: "Run down"
    case .none: "No data"
    }
  }

  private var insight: String {
    switch status {
    case .good: "Your body absorbed yesterday's load. A big day is on the table."
    case .fair: "Partial recharge. Train, but leave something in reserve."
    case .low: "Your body is asking for rest. Keep intensity low today."
    // The core's structured reason, when it gave one — never a platform-invented explanation.
    case .none: model.recoveryUnavailableReason ?? "No recovery data yet."
    }
  }

  private var chargeDetail: AuraDetailData {
    let v = vitals
    return AuraDetailData(
      family: .charge, title: "Charge", value: recovery, unit: "%",
      baseline: baselineOf(points(\.recovery)),
      status: status,
      caption: "How recovered you are, led by overnight HRV against your own baseline.",
      points: points(\.recovery),
      heroFraction: (recovery ?? 0) / 100,
      contributors: v.map {
        .init(label: $0.label, value: $0.value.map($0.display) ?? "--",
              level: $0.level, tint: $0.family.glow)
      })
  }

  // MARK: Health Monitor

  private struct Vital: Identifiable {
    let id: String
    let label: String
    let family: AuraDesign.Family
    let value: Double?
    let baseline: Double?
    let display: (Double) -> String
    let unit: String
    let decimals: Int
    let status: AuraStatus
    let points: [(day: String, value: Double)]
    let caption: String
    let level: Double
  }

  private func points(_ path: KeyPath<DailyMetric, Double?>) -> [(day: String, value: Double)] {
    repo.days.compactMap { d in d[keyPath: path].map { (d.day, $0) } }
  }

  private func baselineOf(_ pts: [(day: String, value: Double)]) -> Double? {
    let v = pts.dropLast().suffix(21).map(\.value)
    return v.isEmpty ? nil : v.reduce(0, +) / Double(v.count)
  }

  private var vitals: [Vital] {
    func frac(_ v: Double?, _ b: Double?) -> Double? {
      guard let v, let b, b != 0 else { return nil }
      return (v - b) / b
    }

    let hrvPts = points(\.avgHrv)
    let rhrPts = repo.days.compactMap { d in d.restingHr.map { (d.day, Double($0)) } }
    let spo2Pts = points(\.spo2Pct)
    let tempPts = points(\.skinTempDevC)
    let respPts = points(\.respRateBpm)

    let hrv = day?.avgHrv ?? vitalsDay?.avgHrv
    let rhr = (day?.restingHr ?? vitalsDay?.restingHr).map(Double.init)
    let spo2 = day?.spo2Pct ?? vitalsDay?.spo2Pct
    let temp = day?.skinTempDevC ?? vitalsDay?.skinTempDevC
    let resp = day?.respRateBpm ?? vitalsDay?.respRateBpm
    let hrvB = baselineOf(hrvPts), rhrB = baselineOf(rhrPts), respB = baselineOf(respPts)

    return [
      // The core decides whether these beats may be called HRV at all; an optical pulse is
      // pulse-rate variability, and saying otherwise is the one claim this app must not make.
      Vital(id: "hrv",
            label: auraVariabilityTitle((day ?? vitalsDay)?.hrvLabel),
            family: .charge, value: hrv, baseline: hrvB,
            display: { "\(Int($0.rounded()))" }, unit: "ms", decimals: 0,
            // A DROP below baseline is the warning direction.
            status: hrv == nil ? .none : .deviation(frac(hrv, hrvB).map { Swift.min($0, 0) }, tolerance: 0.12),
            points: hrvPts,
            caption: (day ?? vitalsDay)?.hrvLabel == "heart_rate_variability"
              ? "Beat-to-beat variability from your heart's electrical signal while you sleep. Higher than your baseline is good."
              : "Beat-to-beat variability timed from your pulse while you sleep. Related to HRV but not the same measurement. Higher than your baseline is good.",
            level: (hrv ?? 0) / 140),
      Vital(id: "rhr", label: "Resting HR", family: .heart, value: rhr, baseline: rhrB,
            display: { "\(Int($0.rounded()))" }, unit: "bpm", decimals: 0,
            // A RISE above baseline is the warning direction.
            status: rhr == nil ? .none : .deviation(frac(rhr, rhrB).map { Swift.max($0, 0) }, tolerance: 0.08),
            points: rhrPts,
            caption: "Your lowest sustained overnight heart-rate. Lower than your baseline is good.",
            level: 1 - (rhr ?? 60) / 100),
      Vital(id: "spo2", label: "Blood O₂", family: .vitals, value: spo2, baseline: baselineOf(spo2Pts),
            display: { "\(Int($0.rounded()))" }, unit: "%", decimals: 0,
            status: spo2 == nil ? .none : (spo2! >= 95 ? .good : spo2! >= 92 ? .fair : .low),
            points: spo2Pts,
            caption: "Mean blood-oxygen saturation during sleep. 95%+ is typical.",
            level: (spo2 ?? 0) / 100),
      Vital(id: "temp", label: "Skin Temp", family: .heart, value: temp, baseline: nil,
            display: { $0 > 0 ? String(format: "+%.1f", $0) : String(format: "%.1f", $0) },
            unit: "°C", decimals: 1,
            status: temp == nil ? .none : (abs(temp!) <= 0.4 ? .good : abs(temp!) <= 0.8 ? .fair : .low),
            points: tempPts,
            caption: "Deviation from your own overnight skin-temperature baseline. Spikes often precede illness.",
            level: 0.5 + (temp ?? 0) / 4),
      Vital(id: "resp", label: "Respiratory", family: .vitals, value: resp, baseline: respB,
            display: { String(format: "%.1f", $0) }, unit: "rpm", decimals: 1,
            status: resp == nil ? .none : .deviation(frac(resp, respB).map { Swift.max($0, 0) }, tolerance: 0.08),
            points: respPts,
            caption: "Breaths per minute during sleep. Steady for you is healthy.",
            level: (resp ?? 0) / 25),
    ]
  }

  private var healthMonitor: some View {
    VStack(alignment: .leading, spacing: 14) {
      AuraSectionHeader(title: "Health Monitor")
      VStack(spacing: 0) {
        let all = vitals
        ForEach(all) { v in
          vitalRow(v)
          if v.id != all.last?.id {
            Rectangle().fill(AuraDesign.hairline).frame(height: 1).padding(.leading, 18)
          }
        }
      }
      .padding(.vertical, 4)
      .background(AuraDesign.card, in: AuraDesign.tileShape)
      .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
    }
  }

  private func vitalRow(_ v: Vital) -> some View {
    Button {
      selected = AuraDetailData(
        family: v.family, title: v.label, value: v.value, unit: v.unit,
        decimals: v.decimals, baseline: v.baseline, status: v.status,
        caption: v.caption, points: v.points,
        provenance: "Measured overnight by your strap; baseline is your own trailing 21 days.")
    } label: {
      HStack(spacing: 14) {
        Circle().fill(v.status.color).frame(width: 8, height: 8)
          .shadow(color: v.status.color.opacity(0.8), radius: 4)
        Text(v.label).font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
        Spacer(minLength: 8)
        VStack(alignment: .trailing, spacing: 2) {
          HStack(alignment: .firstTextBaseline, spacing: 3) {
            Text(v.value.map(v.display) ?? "--")
              .font(AuraDesign.number(22)).foregroundStyle(AuraDesign.ink)
            Text(v.unit).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
          }
          if let b = v.baseline {
            Text("baseline \(v.decimals == 0 ? String(Int(b.rounded())) : String(format: "%.1f", b))")
              .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.45))
          }
        }
        Image(systemName: "chevron.right")
          .font(.system(size: 12, weight: .semibold))
          .foregroundStyle(AuraDesign.ink.opacity(0.35))
      }
      .padding(.horizontal, 18)
      .padding(.vertical, 13)
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
  }

  // MARK: Signals

  @ViewBuilder private var signals: some View {
    // Illness and cycle awareness are declared analytics the core does not yet serve. The card
    // states which, and why, rather than disappearing or showing a number this app computed for
    // itself — one metric, one implementation (ADR-024).
    VStack(alignment: .leading, spacing: 14) {
      AuraSectionHeader(title: "Signals")
      VStack(spacing: AuraDesign.cardSpacing) {
        AuraUnavailableCard(title: "Heads-up", entry: availability("illnessrisk"))
        AuraUnavailableCard(title: "Cycle phase", entry: availability("cyclephase"))
      }
    }
  }

  private func availability(_ id: String) -> AnalyticAvailabilityReport? {
    model.dailySnapshot?.availability.first { $0.analytic == id }
  }

  // MARK: Trend

  private var trendCard: some View {
    VStack(alignment: .leading, spacing: 14) {
      AuraSectionHeader(title: "Last month")
      AuraGraph(points: Array(points(\.recovery).suffix(30)),
                tint: AuraDesign.Family.charge.glow, unit: "%", style: .bars)
        .auraDarkCard()
    }
  }
}
