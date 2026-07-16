import SwiftUI
import MapKit

// Strain hub — day strain building live, time in HR zones, the activities
// list (tap → workout detail), and workout start / live session entry.

struct AuraStrainView: View {
  @EnvironmentObject private var repo: Repository
  @EnvironmentObject private var model: AppModel
  @EnvironmentObject private var live: LiveState

  @State private var editing = false
  @AppStorage(AuraHubCards.storageKey("strain")) private var hiddenCSV = ""

  @State private var day: DailyMetric?
  @State private var todayRows: [WorkoutRow] = []
  @State private var recentRows: [WorkoutRow] = []
  /// This week's zone minutes + the rule-based targets (§8). nil until computed.
  @State private var weekDone: [Double]?
  @State private var weekTargets: [Double]?
  @State private var revealed = false
  @State private var selectedWorkout: WorkoutRow?
  @State private var showWorkouts = false
  @State private var showStrength = false
  @State private var showLive = false
  @State private var showTimer = false

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: AuraDesign.sectionGap) {
        AuraHubHeader(title: "Strain",
                      subtitle: "The load you're putting in",
                      editing: $editing)
          .auraReveal(revealed, index: 0)

        if model.activeWorkout != nil { liveBanner.auraReveal(revealed, index: 1) }

        hero.auraReveal(revealed, index: 2)

        startRow.auraReveal(revealed, index: 3)

        AuraEditableCard(key: "zones", hiddenCSV: $hiddenCSV, editing: editing) {
          zonesCard
        }
        .auraReveal(revealed, index: 4)

        AuraEditableCard(key: "weeklyTargets", hiddenCSV: $hiddenCSV, editing: editing) {
          weeklyTargetsCard
        }
        .auraReveal(revealed, index: 5)

        AuraEditableCard(key: "activities", hiddenCSV: $hiddenCSV, editing: editing) {
          activities
        }
        .auraReveal(revealed, index: 6)
      }
      .padding(.horizontal, AuraDesign.screenMargin)
      .padding(.top, 8)
      .padding(.bottom, 128)
    }
    .scrollIndicators(.hidden)
    .auraScreen(.effort)
    .sheet(item: $selectedWorkout) { AuraWorkoutDetailView(row: $0).presentationDragIndicator(.visible) }
    .sheet(isPresented: $showWorkouts) {
      AuraWorkoutsView().presentationDragIndicator(.visible)
    }
    .sheet(isPresented: $showLive) {
      NavigationStack { AuraLiveView() }.presentationDragIndicator(.visible)
    }
    .sheet(isPresented: $showStrength) {
      AuraStrengthView().presentationDragIndicator(.visible)
    }
    .sheet(isPresented: $showTimer) {
      AuraTimerView(timer: model.countdown).presentationDragIndicator(.visible)
    }
    .task(id: repo.refreshSeq) { await load(); withAnimation { revealed = true } }
    .refreshable { await repo.refresh() }
  }

  private func load() async {
    day = Repository.widgetAnchor(days: repo.days)
    // 6 weeks back: enough for the weekly-target engine's four full prior weeks (§8).
    let rows = await repo.workoutRows(days: 42).sorted { $0.startTs > $1.startTs }
    let f = DateFormatter()
    f.dateFormat = "yyyy-MM-dd"; f.locale = .init(identifier: "en_US_POSIX")
    let anchorKey = day?.day ?? Repository.localDayKey(Date())
    todayRows = rows.filter { f.string(from: Date(timeIntervalSince1970: TimeInterval($0.startTs))) == anchorKey }
    recentRows = Array(rows.prefix(14))

    // Weekly zone targets (§8): this week's banked minutes vs rule-based targets from
    // the previous full weeks + the recent Charge trend.
    let cal = Calendar.current
    let byWeek = TrainingTargets.weeklyZoneMinutes(rows: rows, calendar: cal)
    let thisWeek = cal.dateInterval(of: .weekOfYear, for: Date())?.start
    weekDone = thisWeek.flatMap { byWeek[$0] } ?? [Double](repeating: 0, count: 5)
    let priorWeeks = byWeek.filter { $0.key != thisWeek }.map(\.value)
    let recovery = repo.days.suffix(7).compactMap(\.recovery)
    weekTargets = TrainingTargets.weeklyTargets(
      recentWeeks: Array(priorWeeks.suffix(4)),
      recoveryAvg: recovery.isEmpty ? nil : recovery.reduce(0, +) / Double(recovery.count))
  }

  // MARK: Live banner

  private var liveBanner: some View {
    Button {
      // A strength session's home is its set sheet, not the live-HR screen.
      if model.strengthSession != nil { showStrength = true } else { showWorkouts = true }
    } label: {
      HStack(spacing: 12) {
        Circle().fill(AuraDesign.bad).frame(width: 8, height: 8)
          .shadow(color: AuraDesign.bad.opacity(0.9), radius: 5)
        Text(model.strengthSession != nil ? "Strength session running" : "Workout in progress")
          .font(AuraDesign.label).foregroundStyle(AuraDesign.ink)
        Spacer()
        if let w = model.activeWorkout {
          Text(AuraEffort.text(w.liveStrain))
            .font(AuraDesign.number(20)).foregroundStyle(AuraDesign.Family.effort.glow)
        }
        Image(systemName: "chevron.right")
          .font(.system(size: 12, weight: .semibold)).foregroundStyle(AuraDesign.ink.opacity(0.4))
      }
      .padding(.horizontal, 18).padding(.vertical, 14)
      .auraGlass(.capsule, interactive: true)
      .contentShape(.capsule)
    }
    .buttonStyle(AuraPressStyle())
  }

  // MARK: Hero

  private var strain: Double? { day?.strain }

  private var hero: some View {
    VStack(alignment: .leading, spacing: 0) {
      HStack(alignment: .firstTextBaseline) {
        Text("Effort")
          .font(AuraDesign.number(44)).foregroundStyle(AuraDesign.ink)
        Spacer()
        Text(AuraEffort.text(strain))
          .font(AuraDesign.number(44)).foregroundStyle(AuraDesign.ink)
          .lineLimit(1).minimumScaleFactor(0.5)
          .contentTransition(.identity)
      }
      .frame(maxWidth: .infinity, minHeight: 88)
      .auraGlowTile(.effort, padding: 22, radius: 34)
      Text("Cardiovascular load for the day, built from your heart-rate.")
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

  private var strainWord: String {
    guard let s = strain else { return "No data" }
    switch s {   // stored 0–100 axis
    case 86...: return "All out"
    case 67..<86: return "Strenuous"
    case 48..<67: return "Moderate"
    default: return "Light"
    }
  }

  // MARK: Start

  private var startRow: some View {
    VStack(spacing: 0) {
      AuraNavRow(icon: "play.fill", title: "Start a workout",
                 detail: "Live HR + strain", tint: AuraDesign.accentInk) { showWorkouts = true }
      Rectangle().fill(AuraDesign.ink.opacity(0.08)).frame(height: 1).padding(.leading, 18)
      AuraNavRow(icon: "dumbbell", title: "Strength trainer",
                 detail: "Sets · reps · load", tint: AuraDesign.Family.effort.glow) { showStrength = true }
      Rectangle().fill(AuraDesign.ink.opacity(0.08)).frame(height: 1).padding(.leading, 18)
      AuraNavRow(icon: "waveform.path.ecg", title: "Live heart-rate",
                 detail: live.heartRate.map { "\($0) bpm" } ?? "",
                 tint: AuraDesign.Family.heart.glow) { showLive = true }
      Rectangle().fill(AuraDesign.ink.opacity(0.08)).frame(height: 1).padding(.leading, 18)
      AuraNavRow(icon: "timer", title: "Timer",
                 detail: model.countdown.remaining.map { String(format: "%d:%02d", $0 / 60, $0 % 60) } ?? "Buzz at zero",
                 tint: AuraDesign.Family.energy.glow) { showTimer = true }
    }
    .padding(.vertical, 4)
    .background(AuraDesign.card, in: AuraDesign.tileShape)
    .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
  }

  // MARK: Zones (today)

  @ViewBuilder private var zonesCard: some View {
    if let summary = WorkoutZones.summary(from: todayRows) {
      VStack(alignment: .leading, spacing: 14) {
        AuraSectionHeader(title: "Time in zones")
        AuraZoneBars(minutes: summary.minutes)
          .auraDarkCard(padding: 18)
      }
    }
  }

  // MARK: Weekly zone targets (§8)

  @ViewBuilder private var weeklyTargetsCard: some View {
    if let done = weekDone, let targets = weekTargets, targets.contains(where: { $0 > 0 }) {
      VStack(alignment: .leading, spacing: 14) {
        AuraSectionHeader(title: "This week's zones")
        VStack(alignment: .leading, spacing: 12) {
          AuraZoneBars(minutes: done, targets: targets.map { $0 > 0 ? $0 : nil })
          Text(TrainingTargets.nudgeLine(done: done, targets: targets)
               ?? "Weekly targets met. Nice week.")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.6))
            .fixedSize(horizontal: false, vertical: true)
          Text("Targets adapt to your last four weeks and your recovery. Low Charge weeks plan easier.")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.4))
            .fixedSize(horizontal: false, vertical: true)
        }
        .auraDarkCard(padding: 18)
      }
    }
  }

  // MARK: Activities

  private var activities: some View {
    VStack(alignment: .leading, spacing: 14) {
      AuraSectionHeader(title: "Activities")
      if recentRows.isEmpty {
        Text("No workouts yet. Start one, or let auto-detection catch the next.")
          .font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.6))
          .auraDarkCard(padding: 18)
      } else {
        VStack(spacing: 0) {
          ForEach(recentRows.indices, id: \.self) { i in
            workoutRow(recentRows[i])
            if i < recentRows.count - 1 {
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

  private func workoutRow(_ w: WorkoutRow) -> some View {
    Button { selectedWorkout = w } label: {
      HStack(spacing: 14) {
        Image(systemName: Self.sportIcon(w.sport))
          .font(.system(size: 16, weight: .medium))
          .foregroundStyle(AuraDesign.Family.effort.glow)
          .frame(width: 26)
        VStack(alignment: .leading, spacing: 2) {
          Text(w.sport).font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Text(Date(timeIntervalSince1970: TimeInterval(w.startTs))
                 .formatted(.dateTime.weekday(.abbreviated).month(.abbreviated).day().hour().minute()))
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.5))
        }
        Spacer(minLength: 8)
        VStack(alignment: .trailing, spacing: 2) {
          if w.strain != nil {
            Text(AuraEffort.text(w.strain))
              .font(AuraDesign.number(20)).foregroundStyle(AuraDesign.Family.effort.glow)
          }
          Text(durText(w)).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
        }
        Image(systemName: "chevron.right")
          .font(.system(size: 12, weight: .semibold)).foregroundStyle(AuraDesign.ink.opacity(0.35))
      }
      .padding(.horizontal, 18).padding(.vertical, 13)
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
  }

  private func durText(_ w: WorkoutRow) -> String {
    let s = w.durationS ?? Double(w.endTs - w.startTs)
    let m = Int((s / 60).rounded())
    return m >= 60 ? "\(m / 60)h \(m % 60)m" : "\(m)m"
  }

  static func sportIcon(_ sport: String) -> String {
    switch sport.lowercased() {
    case "running", "trail running": "figure.run"
    case "walking": "figure.walk"
    case "hiking": "figure.hiking"
    case "cycling", "mountain biking", "spin": "figure.outdoor.cycle"
    case "swimming": "figure.pool.swim"
    case "rowing": "figure.rower"
    case "yoga", "pilates": "figure.mind.and.body"
    case "weightlifting", "functional fitness", "strength": "dumbbell"
    case "tennis", "padel", "squash", "badminton", "pickleball": "figure.tennis"
    case "football", "soccer": "figure.indoor.soccer"
    case "basketball": "figure.basketball"
    case "golf": "figure.golf"
    case "boxing", "martial arts", "kickboxing": "figure.boxing"
    case "skiing", "snowboarding": "figure.skiing.downhill"
    case "climbing", "rock climbing": "figure.climbing"
    default: "flame"
    }
  }
}

// MARK: - Workout detail flyout

struct AuraWorkoutDetailView: View {
  let row: WorkoutRow

  var body: some View {
    AuraSheet(title: row.sport, family: .effort) {
      AuraWorkoutSummary(row: row)
    }
  }
}

/// Shared scored-session summary (detail flyout + live-session end state).
struct AuraWorkoutSummary: View {
  let row: WorkoutRow
  @EnvironmentObject private var profile: ProfileStore
  @EnvironmentObject private var repo: Repository
  @State private var route: [RouteMath.LatLng] = []
  @State private var zoneMinutes: [Double]?

  var body: some View {
    hero
    statsGrid
    if !route.isEmpty { routeMap }
    if let minutes = zoneMinutes {
      VStack(alignment: .leading, spacing: 14) {
        AuraSectionHeader(title: "Time in zones")
        AuraZoneBars(minutes: minutes, hrMax: hrMax)
          .auraDarkCard()
      }
      .task(id: row.startTs) {}
    }
    Color.clear.frame(height: 0).task(id: row.startTs) { await load() }
    Text("Source: \(row.source) · \(Date(timeIntervalSince1970: TimeInterval(row.startTs)).formatted(date: .abbreviated, time: .shortened))–\(Date(timeIntervalSince1970: TimeInterval(row.endTs)).formatted(date: .omitted, time: .shortened))")
      .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.45))
      .padding(.horizontal, 4)
  }

  private var hrMax: Int { profile.hrMaxOverride > 0 ? profile.hrMaxOverride : max(120, 220 - profile.age) }

  private func load() async {
    // Imported per-workout zone split wins; else derive minutes from the
    // strap's raw HR (same precedence the legacy detail used).
    if let pct = WorkoutZones.percents(row.zonesJSON) {
      let durMin = (row.durationS ?? Double(row.endTs - row.startTs)) / 60
      if durMin > 0 { zoneMinutes = pct.map { durMin * $0 / 100 } }
    }
    if zoneMinutes == nil {
      let derived = await repo.workoutZoneMinutes(from: row.startTs, to: row.endTs, age: profile.age)
      if let derived, derived.reduce(0, +) > 0 { zoneMinutes = derived }
    }
    // On-device GPS route, when this session recorded one (#524).
    if let r = RouteStore.load(startTs: row.startTs, sport: row.sport) {
      let pts = RouteMath.decode(r.polyline)
      if pts.count >= 2 { route = pts }
    }
  }

  private var routeMap: some View {
    VStack(alignment: .leading, spacing: 14) {
      AuraSectionHeader(title: "Route")
      Map(interactionModes: []) {
        MapPolyline(coordinates: route.map {
          CLLocationCoordinate2D(latitude: $0.lat, longitude: $0.lon)
        })
        .stroke(AuraDesign.accent, lineWidth: 4)
      }
      .frame(height: 200)
      .clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
      .overlay(RoundedRectangle(cornerRadius: 24, style: .continuous)
        .strokeBorder(AuraDesign.hairline, lineWidth: 1))
      .allowsHitTesting(false)
    }
  }

  private var hero: some View {
    VStack(alignment: .leading, spacing: 18) {
      HStack {
        Text("Activity strain").auraLabel()
        Spacer()
        Text(Date(timeIntervalSince1970: TimeInterval(row.startTs))
               .formatted(.dateTime.weekday(.wide).month().day()))
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.6))
      }
      HStack(alignment: .firstTextBaseline, spacing: 6) {
        Text(AuraEffort.text(row.strain))
          .font(AuraDesign.mega(76)).foregroundStyle(AuraDesign.ink)
          .lineLimit(1).minimumScaleFactor(0.5)
        Spacer()
      }
      AuraSlider(value: (row.strain ?? 0) / 100, glow: AuraDesign.Family.effort.glow)
    }
    .frame(maxWidth: .infinity, minHeight: 190, alignment: .leading)
    .auraGlowTile(.effort, padding: 22, radius: 34)
  }

  private var statsGrid: some View {
    LazyVGrid(columns: [GridItem(.flexible(), spacing: 20), GridItem(.flexible(), spacing: 20)], spacing: 22) {
      AuraMiniStat(value: durText, label: "Duration", level: min(durMin / 120, 1), tint: AuraDesign.Family.effort.glow)
      AuraMiniStat(value: row.avgHr.map { "\($0)" } ?? "--", unit: "bpm", label: "Avg HR",
                   level: Double(row.avgHr ?? 0) / 200, tint: AuraDesign.Family.heart.glow)
      AuraMiniStat(value: row.maxHr.map { "\($0)" } ?? "--", unit: "bpm", label: "Max HR",
                   level: Double(row.maxHr ?? 0) / 200, tint: AuraDesign.Family.heart.glow)
      AuraMiniStat(value: row.energyKcal.map { "\(Int($0.rounded()))" } ?? "--", unit: "kcal", label: "Energy",
                   level: (row.energyKcal ?? 0) / 800, tint: AuraDesign.Family.energy.glow)
      if let d = row.distanceM, d > 0 {
        AuraMiniStat(value: String(format: "%.2f", d / 1000), unit: "km", label: "Distance",
                     level: min(d / 15000, 1), tint: AuraDesign.Family.vitals.glow)
      }
    }
    .auraDarkCard(padding: 20)
  }

  private var durMin: Double { (row.durationS ?? Double(row.endTs - row.startTs)) / 60 }
  private var durText: String {
    let m = Int(durMin.rounded())
    return m >= 60 ? "\(m / 60)h \(m % 60)m" : "\(m)m"
  }
}

extension WorkoutRow: Identifiable {
  public var id: String { "\(startTs)-\(sport)" }
}
