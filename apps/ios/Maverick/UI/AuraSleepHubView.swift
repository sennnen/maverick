import SwiftUI

// Sleep hub — performance vs need, last night's hypnogram, stage breakdown,
// debt / consistency, naps, and the haptic alarm (existing Smart-Alarm engine).

struct AuraSleepHubView: View {
  @EnvironmentObject private var repo: Repository

  @State private var editing = false
  @AppStorage(AuraHubCards.storageKey("sleep")) private var hiddenCSV = ""
  @AppStorage("aura.sleep.targetWakeMin") private var wakeMin: Double = 420

  @State private var day: DailyMetric?
  @State private var restPct: Double?
  @State private var restTrend: [Double] = []
  @State private var night: [StageSegment] = []
  @State private var naps: [CachedSleepSession] = []
  @State private var figures: ImportedSleepFigures?
  @State private var revealed = false
  @State private var showAlarm = false

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: AuraDesign.sectionGap) {
        AuraHubHeader(title: "Sleep",
                      subtitle: "Last night, and what it bought you",
                      editing: $editing)
          .auraReveal(revealed, index: 0)

        hero.auraReveal(revealed, index: 1)

        AuraEditableCard(key: "stages", hiddenCSV: $hiddenCSV, editing: editing) {
          stagesCard
        }
        .auraReveal(revealed, index: 2)

        AuraEditableCard(key: "breakdown", hiddenCSV: $hiddenCSV, editing: editing) {
          breakdown
        }
        .auraReveal(revealed, index: 3)

        AuraEditableCard(key: "debt", hiddenCSV: $hiddenCSV, editing: editing) {
          debtCard
        }
        .auraReveal(revealed, index: 4)

        AuraEditableCard(key: "planner", hiddenCSV: $hiddenCSV, editing: editing) {
          plannerCard
        }
        .auraReveal(revealed, index: 5)

        AuraEditableCard(key: "naps", hiddenCSV: $hiddenCSV, editing: editing) {
          napsCard
        }
        .auraReveal(revealed, index: 6)

        alarmCard.auraReveal(revealed, index: 7)
      }
      .padding(.horizontal, AuraDesign.screenMargin)
      .padding(.top, 8)
      .padding(.bottom, 128)
    }
    .scrollIndicators(.hidden)
    .auraScreen(.rest)
    .sheet(isPresented: $showAlarm) {
      AuraAlarmView().presentationDragIndicator(.visible)
    }
    .task(id: repo.refreshSeq) { await load(); withAnimation { revealed = true } }
    .refreshable { await repo.refresh() }
  }

  // MARK: Load

  private func load() async {
    let anchor = Repository.widgetAnchor(days: repo.days)
    day = anchor
    let series = await repo.exploreSeries(key: "sleep_performance", source: Repository.activeDeviceSource)
    restTrend = series.suffix(21).map(\.value)
    if let d = anchor {
      let byDay = Dictionary(series.map { ($0.day, $0.value) }, uniquingKeysWith: { _, l in l })
      restPct = byDay[d.day] ?? (d.day == Repository.localDayKey(Date()) ? series.last?.value : nil)
      figures = repo.importedSleep[d.day]

      // Last night's sessions: any session ENDING on the anchor day. Longest
      // block is the night; short extras are naps.
      let f = DateFormatter()
      f.dateFormat = "yyyy-MM-dd"; f.locale = .init(identifier: "en_US_POSIX")
      let dayKey: (Int) -> String = { ts in f.string(from: Date(timeIntervalSince1970: TimeInterval(ts))) }
      let ofDay = repo.sleeps.filter { dayKey($0.endTs) == d.day }
      if let main = ofDay.max(by: { ($0.endTs - $0.effectiveStartTs) < ($1.endTs - $1.effectiveStartTs) }) {
        night = Self.decodeSegments(main.stagesJSON)
        naps = ofDay.filter { $0.startTs != main.startTs && ($0.endTs - $0.effectiveStartTs) < 3 * 3600 }
      } else {
        night = []; naps = []
      }
    }
  }

  /// Decode the on-device `[{start,end,stage}]` segment array (imported minute
  /// dicts carry no timeline, so they draw the fallback state instead).
  static func decodeSegments(_ json: String?) -> [StageSegment] {
    guard let json, let data = json.data(using: .utf8),
          let segs = try? JSONDecoder().decode([StageSegment].self, from: data) else { return [] }
    return segs.sorted { $0.start < $1.start }
  }

  // MARK: Hero

  private var status: AuraStatus { .sleep(restPct) }

  private var hero: some View {
    VStack(alignment: .leading, spacing: 0) {
      HStack(alignment: .firstTextBaseline) {
        Text("Rest")
          .font(AuraDesign.number(44)).foregroundStyle(AuraDesign.ink)
        Spacer()
        HStack(alignment: .firstTextBaseline, spacing: 3) {
          Text(restPct.map { String(Int($0.rounded())) } ?? "--")
            .font(AuraDesign.number(44)).foregroundStyle(AuraDesign.ink)
            .lineLimit(1).minimumScaleFactor(0.5)
          if restPct != nil {
            Text("%")
              .font(AuraDesign.number(24)).foregroundStyle(AuraDesign.ink.opacity(0.66))
          }
        }
      }
      .frame(maxWidth: .infinity, minHeight: 88)
      .auraGlowTile(.rest, padding: 22, radius: 34)
      Text(needLine)
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

  private var needLine: String {
    if let slept = day?.totalSleepMin, let need = figures?.needMin, need > 0 {
      return "You slept \(hm(slept)) of the \(hm(need)) your body needed."
    }
    return "How restorative last night was: duration, efficiency, deep and REM."
  }

  // MARK: Stages

  private var stagesCard: some View {
    VStack(alignment: .leading, spacing: 14) {
      AuraSectionHeader(title: "Last night")
      Group {
        if night.isEmpty {
          fallbackStageBar
        } else {
          AuraHypnogram(segments: night)
        }
      }
      .auraDarkCard(padding: 18)
    }
  }

  /// Proportional stacked bar for nights without a stage timeline (imports).
  @ViewBuilder private var fallbackStageBar: some View {
    let parts: [(String, Double, Color)] = [
      ("Deep", day?.deepMin ?? 0, AuraDesign.dyn(dark: 0x3E7BFF, light: 0x2F5FD0)),
      ("REM", day?.remMin ?? 0, AuraDesign.dyn(dark: 0x12AEBE, light: 0x0F93A1)),
      ("Light", day?.lightMin ?? 0, AuraDesign.dyn(dark: 0x6E9BFF, light: 0x5B82D8)),
    ]
    let total = parts.reduce(0) { $0 + $1.1 }
    if total > 0 {
      VStack(alignment: .leading, spacing: 12) {
        GeometryReader { g in
          HStack(spacing: 3) {
            ForEach(parts, id: \.0) { p in
              RoundedRectangle(cornerRadius: 3, style: .continuous)
                .fill(p.2)
                .frame(width: max(4, g.size.width * (p.1 / total)))
            }
          }
        }
        .frame(height: 14)
        HStack(spacing: 14) {
          ForEach(parts, id: \.0) { p in
            HStack(spacing: 5) {
              Circle().fill(p.2).frame(width: 7, height: 7)
              Text("\(p.0) \(hm(p.1))").font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.7))
            }
          }
        }
      }
    } else {
      Text("No staged sleep recorded")
        .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
    }
  }

  // MARK: Breakdown

  private var breakdown: some View {
    LazyVGrid(columns: [GridItem(.flexible(), spacing: 20), GridItem(.flexible(), spacing: 20)], spacing: 22) {
      AuraMiniStat(value: hmOpt(day?.totalSleepMin), label: "Asleep",
                   level: (day?.totalSleepMin ?? 0) / 540, tint: AuraDesign.Family.rest.glow)
      AuraMiniStat(value: day?.efficiency.map { "\(Int($0.rounded()))" } ?? "--", unit: "%", label: "Efficiency",
                   level: (day?.efficiency ?? 0) / 100, tint: AuraDesign.Family.charge.glow)
      AuraMiniStat(value: hmOpt(day?.deepMin), label: "Deep",
                   level: (day?.deepMin ?? 0) / 150, tint: AuraDesign.dyn(dark: 0x3E7BFF, light: 0x2F5FD0))
      AuraMiniStat(value: hmOpt(day?.remMin), label: "REM",
                   level: (day?.remMin ?? 0) / 150, tint: AuraDesign.Family.vitals.glow)
      AuraMiniStat(value: hmOpt(day?.lightMin), label: "Light",
                   level: (day?.lightMin ?? 0) / 300, tint: AuraDesign.dyn(dark: 0x6E9BFF, light: 0x5B82D8))
      AuraMiniStat(value: day?.disturbances.map { "\($0)" } ?? "--", label: "Disturbances",
                   level: Double(day?.disturbances ?? 0) / 20, tint: AuraDesign.Family.heart.glow)
    }
    .padding(20)
    .frame(maxWidth: .infinity)
    .background(AuraDesign.card, in: AuraDesign.tileShape)
    .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
  }

  // MARK: Debt / consistency

  @ViewBuilder private var debtCard: some View {
    if let f = figures, f.needMin != nil || f.debtMin != nil || f.consistencyPct != nil {
      VStack(alignment: .leading, spacing: 14) {
        AuraSectionHeader(title: "Sleep bank")
        HStack(spacing: 18) {
          if let need = f.needMin {
            bankStat("Need", hm(need), .none)
          }
          if let debt = f.debtMin {
            bankStat("Debt", hm(debt), debt <= 30 ? .good : debt <= 90 ? .fair : .low)
          }
          if let cons = f.consistencyPct {
            bankStat("Consistency", "\(Int(cons.rounded()))%",
                     cons >= 80 ? .good : cons >= 60 ? .fair : .low)
          }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .auraDarkCard(padding: 20)
      }
    }
  }

  private func bankStat(_ label: String, _ value: String, _ status: AuraStatus) -> some View {
    VStack(alignment: .leading, spacing: 6) {
      HStack(spacing: 6) {
        if status != .none {
          Circle().fill(status.color).frame(width: 7, height: 7)
            .shadow(color: status.color.opacity(0.8), radius: 4)
        }
        Text(label).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.6))
      }
      Text(value).font(AuraDesign.number(26)).foregroundStyle(AuraDesign.ink)
        .lineLimit(1).minimumScaleFactor(0.6)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  // MARK: Planner

  /// Target-wake slider → the bedtime that covers tonight's need (need + a
  /// settle allowance). Wake choice persists across launches.
  private var plannerCard: some View {
    VStack(alignment: .leading, spacing: 14) {
      AuraSectionHeader(title: "Sleep planner")
      VStack(alignment: .leading, spacing: 12) {
        HStack(alignment: .lastTextBaseline, spacing: 8) {
          Text("In bed by").font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.7))
          Text(clock(plannerBedMin)).font(AuraDesign.number(34)).foregroundStyle(AuraDesign.Family.rest.glow)
        }
        Text(plannerCaption)
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.6))
          .fixedSize(horizontal: false, vertical: true)
        Slider(value: $wakeMin, in: 240...660, step: 15)
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      .auraDarkCard(padding: 20)
    }
  }

  private var plannerNeedMin: Double { figures?.needMin ?? 480 }

  /// Minutes-after-midnight bedtime that covers tonight's need plus a 20m
  /// settle allowance, wrapped into the 0..<1440 clock range.
  private var plannerBedMin: Int {
    let raw = Int(wakeMin) - Int(plannerNeedMin.rounded()) - 20
    return ((raw % 1440) + 1440) % 1440
  }

  private var plannerCaption: String {
    var s = "Wake at \(clock(Int(wakeMin))) · covers your \(hm(plannerNeedMin)) need + 20m to drift off"
    if (figures?.debtMin ?? 0) > 30 {
      s += " · tonight's need already carries your debt"
    }
    return s
  }

  // MARK: Naps

  @ViewBuilder private var napsCard: some View {
    if !naps.isEmpty {
      VStack(alignment: .leading, spacing: 14) {
        AuraSectionHeader(title: "Naps")
        VStack(spacing: 0) {
          ForEach(naps.indices, id: \.self) { i in
            let n = naps[i]
            AuraInfoRow(label: Date(timeIntervalSince1970: TimeInterval(n.effectiveStartTs))
                          .formatted(date: .omitted, time: .shortened),
                        value: hm(Double(n.endTs - n.effectiveStartTs) / 60))
            if i < naps.count - 1 {
              Rectangle().fill(AuraDesign.ink.opacity(0.08)).frame(height: 1).padding(.leading, 18)
            }
          }
        }
        .padding(.vertical, 4)
        .background(AuraDesign.card, in: AuraDesign.tileShape)
        .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
      }
    }
  }

  // MARK: Alarm

  private var alarmCard: some View {
    VStack(spacing: 0) {
      AuraNavRow(icon: "alarm", title: "Haptic alarm",
                 detail: "Wake by wrist buzz",
                 tint: AuraDesign.accentInk) { showAlarm = true }
    }
    .padding(.vertical, 4)
    .background(AuraDesign.card, in: AuraDesign.tileShape)
    .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
  }

  // MARK: Format

  private func hm(_ m: Double) -> String {
    let t = Int(m.rounded()); return t >= 60 ? "\(t / 60)h \(t % 60)m" : "\(t)m"
  }
  private func hmOpt(_ m: Double?) -> String {
    guard let m, m > 0 else { return "--" }
    return hm(m)
  }

  /// Minutes-after-midnight → "HH:mm" clock text.
  private func clock(_ m: Int) -> String {
    String(format: "%02d:%02d", m / 60, m % 60)
  }
}
