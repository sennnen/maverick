import SwiftUI

// `Today` — the narrative tab. It opens with the score rail, because "give me the numbers" deserves
// half a second, and everything under it is prose.
//
// There is deliberately no live heart rate here. It belongs inside the Heart Rate vital; it was on
// the old Today because the old shell had nowhere else to put it, which is not a reason.

struct MavTodayView: View {
  @ObservedObject var shell: MavShellState
  @EnvironmentObject private var model: AppModel
  @EnvironmentObject private var profile: ProfileStore
  @EnvironmentObject private var repo: Repository
  @EnvironmentObject private var analytics: MavAnalyticsModel
  @State private var workouts: [WorkoutRow] = []

  private let narrative: MavNarrativeProviding = MavStubNarrativeProvider()

  private var rows: [MavMetricRow] {
    MavMetricMapper.rows(
      from: model.dailySnapshot,
      cycleEnabled: profile.tracksCycle || model.usingDebugFixture)
  }

  var body: some View {
    MavTabScroll {
      MavScoreRail(items: MavRailItem.rail(from: rows))

      MavNarrativeHero(state: narrative.daily(day: shell.dayKey, rows: rows))

      MavSectionHeader(title: "Your day")
      MavDayTimeline(
        snapshot: model.dailySnapshot,
        syncProgress: model.syncProgress,
        workouts: todaysWorkouts,
        usingFixture: model.usingDebugFixture)

      MavSectionHeader(title: "Discoveries")
      VStack(spacing: 0) {
        MavTrendLine(
          title: "Resilience",
          window: "3 months",
          family: .charge,
          state: narrative.trend(id: "resilience", rows: rows))
        MavDivider()
        MavTrendLine(
          title: "Training load",
          window: "8 weeks",
          family: .heart,
          state: narrative.trend(id: "cardio_load", rows: rows))
      }
      .padding(.horizontal, MavTheme.tilePadding)
      .mavSurface(MavTheme.tileShape)

      MavSectionHeader(title: "More")
      VStack(spacing: 0) {
        NavigationLink {
          MavReportsView(day: shell.dayKey)
        } label: {
          MavRow(title: "Weekly report", detail: "Every day the core has scored") {
            Image(systemName: "chevron.right")
              .font(.system(size: 13, weight: .semibold))
              .foregroundStyle(MavTheme.inkSecondary)
          }
        }
        .buttonStyle(.plain)
        // Only once an engine exists. Before that the screen can only say "nothing has run",
        // which is a link to an apology rather than to a surface.
        if analytics.isAttached {
          MavDivider()
          NavigationLink {
            MavAnalyticsView()
          } label: {
            MavRow(
              title: NSLocalizedString("analytics.title", comment: ""),
              // The row carries the live coverage, so it says something about this wearer's
              // phone rather than only naming a destination.
              detail: MavSignalCopy.rowDetail(analytics.snapshot)
            ) {
              Image(systemName: "chevron.right")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(MavTheme.inkSecondary)
            }
          }
          .buttonStyle(.plain)
        }
        MavDivider()
        NavigationLink {
          MavDiagnosticsView()
        } label: {
          MavRow(title: "Diagnostics", detail: "What the core recorded, and any errors") {
            Image(systemName: "chevron.right")
              .font(.system(size: 13, weight: .semibold))
              .foregroundStyle(MavTheme.inkSecondary)
          }
        }
        .buttonStyle(.plain)
      }
      .mavSurface(MavTheme.tileShape)
    }
    .task(id: model.usingDebugFixture) {
      let real = await repo.workoutRows()
      #if DEBUG
        workouts = real.isEmpty && model.usingDebugFixture ? MavDebugFixture.workouts() : real
      #else
        workouts = real
      #endif
    }
  }

  private var todaysWorkouts: [WorkoutRow] {
    workouts.filter {
      Repository.localDayKey(Date(timeIntervalSince1970: TimeInterval($0.startTs))) == shell.dayKey
    }
  }
}

// MARK: - Score rail

/// A horizontally scrolling row of open-bottom arcs. An unavailable score is a dashed arc and an em
/// dash — never a zero, because a zero is a claim about the day.
struct MavScoreRail: View {
  let items: [MavRailItem]

  var body: some View {
    ScrollView(.horizontal) {
      HStack(spacing: MavTheme.railGap) {
        ForEach(items) { item in
          NavigationLink {
            MavMetricDestination(metric: item.metric)
          } label: {
            MavArcGauge(
              text: item.text,
              label: item.metric.shortName,
              fraction: item.fraction,
              family: item.metric.family,
              accessibilitySummary: summary(item))
              .padding(.vertical, 4)
          }
          .buttonStyle(.plain)
        }
      }
      .padding(.horizontal, MavTheme.screenMargin)
      .padding(.trailing, 42)
      .padding(.vertical, 2)
    }
    .scrollIndicators(.hidden)
    .padding(.horizontal, -MavTheme.screenMargin)
  }

  private func summary(_ item: MavRailItem) -> String {
    item.fraction == nil
      ? "\(item.metric.name), no value today"
      : "\(item.metric.name), \(item.text)\(item.metric.unit.map { " \($0)" } ?? "")"
  }
}

// MARK: - The hero

struct MavNarrativeHero: View {
  let state: MavNarrativeState

  var body: some View {
    ZStack(alignment: .bottomLeading) {
      MavScene(crop: .high)

      VStack(alignment: .leading, spacing: 9) {
        Text("Daily insight")
          .mavType(.caption)
          .foregroundStyle(.white.opacity(0.88))
        switch state {
        case .generated(let headline, let body), .sample(let headline, let body):
          Text(headline)
            .mavType(.display)
            .foregroundStyle(.white)
            .lineLimit(2)
          if !body.isEmpty {
            Text(body)
              .mavType(.body)
              .foregroundStyle(.white.opacity(0.9))
              .lineLimit(2)
          }
        case .unavailable(let reason):
          Text("Nothing written yet")
            .mavType(.display)
            .foregroundStyle(.white)
          Text(reason)
            .mavType(.body)
            .foregroundStyle(.white.opacity(0.9))
            .lineLimit(2)
        }
      }
      .padding(18)

      if state.isSample {
        MavBadge(text: "Preview")
          .padding(12)
          .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
      }
    }
    .frame(minHeight: 190)
    .clipShape(MavTheme.cardShape)
  }
}

// MARK: - Trend card

struct MavTrendLine: View {
  let title: String
  let window: String
  let family: MavFamily
  let state: MavNarrativeState

  var body: some View {
    VStack(alignment: .leading, spacing: 7) {
      HStack(spacing: 9) {
        Image(systemName: family == .heart ? "heart" : "bolt.heart")
          .font(.system(size: 12, weight: .medium))
          .foregroundStyle(family.hue)
          .frame(width: 24, height: 24)
          .mavSurface(MavTheme.chipShape)
        Text(title).mavType(.label).foregroundStyle(MavTheme.ink)
        Spacer()
        Text(window).mavType(.caption).foregroundStyle(MavTheme.inkSecondary)
      }
      Text(state.headline ?? "Not enough history yet")
        .mavType(.body)
        .foregroundStyle(MavTheme.ink)
        .fixedSize(horizontal: false, vertical: true)
    }
    .padding(.vertical, 15)
    .accessibilityElement(children: .combine)
  }
}

// MARK: - Timeline

/// What actually happened, in the order it happened. Every row is something the core recorded; an
/// empty day says so rather than showing an invented morning.
struct MavDayTimeline: View {
  let snapshot: DailySnapshotReport?
  let syncProgress: String?
  let workouts: [WorkoutRow]
  let usingFixture: Bool

  private struct Entry: Identifiable {
    let title: String
    let detail: String
    let period: String
    let family: MavFamily
    var id: String { period + title + detail }
  }

  private var entries: [Entry] {
    var entries: [Entry] = []
    if usingFixture {
      entries.append(
        Entry(title: "Sleep", detail: "7h 42m · 91% efficiency", period: "06:42", family: .rest))
      entries.append(
        Entry(
          title: "Recovery", detail: "82 · overnight signals stayed in range",
          period: "07:05", family: .charge))
    } else if let snapshot {
      if let hrv = snapshot.hrv {
        entries.append(
          Entry(
            title: "Overnight variability",
            detail:
              "\(MavMetricMapper.decimal(hrv.rmssdMs, places: 0)) ms · "
              + "\(hrv.intervalCount) intervals",
            period: "Overnight",
            family: .vitals))
      }
      if snapshot.hrSampleCount > 0 {
        entries.append(
          Entry(
            title: "Heart rate",
            detail:
              "\(snapshot.hrSampleCount) readings recorded"
              + (snapshot.hrExcludedCount > 0
                ? " · \(snapshot.hrExcludedCount) omitted after quality checks" : ""),
            period: "All day",
            family: .heart))
      }
    }
    for workout in workouts.sorted(by: { $0.startTs < $1.startTs }) {
      let start = Date(timeIntervalSince1970: TimeInterval(workout.startTs))
      let minutes = Int(
        ((workout.durationS ?? Double(workout.endTs - workout.startTs)) / 60).rounded())
      entries.append(
        Entry(
          title: workout.sport,
          detail: "\(minutes) min" + (workout.avgHr.map { " · \($0) avg bpm" } ?? ""),
          period: start.formatted(date: .omitted, time: .shortened),
          family: .effort))
    }
    if usingFixture {
      entries.append(
        Entry(
          title: "Journal", detail: "Late caffeine · marked for comparison",
          period: "20:45", family: .energy))
    }
    if let syncProgress {
      entries.append(
        Entry(title: "Sync", detail: syncProgress, period: "Now", family: .vitals))
    }
    return entries
  }

  var body: some View {
    Group {
      if entries.isEmpty {
        Text("Nothing recorded for this day yet.")
          .mavType(.body)
          .foregroundStyle(MavTheme.inkSecondary)
      } else {
        VStack(alignment: .leading, spacing: 0) {
          ForEach(Array(entries.enumerated()), id: \.element.id) { index, entry in
            HStack(alignment: .top, spacing: 0) {
              Text(entry.period)
                .mavType(.caption)
                .foregroundStyle(MavTheme.inkSecondary)
                .frame(width: 64, alignment: .leading)
                .padding(.top, 11)
              VStack(spacing: 0) {
                Rectangle()
                  .fill(index == 0 ? Color.clear : MavTheme.hairline)
                  .frame(width: 1, height: 10)
                Circle().fill(entry.family.hue).frame(width: 10, height: 10)
                Rectangle()
                  .fill(index == entries.count - 1 ? Color.clear : MavTheme.hairline)
                  .frame(width: 1, height: 52)
              }
              .frame(width: 24)
              VStack(alignment: .leading, spacing: 4) {
                Text(entry.title)
                  .mavType(.label)
                  .foregroundStyle(MavTheme.ink)
                Text(entry.detail)
                  .mavType(.sub)
                  .foregroundStyle(MavTheme.inkSecondary)
                  .fixedSize(horizontal: false, vertical: true)
              }
              .padding(.leading, 12)
              .padding(.top, 7)
              .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 72, alignment: .top)
            .accessibilityElement(children: .combine)
          }
        }
      }
    }
  }
}
