import SwiftUI

// Today — the overview home that is also *you*: the three pillar rings with
// status colour + a plain-language day insight + live HR at a glance; then the
// morning-Journal nudge, the Coach entry, and Reports + Trends links. Tapping
// a ring jumps to that pillar's hub tab.

// MARK: - Display model

struct AuraTodayModel: Equatable {
  var charge: Double?
  var rest: Double?
  var effort: Double?
  var hrv: Double?
  /// The core's own label for `hrv`, so the tile is titled rather than assumed.
  var hrvLabel: String?
  var restingHr: Int?
  var respRate: Double?
  var spo2: Double?
  var skinTemp: Double?
  var sleepMin: Double?
  var chargeDelta: Double?
  var bpm: Int?
  var battery: Int?
  var bonded = false
  var deviceName = "Wearable"

  static let placeholder = AuraTodayModel()

  @MainActor
  static func load(repo: Repository, live: LiveState, bpm: Int?) async -> AuraTodayModel {
    let days = repo.days
    let day = Repository.widgetAnchor(days: days)
    let vitalsDay = Repository.lastVitalsDay(days: days)

    var rest: Double?
    if let day {
      let series = await repo.exploreSeries(key: "sleep_performance", source: Repository.activeDeviceSource)
      let byDay = Dictionary(series.map { ($0.day, $0.value) }, uniquingKeysWith: { _, last in last })
      let isToday = day.day == Repository.localDayKey(Date())
      rest = byDay[day.day] ?? (isToday ? series.last?.value : nil)
    }

    let history = days.dropLast().suffix(21).compactMap(\.recovery)
    let chargeBaseline = history.isEmpty ? nil : history.reduce(0, +) / Double(history.count)

    return AuraTodayModel(
      charge: day?.recovery, rest: rest, effort: day?.strain,
      hrv: day?.avgHrv ?? vitalsDay?.avgHrv,
      hrvLabel: (day ?? vitalsDay)?.hrvLabel,
      restingHr: day?.restingHr ?? vitalsDay?.restingHr,
      respRate: day?.respRateBpm ?? vitalsDay?.respRateBpm,
      spo2: day?.spo2Pct ?? vitalsDay?.spo2Pct,
      skinTemp: day?.skinTempDevC ?? vitalsDay?.skinTempDevC,
      sleepMin: day?.totalSleepMin,
      chargeDelta: chargeBaseline.flatMap { b in day?.recovery.map { $0 - b } },
      bpm: bpm ?? live.heartRate,
      battery: live.batteryPct.map { Int($0.rounded()) },
      bonded: live.bonded, deviceName: live.advertisingName ?? "Wearable"
    )
  }
}

// MARK: - Today

struct AuraTodayView: View {
  @EnvironmentObject private var model: AppModel
  @EnvironmentObject private var repo: Repository
  @EnvironmentObject private var live: LiveState

  @Environment(\.auraSwitchTab) private var switchTab

  @State private var data = AuraTodayModel.placeholder
  @State private var revealed = false
  @State private var editing = false
  @AppStorage(AuraHubCards.storageKey("today")) private var hiddenCSV = ""

  @State private var showJournal = false
  @State private var showCoach = false
  @State private var showReports = false
  @State private var showTrends = false
  @State private var showLive = false
  @State private var showTimer = false

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: AuraDesign.sectionGap) {
        AuraHubHeader(title: greeting,
                      subtitle: Date.now.formatted(.dateTime.weekday(.wide).month().day()),
                      editing: $editing)
          .auraReveal(revealed, index: 0)

        hero.auraReveal(revealed, index: 1)

        AuraLiveHRPill(bpm: data.bpm, deviceName: data.deviceName,
                       batteryPercent: data.battery, bonded: data.bonded) { showLive = true }
          .auraReveal(revealed, index: 2)

        AuraEditableCard(key: "journal", hiddenCSV: $hiddenCSV, editing: editing) {
          journalNudge
        }
        .auraReveal(revealed, index: 3)

        AuraEditableCard(key: "vitals", hiddenCSV: $hiddenCSV, editing: editing) {
          vitals
        }
        .auraReveal(revealed, index: 4)

        AuraEditableCard(key: "coach", hiddenCSV: $hiddenCSV, editing: editing) {
          coachCard
        }
        .auraReveal(revealed, index: 5)

        AuraEditableCard(key: "links", hiddenCSV: $hiddenCSV, editing: editing) {
          linkTiles
        }
        .auraReveal(revealed, index: 6)
      }
      .padding(.horizontal, AuraDesign.screenMargin)
      .padding(.top, 8)
      .padding(.bottom, 128)
    }
    .scrollIndicators(.hidden)
    .auraScreen(.charge)
    .sheet(isPresented: $showJournal) { AuraJournalView().presentationDragIndicator(.visible) }
    .sheet(isPresented: $showCoach) { AuraCoachView().presentationDragIndicator(.visible) }
    .sheet(isPresented: $showReports) { AuraReportsView().presentationDragIndicator(.visible) }
    .sheet(isPresented: $showTrends) { AuraTrendsView().presentationDragIndicator(.visible) }
    .sheet(isPresented: $showLive) { NavigationStack { AuraLiveView() }.presentationDragIndicator(.visible) }
    .sheet(isPresented: $showTimer) { AuraTimerView(timer: model.countdown).presentationDragIndicator(.visible) }
    .task(id: repo.refreshSeq) {
      data = await AuraTodayModel.load(repo: repo, live: live, bpm: model.bpm)
    }
    .onChange(of: model.bpm) { _, new in data.bpm = new ?? live.heartRate }
    .onChange(of: live.heartRate) { _, new in if model.bpm == nil { data.bpm = new } }
    .onAppear { revealed = true }
    .refreshable { await repo.refresh() }
  }

  private var greeting: String {
    switch Calendar.current.component(.hour, from: .now) {
    case 5..<12: "Good morning"
    case 12..<17: "Good afternoon"
    case 17..<22: "Good evening"
    default: "Good night"
    }
  }

  // MARK: Hero — the three pillar rings + day insight

  private var chargeStatus: AuraStatus { .recovery(data.charge) }
  private var restStatus: AuraStatus { .sleep(data.rest) }

  private var hero: some View {
    VStack(alignment: .leading, spacing: 22) {
      HStack(spacing: 10) {
        ring("Charge", data.charge.map { "\(Int($0.rounded()))" } ?? "--",
             value: data.charge, max: 100, status: chargeStatus, tab: .recovery)
        ring("Effort", AuraEffort.text(data.effort),
             value: data.effort, max: 100,
             status: data.effort == nil ? AuraStatus.none : .good, tab: .strain,
             tint: AuraDesign.Family.effort.glow)
        ring("Rest", data.rest.map { "\(Int($0.rounded()))" } ?? "--",
             value: data.rest, max: 100, status: restStatus, tab: .sleep)
      }
      Text(insight)
        .font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.78))
        .fixedSize(horizontal: false, vertical: true)
    }
    .padding(.vertical, 22)
    .padding(.horizontal, 18)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(AuraDesign.card, in: AuraDesign.cardShape)
    .overlay(AuraDesign.cardShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
  }

  private func ring(_ label: String, _ text: String, value: Double?, max: Double,
                    status: AuraStatus, tab: AuraTab, tint: Color? = nil) -> some View {
    Button { switchTab(tab) } label: {
      AuraScoreRing(value: value, maxValue: max, text: text, label: label,
                    status: status, tintOverride: tint, size: 88, lineWidth: 6)
        .frame(maxWidth: .infinity)
        .contentShape(Circle())
    }
    .buttonStyle(AuraPressStyle())
    .accessibilityHint(Text("Opens \(label)"))
  }

  private var insight: String {
    switch (chargeStatus, restStatus) {
    case (.good, .good): "Recovered and rested. Today can take whatever you want to give it."
    case (.good, _): "Your body recharged well even if sleep fell short. Green light, gently."
    case (.fair, _): "A middling recharge. Train, but keep something in reserve."
    case (.low, _): "Recovery is low. Today is for easy movement and an early night."
    case (.none, _): "Wear your strap tonight and tomorrow starts with a score."
    }
  }

  // MARK: Journal nudge

  private var journalNudge: some View {
    Button { showJournal = true } label: {
      HStack(spacing: 14) {
        Image(systemName: "sun.horizon")
          .font(.system(size: 18, weight: .medium))
          .foregroundStyle(AuraDesign.Family.energy.glow)
          .frame(width: 28)
        VStack(alignment: .leading, spacing: 3) {
          Text("Morning journal").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Text("Log last night's behaviours to sharpen your recovery insights.")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.6))
            .fixedSize(horizontal: false, vertical: true)
        }
        Spacer(minLength: 8)
        Image(systemName: "chevron.right")
          .font(.system(size: 12, weight: .semibold)).foregroundStyle(AuraDesign.ink.opacity(0.35))
      }
      .padding(18)
      .frame(maxWidth: .infinity, alignment: .leading)
      .background(AuraDesign.card, in: AuraDesign.tileShape)
      .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
      .contentShape(Rectangle())
    }
    .buttonStyle(AuraPressStyle())
  }

  // MARK: Vitals at a glance

  private var vitals: some View {
    VStack(alignment: .leading, spacing: 14) {
      AuraSectionHeader(title: "Vitals")
      LazyVGrid(columns: [GridItem(.flexible(), spacing: 22), GridItem(.flexible(), spacing: 22)],
                spacing: 22) {
        AuraMiniStat(value: intText(data.hrv), unit: "ms", label: auraVariabilityTitle(data.hrvLabel),
                     level: (data.hrv ?? 0) / 140, tint: AuraDesign.Family.charge.glow)
        AuraMiniStat(value: data.restingHr.map { "\($0)" } ?? "--", unit: "bpm", label: "Resting HR",
                     level: 1 - Double(data.restingHr ?? 60) / 100, tint: AuraDesign.Family.heart.glow)
        AuraMiniStat(value: decText(data.respRate, 1), unit: "rpm", label: "Respiratory",
                     level: (data.respRate ?? 0) / 25, tint: AuraDesign.Family.vitals.glow)
        AuraMiniStat(value: intText(data.spo2), unit: "%", label: "Blood O₂",
                     level: (data.spo2 ?? 0) / 100, tint: AuraDesign.Family.vitals.glow)
        AuraMiniStat(value: tempText, unit: "°C", label: "Skin Temp",
                     level: 0.5 + (data.skinTemp ?? 0) / 4, tint: AuraDesign.Family.heart.glow)
        AuraMiniStat(value: hmText(data.sleepMin), label: "Slept",
                     level: (data.sleepMin ?? 0) / 540, tint: AuraDesign.Family.rest.glow)
      }
      .padding(20)
      .frame(maxWidth: .infinity)
      .background(AuraDesign.card, in: AuraDesign.tileShape)
      .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
    }
  }

  // MARK: Coach

  private var coachCard: some View {
    Button { showCoach = true } label: {
      HStack(spacing: 14) {
        Image(systemName: "sparkles")
          .font(.system(size: 18, weight: .medium))
          .foregroundStyle(AuraDesign.Family.effort.glow)
          .frame(width: 28)
        VStack(alignment: .leading, spacing: 3) {
          Text("Coach").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Text("Ask anything about your data. Private: your key, your device.")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.6))
            .fixedSize(horizontal: false, vertical: true)
        }
        Spacer(minLength: 8)
        Image(systemName: "chevron.right")
          .font(.system(size: 12, weight: .semibold)).foregroundStyle(AuraDesign.ink.opacity(0.35))
      }
      .padding(18)
      .frame(maxWidth: .infinity, alignment: .leading)
      .background(AuraDesign.card, in: AuraDesign.tileShape)
      .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
      .contentShape(Rectangle())
    }
    .buttonStyle(AuraPressStyle())
  }

  // MARK: Reports + Trends

  private var linkTiles: some View {
    HStack(spacing: AuraDesign.cardSpacing) {
      linkTile("Reports", "Week · month", icon: "doc.text") { showReports = true }
      linkTile("Trends", "1w · 1m · 6m", icon: "chart.line.uptrend.xyaxis") { showTrends = true }
      linkTile("Timer", timerSub, icon: "timer") { showTimer = true }
    }
  }

  /// Live countdown readout on the tile while the timer runs, so it's glanceable
  /// without opening the sheet.
  private var timerSub: String {
    if model.countdown.isRinging { return "Time's up" }
    guard let r = model.countdown.remaining else { return "Wrist buzz" }
    return String(format: "%d:%02d left", r / 60, r % 60)
  }

  private func linkTile(_ title: String, _ sub: String, icon: String,
                        action: @escaping () -> Void) -> some View {
    Button(action: action) {
      VStack(alignment: .leading, spacing: 8) {
        Image(systemName: icon)
          .font(.system(size: 18, weight: .medium))
          .foregroundStyle(AuraDesign.accentInk)
        Text(title).font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          .lineLimit(1).minimumScaleFactor(0.8)
        // One line always, so three tiles in a row stay the same height (a wrapping
        // subtitle used to make Reports taller than its neighbours).
        Text(sub).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
          .lineLimit(1).minimumScaleFactor(0.75)
      }
      .padding(16)
      // maxHeight lets every tile stretch to the tallest in the row → uniform cards.
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
      .background(AuraDesign.card, in: AuraDesign.tileShape)
      .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
      .contentShape(Rectangle())
    }
    .buttonStyle(AuraPressStyle())
  }

  // MARK: Formatting

  private func intText(_ v: Double?) -> String { v.map { String(Int($0.rounded())) } ?? "--" }
  private func decText(_ v: Double?, _ d: Int) -> String { v.map { String(format: "%.\(d)f", $0) } ?? "--" }
  private func hmText(_ m: Double?) -> String {
    guard let m, m > 0 else { return "--" }
    let t = Int(m.rounded()); return "\(t / 60)h \(t % 60)m"
  }
  private var tempText: String {
    guard let t = data.skinTemp else { return "--" }
    let s = String(format: "%.1f", t)
    return t > 0 ? "+\(s)" : s
  }
}
