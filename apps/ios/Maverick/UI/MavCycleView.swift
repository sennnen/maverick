import SwiftUI

// Cycle Insights. Reached from the cycle row in `Vitals`, which only exists when the feature is on.

struct MavCycleView: View {
  @EnvironmentObject private var model: AppModel
  @State private var log = MavCycleLog.load()
  @State private var today = Repository.localDayKey(Date())

  private var cycleDay: Int? { MavCycle.cycleDay(log: log, on: today) }
  private var median: Int? { MavCycle.medianLength(log: log) }
  private var lengths: [Int] { MavCycle.completedLengths(log: log) }

  var body: some View {
    MavDetailScaffold(title: "Cycle") {
      hero

      MavSectionHeader(title: "This cycle")
      estimateCard

      MavSectionHeader(title: "History")
      historyCard

      MavSectionHeader(title: "Logged starts")
      startsList

      Text(MavCycleLog.disclaimer)
        .mavType(.sub)
        .foregroundStyle(MavTheme.inkSecondary)
        .fixedSize(horizontal: false, vertical: true)
        .padding(.horizontal, 4)
        .padding(.top, 10)
    }
    .onAppear {
      #if DEBUG
        guard model.usingDebugFixture, log.periodStarts.isEmpty else { return }
        let offsets = [127, 98, 70, 42, 14]
        log = MavCycleLog(
          periodStarts: offsets.compactMap {
            Calendar.current.date(byAdding: .day, value: -$0, to: Date()).map(MavCycle.key)
          })
      #endif
    }
  }

  // MARK: Hero

  private var hero: some View {
    VStack(alignment: .leading, spacing: 0) {
      Text("Cycle")
        .mavType(.eyebrow)
        .foregroundStyle(MavTheme.inkSecondary)
        .padding(.bottom, 6)

      if let cycleDay {
        HStack(alignment: .firstTextBaseline, spacing: 11) {
          Text("\(cycleDay)")
            .mavType(.numeralXL)
            .foregroundStyle(MavTheme.ink)
          Text("CYCLE DAY")
            .mavType(.eyebrow)
            .foregroundStyle(MavTheme.inkSecondary)
        }
        Text(
          "Counted from the period start you logged on "
          + "\(log.periodStarts.last(where: { $0 <= today }) ?? "—")."
        )
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)
        .fixedSize(horizontal: false, vertical: true)
        .padding(.top, 14)
      } else {
        Text("Nothing logged yet")
          .mavType(.title)
          .foregroundStyle(MavTheme.ink)
        Text(
          "Log the first day of a period and everything on this screen starts counting from it."
        )
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)
        .fixedSize(horizontal: false, vertical: true)
        .padding(.top, 12)
      }

      MavWideButton(
        title: log.periodStarts.contains(today) ? "Logged for today" : "Log a period start today"
      ) {
        log.logStart(today)
      }
      .disabled(log.periodStarts.contains(today))
      .padding(.top, 15)
    }
    .padding(22)
    .frame(maxWidth: .infinity, alignment: .leading)
    .mavSurface(MavTheme.cardShape, tint: MavTheme.tint(.cycle))
  }

  // MARK: Estimate

  @ViewBuilder private var estimateCard: some View {
    if let range = MavCycle.nextPeriodRange(log: log) {
      MavTile {
        VStack(alignment: .leading, spacing: 7) {
          Text("Next period").mavType(.title).foregroundStyle(MavTheme.ink)
          Text("\(pretty(range.earliest)) – \(pretty(range.latest))")
            .mavType(.numeralSmall)
            .foregroundStyle(MavTheme.ink)
          Text(
            "A range, not a date — from the shortest and longest of your last "
            + "\(min(lengths.count, 6)) cycles."
          )
          .mavType(.sub)
          .foregroundStyle(MavTheme.inkSecondary)
          .fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .combine)
      }
    } else if let needed = MavCycle.cyclesNeeded(log: log) {
      MavUnavailableCard(
        name: "Next period",
        reason:
          "Needs \(needed) more logged cycle\(needed == 1 ? "" : "s"). Two points is not a pattern, "
          + "so there is no estimate rather than a bad one.")
    }
  }

  // MARK: History

  @ViewBuilder private var historyCard: some View {
    if lengths.isEmpty {
      MavUnavailableCard(
        name: "Cycle lengths",
        reason: "A length needs two logged starts. Nothing to chart yet.")
    } else {
      MavTile {
        VStack(alignment: .leading, spacing: 12) {
          MavCycleHistoryChart(
            lengths: lengths,
            accessibilitySummary:
              "\(lengths.count) cycle lengths, from \(lengths.min() ?? 0) to "
              + "\(lengths.max() ?? 0) days")
          Text(
            "Median \(median.map { "\($0)" } ?? "—") days · range "
            + "\(lengths.min() ?? 0)–\(lengths.max() ?? 0)."
          )
          .mavType(.sub)
          .foregroundStyle(MavTheme.inkSecondary)
        }
      }
    }
  }

  // MARK: Logged starts

  @ViewBuilder private var startsList: some View {
    if log.periodStarts.isEmpty {
      MavTile {
        Text("No period starts logged.")
          .mavType(.body)
          .foregroundStyle(MavTheme.inkSecondary)
      }
    } else {
      VStack(spacing: 0) {
        ForEach(Array(log.periodStarts.reversed().enumerated()), id: \.element) { index, start in
          if index > 0 { MavDivider() }
          MavRow(title: pretty(start)) {
            Button {
              log.removeStart(start)
            } label: {
              Text("Remove")
                .mavType(.label)
                .foregroundStyle(MavTheme.destructiveInk())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Remove the period start logged on \(pretty(start))")
          }
        }
      }
      .mavSurface(MavTheme.tileShape)
    }
  }

  private func pretty(_ key: String) -> String {
    guard let date = MavCycle.date(key) else { return key }
    return date.formatted(.dateTime.day().month(.abbreviated))
  }
}
