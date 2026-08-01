import SwiftUI

// `Workouts` — the training tab.
//
// The core has no workout source yet, so release stays honestly empty. Debug carries a clearly
// marked fixture so the week, start flow, and complete strength logger can all be reviewed.
//
// Every destination here is *pushed*, not presented. The flow used to be three sheets, which meant
// the live session had two different appearances depending on how you reached it — a modal when
// resumed from the banner, a pushed screen when entered from the start list — with two different
// dismiss gestures and two different chrome treatments. A workout is a place in the app, not an
// interruption of it, so it gets a back button and a navigation title like everywhere else.
// Android reached the same conclusion first; this is iOS catching up rather than a new idea.
//
// Sheets remain correct for settings and the device, which genuinely are interruptions.

/// A pushed destination inside the Workouts tab.
enum MavWorkoutRoute: Hashable {
  case start
  /// The confirm screen for a chosen sport: end condition, zone target, GPS, keep screen on.
  case configure(MavSport)
  case live
  case session(WorkoutRow)
  case strength
}

struct MavWorkoutsView: View {
  @ObservedObject var shell: MavShellState
  @EnvironmentObject private var repo: Repository
  @EnvironmentObject private var model: AppModel
  @State private var workouts: [WorkoutRow] = []
  @State private var selectedDay = 6
  @State private var loadSelection: Int?

  private var weekStart: Date {
    Calendar.current.date(byAdding: .day, value: -6, to: shell.day) ?? shell.day
  }

  var body: some View {
    MavTabScroll {
      weekHero

      if model.activeWorkout != nil {
        NavigationLink(value: MavWorkoutRoute.live) {
          MavLiveSessionBanner()
        }
        .buttonStyle(.plain)
      }

      MavPrimaryLink(
        title: "Start workout",
        systemImage: "play.fill",
        value: MavWorkoutRoute.start)

      if !workouts.isEmpty {
        MavSectionHeader(title: "Weekly activity")
        MavTile {
          VStack(alignment: .leading, spacing: 4) {
            Text("Minutes")
              .mavType(.caption)
              .foregroundStyle(MavTheme.inkSecondary)
            MavWeekStrip(days: weekDays, selected: selectedDay) { selectedDay = $0 }
          }
        }

        if trainingLoadPoints.count > 1 {
          MavSectionHeader(title: "Training load")
          MavTile {
            VStack(alignment: .leading, spacing: 8) {
              HStack(alignment: .firstTextBaseline) {
                Text(selectedLoadValue)
                  .mavType(.numeralMedium)
                  .foregroundStyle(MavTheme.ink)
                Text("effort")
                  .mavType(.sub)
                  .foregroundStyle(MavTheme.inkSecondary)
                Spacer()
                Text(selectedLoadLabel)
                  .mavType(.sub)
                  .foregroundStyle(MavTheme.inkSecondary)
              }
              MavSeriesChart(
                points: trainingLoadPoints,
                band: nil,
                family: .effort,
                accessibilitySummary: trainingLoadSummary,
                selection: $loadSelection)
              HStack {
                Text(trainingLoadPoints.first?.label ?? "")
                Spacer()
                Text(trainingLoadPoints.last?.label ?? "")
              }
              .mavType(.sub)
              .foregroundStyle(MavTheme.inkSecondary)
            }
          }
        }
      }

      MavSectionHeader(title: "Sessions")
      sessions
    }
    .task(id: "\(repo.refreshSeq)-\(model.usingDebugFixture)") {
      let real = await repo.workoutRows()
      #if DEBUG
        workouts = real.isEmpty && model.usingDebugFixture ? MavDebugFixture.workouts() : real
      #else
        workouts = real
      #endif
    }
    .navigationDestination(for: MavWorkoutRoute.self) { route in
      switch route {
      case .start:
        MavWorkoutStartView(shell: shell)
      case .configure(let sport):
        MavWorkoutConfigView(shell: shell, sport: sport)
      case .live:
        MavLiveWorkoutView(shell: shell)
      case .session(let workout):
        MavWorkoutDetailView(workout: workout)
      case .strength:
        MavStrengthView(standalone: false) {
          model.activeWorkout = nil
          shell.workoutPath.removeAll()
        }
      }
    }
  }

  // MARK: Week hero

  private var weekHero: some View {
    ZStack(alignment: .bottomLeading) {
      MavScene(crop: .low)

      VStack(alignment: .leading, spacing: 12) {
        Text("This week")
          .mavType(.caption)
          .foregroundStyle(.white.opacity(0.88))
        Text(weekVerdict)
          .mavType(.title)
          .foregroundStyle(.white)
          .fixedSize(horizontal: false, vertical: true)

        if !workouts.isEmpty {
          HStack(spacing: 26) {
            statBlock("Time", durationLabel(totalMinutes), onPhoto: true)
            statBlock("Sessions", "\(workouts.count)", onPhoto: true)
          }
        }
      }
      .padding(20)
    }
    .frame(minHeight: 148)
    .clipShape(MavTheme.cardShape)
  }

  private func statBlock(_ key: String, _ value: String, onPhoto: Bool = false) -> some View {
    VStack(alignment: .leading, spacing: 4) {
      Text(key)
        .mavType(.caption)
        .foregroundStyle(onPhoto ? .white.opacity(0.82) : MavTheme.inkSecondary)
      Text(value).mavType(.numeralMedium).foregroundStyle(onPhoto ? .white : MavTheme.ink)
    }
    .accessibilityElement(children: .combine)
    .accessibilityLabel("\(key), \(value == "—" ? "no value" : value)")
  }

  private var weekVerdict: String {
    workouts.isEmpty
      ? "No sessions yet"
      : "\(workouts.count) session\(workouts.count == 1 ? "" : "s") this week"
  }

  private var totalMinutes: Double {
    workouts.reduce(0) { $0 + ($1.durationS ?? Double($1.endTs - $1.startTs)) / 60 }
  }

  private func durationLabel(_ minutes: Double) -> String {
    let total = Int(minutes.rounded())
    return total >= 60 ? "\(total / 60)h \(total % 60)m" : "\(total)m"
  }

  // MARK: Week strip

  private var weekDays: [MavWeekStrip.Day] {
    let calendar = Calendar.current
    let letters = ["S", "M", "T", "W", "T", "F", "S"]
    return (0..<7).compactMap { offset in
      guard let date = calendar.date(byAdding: .day, value: offset, to: weekStart) else { return nil }
      let weekday = calendar.component(.weekday, from: date) - 1
      let key = Repository.localDayKey(date)
      let minutes = workouts
        .filter { Repository.localDayKey(Date(timeIntervalSince1970: TimeInterval($0.startTs))) == key }
        .reduce(0.0) { $0 + ($1.durationS ?? Double($1.endTs - $1.startTs)) / 60 }
      let peak = max(
        workouts.map { ($0.durationS ?? Double($0.endTs - $0.startTs)) / 60 }.max() ?? 1, 1)
      return MavWeekStrip.Day(
        letter: letters[weekday],
        full: key,
        fraction: min(minutes / peak, 1),
        minutes: Int(minutes.rounded()),
        summary: minutes > 0
          ? "\(date.formatted(.dateTime.weekday(.wide))), \(Int(minutes)) minutes"
          : "\(date.formatted(.dateTime.weekday(.wide))), nothing recorded")
    }
  }

  // MARK: Sessions

  private var selectedWorkouts: [WorkoutRow] {
    guard weekDays.indices.contains(selectedDay) else { return workouts }
    let key = weekDays[selectedDay].full
    return workouts.filter {
      Repository.localDayKey(Date(timeIntervalSince1970: TimeInterval($0.startTs))) == key
    }
  }

  @ViewBuilder private var sessions: some View {
    if workouts.isEmpty {
      VStack(alignment: .leading, spacing: 7) {
        Text("Nothing here yet").mavType(.title).foregroundStyle(MavTheme.ink)
        Text("Start a workout above, or connect a source that provides sessions.")
          .mavType(.body)
          .foregroundStyle(MavTheme.inkSecondary)
      }
      .padding(.vertical, 8)
    } else if selectedWorkouts.isEmpty {
      Text("No sessions on this day.")
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)
        .padding(.vertical, 8)
    } else {
      VStack(spacing: 0) {
        ForEach(Array(selectedWorkouts.enumerated()), id: \.offset) { index, workout in
          if index > 0 { MavDivider() }
          NavigationLink(value: MavWorkoutRoute.session(workout)) {
            MavWorkoutRow(workout: workout)
          }
          .buttonStyle(.plain)
          .accessibilityHint("Opens workout details")
        }
      }
      .mavSurface(MavTheme.tileShape)
    }
  }

  private var trainingLoadPoints: [MavSeriesChart.Point] {
    workouts
      .filter { $0.strain != nil }
      .sorted { $0.startTs < $1.startTs }
      .suffix(10)
      .map { workout in
        MavSeriesChart.Point(
          label: Date(timeIntervalSince1970: TimeInterval(workout.startTs))
            .formatted(.dateTime.day().month(.abbreviated)),
          value: workout.strain ?? 0)
      }
  }

  private var selectedLoadPoint: MavSeriesChart.Point? {
    guard !trainingLoadPoints.isEmpty else { return nil }
    return loadSelection.flatMap {
      trainingLoadPoints.indices.contains($0) ? trainingLoadPoints[$0] : nil
    } ?? trainingLoadPoints.last
  }

  private var selectedLoadValue: String {
    selectedLoadPoint.map { AuraEffortText.text($0.value) } ?? "—"
  }

  private var selectedLoadLabel: String { selectedLoadPoint?.label ?? "" }

  private var trainingLoadSummary: String {
    "Effort over \(trainingLoadPoints.count) workouts, from "
      + "\(trainingLoadPoints.map(\.value).min().map(AuraEffortText.text) ?? "—") to "
      + "\(trainingLoadPoints.map(\.value).max().map(AuraEffortText.text) ?? "—")."
  }
}

// MARK: - Workout flows

/// The sport catalogue. A pushed screen, so the back button returns to the tab and the tab bar
/// stays where it was rather than being covered by a card.
struct MavWorkoutStartView: View {
  @ObservedObject var shell: MavShellState

  var body: some View {
    MavDetailScaffold(title: "Start workout") {
      Text("Choose an activity. The next screen sets how it ends.")
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)

      ForEach(MavSportCatalog.categories) { category in
        MavSectionHeader(title: category.title)
        VStack(spacing: 0) {
          ForEach(Array(category.sports.enumerated()), id: \.offset) { index, sport in
            if index > 0 { MavDivider() }
            // Strength is reached here, inside the sport catalogue, rather than from a button of
            // its own on the tab. It is one activity among many from the wearer's point of view,
            // and promoting it to the tab root made the tab look like two apps. It also skips the
            // confirm screen entirely: a lifting session has no end condition and no zone target,
            // so there would be nothing on it to decide.
            NavigationLink(value: sport.isStrength ? MavWorkoutRoute.strength : .configure(sport)) {
              startRowLabel(
                title: sport.name, detail: sport.detail, systemImage: sport.systemImage)
            }
            .buttonStyle(.plain)
            .accessibilityHint(
              sport.isStrength ? "Opens strength routines" : "Opens the settings for this workout")
          }
        }
        .mavSurface(MavTheme.tileShape)
      }
    }
  }

  private func startRowLabel(title: String, detail: String, systemImage: String) -> some View {
    HStack(spacing: 14) {
      Image(systemName: systemImage)
        .font(.system(size: 15, weight: .medium))
        .foregroundStyle(MavTheme.ink)
        .frame(width: 30, height: 30)
        .mavSurface(MavTheme.chipShape)
      VStack(alignment: .leading, spacing: 3) {
        Text(title).mavType(.label).foregroundStyle(MavTheme.ink)
        Text(detail).mavType(.sub).foregroundStyle(MavTheme.inkSecondary)
      }
      Spacer(minLength: 8)
      Image(systemName: "chevron.right")
        .font(.system(size: 12, weight: .semibold))
        .foregroundStyle(MavTheme.inkSecondary)
    }
    .padding(.horizontal, MavTheme.tilePadding)
    .padding(.vertical, 14)
    .contentShape(.rect)
  }
}

/// The running session. A pushed screen, reached the same way whether you started the workout just
/// now or came back to it from the banner — there is one live screen and it looks like itself.
struct MavLiveWorkoutView: View {
  @ObservedObject var shell: MavShellState
  @EnvironmentObject private var model: AppModel
  @EnvironmentObject private var live: LiveState
  @State private var confirmStop = false

  /// The configuration the confirm screen persisted for this sport a moment ago. Read rather than
  /// carried on the session, so a relaunch mid-workout recovers the same goal.
  private var config: WorkoutConfig {
    WorkoutPrefs.config(for: model.activeWorkout?.sport ?? "")
  }

  var body: some View {
    MavDetailScaffold(title: model.activeWorkout?.sport ?? "Live workout") {
      MavTile {
        VStack(alignment: .leading, spacing: 10) {
          Text("Elapsed")
            .mavType(.caption)
            .foregroundStyle(MavTheme.inkSecondary)
          TimelineView(.periodic(from: .now, by: 1)) { _ in
            Text(elapsed)
              .mavType(.numeralXL)
              .monospacedDigit()
              .foregroundStyle(MavTheme.ink)
              .contentTransition(.numericText())
              .accessibilityLabel("Elapsed time, \(spokenElapsed)")
          }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
      }

      if config.goal.isActive {
        MavSectionHeader(title: "Goal")
        goalCard
      }

      MavSectionHeader(title: "Now")
      LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 12) {
        liveStat("Heart rate", live.heartRate.map { "\($0)" } ?? "—", unit: "bpm")
        liveStat("Effort", AuraEffortText.text(model.activeWorkout?.liveStrain), unit: nil)
      }

      // A strap that is not streaming is stated rather than shown as a dash and left ambiguous.
      if !live.connected {
        Text(
          "No strap is streaming, so heart rate and effort stay empty. The elapsed clock keeps "
            + "running and the session still records.")
          .mavType(.sub)
          .foregroundStyle(MavTheme.inkSecondary)
          .fixedSize(horizontal: false, vertical: true)
          .padding(.top, 2)
      }

      MavWideButton(title: "End workout", destructive: true) { confirmStop = true }
        .padding(.top, 6)
    }
    .confirmationDialog("End this workout?", isPresented: $confirmStop) {
      Button("End workout", role: .destructive) {
        model.activeWorkout = nil
        shell.workoutPath.removeAll()
      }
    }
  }

  /// Progress toward the end condition.
  ///
  /// Only a time goal can be answered today: nothing records distance or energy live yet, and a
  /// progress bar sitting at zero for an hour is worse than saying so. When the source arrives this
  /// branch collapses — `MavMilestones.progress` already handles all three kinds.
  @ViewBuilder private var goalCard: some View {
    MavTile {
      VStack(alignment: .leading, spacing: 10) {
        HStack(alignment: .firstTextBaseline) {
          Text(goalHeadline).mavType(.label).foregroundStyle(MavTheme.ink)
          Spacer()
          if let fraction = timeProgress {
            Text("\(Int((fraction * 100).rounded()))%")
              .mavType(.sub)
              .monospacedDigit()
              .foregroundStyle(MavTheme.inkSecondary)
          }
        }

        if let fraction = timeProgress {
          GeometryReader { proxy in
            ZStack(alignment: .leading) {
              Capsule().fill(MavTheme.hairline)
              Capsule()
                .fill(MavFamily.effort.hue)
                .frame(width: proxy.size.width * fraction)
            }
          }
          .frame(height: 8)
          .accessibilityElement()
          .accessibilityLabel(
            "\(goalHeadline), \(Int((fraction * 100).rounded())) percent complete")
        } else {
          Text(goalUnavailableReason)
            .mavType(.sub)
            .foregroundStyle(MavTheme.inkSecondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)
    }
  }

  private var goalHeadline: String {
    let goal = config.goal
    switch goal.kind {
    case .none: return ""
    case .time: return "\(Int(goal.value)) min"
    case .distance: return "\(MavGoalText.display(goal, isImperial: false)) km"
    case .calories: return "\(Int(goal.value)) kcal"
    }
  }

  private var timeProgress: Double? {
    guard config.goal.kind == .time else { return nil }
    return MavMilestones.progress(
      config.goal, elapsedSec: elapsedSeconds, distanceM: 0, kcal: 0)
  }

  private var goalUnavailableReason: String {
    switch config.goal.kind {
    case .distance: "No source is recording distance yet, so this goal cannot be tracked live."
    case .calories: "No source is recording energy yet, so this goal cannot be tracked live."
    default: ""
    }
  }

  private func liveStat(_ label: String, _ value: String, unit: String?) -> some View {
    MavTile {
      VStack(alignment: .leading, spacing: 5) {
        Text(label).mavType(.caption).foregroundStyle(MavTheme.inkSecondary)
        HStack(alignment: .firstTextBaseline, spacing: 3) {
          Text(value).mavType(.numeralMedium).foregroundStyle(MavTheme.ink).monospacedDigit()
          if let unit {
            Text(unit).mavType(.sub).foregroundStyle(MavTheme.inkSecondary)
          }
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      .accessibilityElement(children: .combine)
      .accessibilityLabel("\(label), \(value == "—" ? "no value" : value)\(unit.map { " \($0)" } ?? "")")
    }
  }

  private var elapsedSeconds: Int {
    guard let start = model.activeWorkout?.startMs else { return 0 }
    return max(0, Int(Date().timeIntervalSince1970) - start / 1_000)
  }

  private var elapsed: String { MavElapsed.format(elapsedSeconds) }
  private var spokenElapsed: String { MavElapsed.spoken(elapsedSeconds) }
}

/// The live session's clock, as pure functions so a test can assert the boundaries without
/// rendering anything.
///
/// Both platforms formatted elapsed time as `mm:ss` with no hour rollover, so a ninety-minute
/// session read "90:00". The Kotlin twin is `formatElapsed` / `spokenElapsed` in `MavScreens.kt`.
enum MavElapsed {

  /// Hours appear once there are any.
  static func format(_ seconds: Int) -> String {
    seconds >= 3_600
      ? String(format: "%d:%02d:%02d", seconds / 3_600, (seconds % 3_600) / 60, seconds % 60)
      : String(format: "%02d:%02d", seconds / 60, seconds % 60)
  }

  /// VoiceOver reads a duration, not a punctuation pattern.
  static func spoken(_ seconds: Int) -> String {
    let hours = seconds / 3_600
    let minutes = (seconds % 3_600) / 60
    var parts: [String] = []
    if hours > 0 { parts.append("\(hours) hour\(hours == 1 ? "" : "s")") }
    if minutes > 0 { parts.append("\(minutes) minute\(minutes == 1 ? "" : "s")") }
    parts.append("\(seconds % 60) second\(seconds % 60 == 1 ? "" : "s")")
    return parts.joined(separator: " ")
  }
}

struct MavWorkoutRow: View {
  let workout: WorkoutRow

  private var start: Date { Date(timeIntervalSince1970: TimeInterval(workout.startTs)) }
  private var minutes: Int {
    Int(((workout.durationS ?? Double(workout.endTs - workout.startTs)) / 60).rounded())
  }

  var body: some View {
    HStack(spacing: 14) {
      Image(systemName: "figure.run")
        .font(.system(size: 13))
        .foregroundStyle(MavFamily.effort.hue)
        .frame(width: 30, height: 30)
        .mavSurface(MavTheme.chipShape)

      VStack(alignment: .leading, spacing: 3) {
        Text(workout.sport).mavType(.title).foregroundStyle(MavTheme.ink)
        Text(
          "\(start.formatted(date: .omitted, time: .shortened)) · \(minutes) min"
          + (workout.avgHr.map { " · \($0) avg bpm" } ?? "")
        )
        .mavType(.sub)
        .foregroundStyle(MavTheme.inkSecondary)
      }

      Spacer(minLength: 8)

      if let strain = workout.strain {
        VStack(alignment: .trailing, spacing: 2) {
          Text(AuraEffortText.text(strain))
            .mavType(.numeralSmall)
            .foregroundStyle(MavTheme.ink)
          Text("effort").mavType(.sub).foregroundStyle(MavTheme.inkSecondary)
        }
      }
    }
    .padding(.horizontal, MavTheme.tilePadding)
    .padding(.vertical, 15)
    .accessibilityElement(children: .combine)
  }
}

struct MavWorkoutDetailView: View {
  let workout: WorkoutRow

  private var durationMinutes: Int {
    Int(((workout.durationS ?? Double(workout.endTs - workout.startTs)) / 60).rounded())
  }

  var body: some View {
    MavDetailScaffold(title: workout.sport) {
      MavTile {
        VStack(alignment: .leading, spacing: 12) {
          Text(
            Date(timeIntervalSince1970: TimeInterval(workout.startTs))
              .formatted(.dateTime.weekday(.wide).day().month(.wide)))
            .mavType(.caption)
            .foregroundStyle(MavTheme.inkSecondary)
          HStack(alignment: .firstTextBaseline) {
            Text(workout.sport).mavType(.display).foregroundStyle(MavTheme.ink)
            Spacer()
            if let strain = workout.strain {
              Text(AuraEffortText.text(strain))
                .mavType(.numeralLarge)
                .foregroundStyle(MavTheme.ink)
            }
          }
        }
      }

      MavSectionHeader(title: "Summary")
      LazyVGrid(
        columns: [GridItem(.flexible()), GridItem(.flexible())],
        spacing: 12
      ) {
        detailStat("Duration", "\(durationMinutes) min")
        detailStat("Average HR", workout.avgHr.map { "\($0) bpm" } ?? "—")
        detailStat("Maximum HR", workout.maxHr.map { "\($0) bpm" } ?? "—")
        detailStat(
          "Energy",
          workout.energyKcal.map { "\(Int($0.rounded())) kcal" } ?? "—")
        if let distance = workout.distanceM, distance > 0 {
          detailStat("Distance", String(format: "%.2f km", distance / 1_000))
        }
      }

      if let zones = WorkoutZones.percents(workout.zonesJSON) {
        MavSectionHeader(title: "Heart-rate distribution")
        MavTile {
          MavWorkoutZoneDistribution(percentages: zones)
        }
      }

      Text(
        "Source: \(workout.source) · "
          + Date(timeIntervalSince1970: TimeInterval(workout.startTs))
          .formatted(date: .abbreviated, time: .shortened))
        .mavType(.sub)
        .foregroundStyle(MavTheme.inkSecondary)
        .padding(.vertical, 10)
    }
  }

  private func detailStat(_ label: String, _ value: String) -> some View {
    MavTile {
      VStack(alignment: .leading, spacing: 5) {
        Text(label).mavType(.caption).foregroundStyle(MavTheme.inkSecondary)
        Text(value).mavType(.numeralSmall).foregroundStyle(MavTheme.ink)
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      .accessibilityElement(children: .combine)
    }
  }
}

private struct MavWorkoutZoneDistribution: View {
  let percentages: [Double]
  private let names = ["Easy", "Base", "Tempo", "Threshold", "Maximum"]

  var body: some View {
    VStack(spacing: 14) {
      ForEach(Array(percentages.enumerated()).reversed(), id: \.offset) { index, value in
        HStack(spacing: 12) {
          Text("Z\(index + 1)")
            .mavType(.label)
            .frame(width: 28, alignment: .leading)
          Text(names[index])
            .mavType(.sub)
            .foregroundStyle(MavTheme.inkSecondary)
            .frame(width: 72, alignment: .leading)
          GeometryReader { proxy in
            ZStack(alignment: .leading) {
              Capsule().fill(MavTheme.hairline)
              Capsule()
                .fill(MavTheme.ink.opacity(0.42 + Double(index) * 0.1))
                .frame(width: proxy.size.width * min(max(value / 100, 0), 1))
            }
          }
          .frame(height: 8)
          Text("\(Int(value.rounded()))%")
            .mavType(.sub)
            .monospacedDigit()
            .frame(width: 38, alignment: .trailing)
        }
        .accessibilityElement(children: .combine)
      }
    }
  }
}

/// A compact route back into the active session. Biometric readings stay under Vitals.
struct MavLiveSessionBanner: View {
  var body: some View {
    MavStatusCard(family: .effort, shape: AnyShape(MavTheme.tileShape)) {
      HStack(spacing: 12) {
        Circle()
          .fill(MavTheme.accent)
          .frame(width: 7, height: 7)
        VStack(alignment: .leading, spacing: 3) {
          Text("Session running").mavType(.label).foregroundStyle(MavTheme.ink)
          Text("View elapsed time and session controls")
            .mavType(.sub)
            .foregroundStyle(MavTheme.inkSecondary)
        }
        Spacer(minLength: 0)
        Image(systemName: "chevron.right")
          .font(.system(size: 13, weight: .semibold))
          .foregroundStyle(MavTheme.inkSecondary)
      }
    }
    .accessibilityElement(children: .combine)
  }
}

/// Effort is stored 0–100; WHOOP's 0–21 is a display axis the wearer chooses.
enum AuraEffortText {
  static func text(_ stored: Double?) -> String {
    guard let stored else { return "—" }
    let factor = UnitPrefs.currentEffortDisplayFactor()
    let value = stored * factor
    return factor == 1.0
      ? String(Int(value.rounded()))
      : String(format: "%.1f", value)
  }
}

// MARK: - Confirm screen

/// What happens before a cardio session starts: the end condition, an optional zone target, and
/// the two per-session options.
///
/// This is the screen the rewrite lost. Without it every workout was a free workout — the strap
/// recorded until you remembered to stop it — and the entire milestone vocabulary had nothing to
/// fire against.
///
/// Settings are **sticky per sport**, which is the whole template system: the last configuration
/// used for a run is the next one offered for a run. A separate "save as template" step is a step
/// nobody takes.
struct MavWorkoutConfigView: View {
  @ObservedObject var shell: MavShellState
  let sport: MavSport
  @EnvironmentObject private var model: AppModel
  @EnvironmentObject private var live: LiveState

  @State private var config = WorkoutConfig()
  @State private var goalText = ""
  @State private var zoneOn = false
  @State private var loaded = false

  /// Nothing declares haptics yet (ADR-032), so the buzz hints below describe what *would* happen
  /// and the screen says plainly that it will not. Promising a wrist tap the strap cannot deliver
  /// is the failure this check exists to prevent.
  private var haptics: MavHapticSupport { .none }

  /// Read the same way every other screen reads it, so changing units in settings updates this
  /// screen without it having to be told.
  @AppStorage(UnitPrefs.systemKey) private var systemRaw = UnitSystem.metric.rawValue
  private var system: UnitSystem { UnitSystem(rawValue: systemRaw) ?? .metric }
  private var isImperial: Bool { system == .imperial }
  private var distanceUnit: String { UnitFormatter.distanceUnit(system) }

  var body: some View {
    MavDetailScaffold(title: sport.name) {
      endCondition
      zoneTarget
      options
      startButton
    }
    .onAppear {
      guard !loaded else { return }
      loaded = true
      config = WorkoutPrefs.config(for: sport.name)
      zoneOn = config.zoneTarget != nil
      goalText = goalDisplayText(config.goal)
    }
  }

  // MARK: End condition

  private var availableKinds: [WorkoutGoalKind] {
    // Distance is only offered where a distance means something. A "5 km" yoga session is not a
    // goal, it is a confused control.
    WorkoutGoalKind.allCases.filter { $0 != .distance || sport.isDistance }
  }

  private var endCondition: some View {
    MavTile {
      VStack(alignment: .leading, spacing: 14) {
        Text("End condition").mavType(.caption).foregroundStyle(MavTheme.inkSecondary)

        HStack(spacing: 6) {
          ForEach(availableKinds) { kind in
            kindChip(kind)
          }
        }

        if config.goal.kind != .none {
          HStack(spacing: 10) {
            TextField("0", text: $goalText)
              .keyboardType(.decimalPad)
              .multilineTextAlignment(.center)
              .mavType(.numeralSmall)
              .monospacedDigit()
              .foregroundStyle(MavTheme.ink)
              .padding(.vertical, 9)
              .frame(width: 96)
              .mavSurface(MavTheme.chipShape)
              .onChange(of: goalText) { _, new in
                let parsed = Double(new.replacingOccurrences(of: ",", with: ".")) ?? 0
                // Stored natively — km, minutes, kcal — so the comparison in MavMilestones never
                // sees a display unit.
                config.goal.value =
                  config.goal.kind == .distance && isImperial
                  ? parsed / UnitFormatter.milesPerKilometer
                  : parsed
              }
              .accessibilityLabel("\(config.goal.kind.label) goal in \(goalUnit)")

            Text(goalUnit).mavType(.label).foregroundStyle(MavTheme.inkSecondary)
            Spacer()
          }

          Text(interimHint)
            .mavType(.sub)
            .foregroundStyle(MavTheme.inkSecondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
    }
  }

  private func kindChip(_ kind: WorkoutGoalKind) -> some View {
    let active = config.goal.kind == kind
    return Button {
      withAnimation(MavTheme.calm) {
        config.goal.kind = kind
        config.goal.value = defaultValue(kind)
        goalText = goalDisplayText(config.goal)
      }
    } label: {
      Text(kind.label)
        .mavType(.caption)
        .foregroundStyle(active ? MavTheme.onAccent : MavTheme.inkSecondary)
        .padding(.vertical, 9)
        .frame(maxWidth: .infinity)
        .background(active ? MavTheme.accent : MavTheme.hairline, in: MavTheme.pillShape)
        .contentShape(MavTheme.pillShape)
    }
    .buttonStyle(.plain)
    .accessibilityAddTraits(active ? [.isSelected, .isButton] : .isButton)
  }

  private func defaultValue(_ kind: WorkoutGoalKind) -> Double {
    MavGoalText.defaultValue(kind, isImperial: isImperial)
  }

  private func goalDisplayText(_ goal: WorkoutGoal) -> String {
    MavGoalText.display(goal, isImperial: isImperial)
  }

  private var goalUnit: String {
    MavGoalText.unit(config.goal.kind, distanceUnit: distanceUnit)
  }

  /// What the strap will do, stated where the decision is made rather than buried in settings.
  private var interimHint: String {
    guard haptics.supports(.goalComplete) else {
      return haptics.reason(deviceName: live.advertisingName)
        + " The goal still tracks on screen."
    }
    switch config.goal.kind {
    case .none:
      return ""
    case .distance:
      return "A light tap every \(distanceUnit == "mi" ? "mile" : "kilometre"), "
        + "and a strong buzz at the goal."
    case .time:
      return "A light tap \(WorkoutPrefs.timeMode() == .halfway ? "at halfway" : "on the interval"), "
        + "and a strong buzz at the goal."
    case .calories:
      return "A light tap "
        + "\(WorkoutPrefs.calorieMode() == .halfway ? "at halfway" : "on the interval"), "
        + "and a strong buzz at the goal."
    }
  }

  // MARK: Zone target

  private var zoneTarget: some View {
    MavTile {
      VStack(alignment: .leading, spacing: 14) {
        Toggle(isOn: $zoneOn.animation(MavTheme.calm)) {
          Text("Zone target").mavType(.label).foregroundStyle(MavTheme.ink)
        }
        .tint(MavTheme.accent)
        .onChange(of: zoneOn) { _, on in
          config.zoneTarget = on ? (config.zoneTarget ?? WorkoutZoneTarget(zone: 2, minutes: 20)) : nil
        }

        if zoneOn, let target = config.zoneTarget {
          HStack(spacing: 6) {
            ForEach(1...5, id: \.self) { zone in
              zoneChip(zone, selected: target.zone)
            }
          }

          HStack(spacing: 10) {
            Text("for").mavType(.sub).foregroundStyle(MavTheme.inkSecondary)
            Picker("Minutes in zone", selection: minutesBinding) {
              ForEach([10, 15, 20, 30, 45, 60], id: \.self) { Text("\($0) min").tag($0) }
            }
            .pickerStyle(.menu)
            .tint(MavTheme.ink)
            Spacer()
          }

          Text("The zone bars track it live, and it is banked once the time is in.")
            .mavType(.sub)
            .foregroundStyle(MavTheme.inkSecondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
    }
  }

  private var minutesBinding: Binding<Int> {
    Binding(
      get: { config.zoneTarget?.minutes ?? 20 },
      set: { config.zoneTarget?.minutes = $0 })
  }

  private func zoneChip(_ zone: Int, selected: Int) -> some View {
    let active = zone == selected
    return Button {
      config.zoneTarget?.zone = zone
    } label: {
      Text("Z\(zone)")
        .mavType(.caption)
        .monospacedDigit()
        .foregroundStyle(active ? MavTheme.onAccent : MavTheme.inkSecondary)
        .padding(.vertical, 9)
        .frame(maxWidth: .infinity)
        .background(active ? MavTheme.accent : MavTheme.hairline, in: MavTheme.pillShape)
        .contentShape(MavTheme.pillShape)
    }
    .buttonStyle(.plain)
    .accessibilityLabel("Zone \(zone)")
    .accessibilityAddTraits(active ? [.isSelected, .isButton] : .isButton)
  }

  // MARK: Options

  private var options: some View {
    VStack(spacing: 0) {
      if sport.isDistance {
        toggleRow(
          "GPS route", "Distance, pace and the route map",
          isOn: Binding(
            get: { config.gpsEnabled ?? true },
            set: { config.gpsEnabled = $0 }))
        MavDivider()
      }
      toggleRow("Keep screen on", "No auto-lock while the session runs", isOn: $config.keepScreenOn)
    }
    .mavSurface(MavTheme.tileShape)
  }

  private func toggleRow(_ title: String, _ detail: String, isOn: Binding<Bool>) -> some View {
    Toggle(isOn: isOn) {
      VStack(alignment: .leading, spacing: 3) {
        Text(title).mavType(.label).foregroundStyle(MavTheme.ink)
        Text(detail).mavType(.sub).foregroundStyle(MavTheme.inkSecondary)
      }
    }
    .tint(MavTheme.accent)
    .padding(.horizontal, MavTheme.tilePadding)
    .padding(.vertical, 13)
  }

  // MARK: Start

  private var startButton: some View {
    MavPrimaryButton(title: "Start \(sport.name.lowercased())", systemImage: "play.fill") {
      var resolved = config
      if !resolved.goal.isActive { resolved.goal = .none }
      if !zoneOn { resolved.zoneTarget = nil }
      // Persist before starting, so the settings are sticky even if the session is abandoned.
      WorkoutPrefs.save(resolved, for: sport.name)

      model.activeWorkout = ActiveWorkout(
        startMs: Int(Date().timeIntervalSince1970 * 1_000), sport: sport.name)
      shell.workoutPath = [.live]
    }
    .padding(.top, 4)
  }
}
