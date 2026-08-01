import SwiftUI

// Reports, diagnostics and the journal. Small screens, all reached from exactly one place.

/// Every day the core has scored, newest first. There is no "weekly summary" computed here — that
/// would be the app inventing an aggregate — so the report is the days themselves.
struct MavReportsView: View {
  let day: String
  @EnvironmentObject private var repo: Repository

  var body: some View {
    MavDetailScaffold(title: "Report") {
      MavTile {
        Text(
          "One row per day the core has scored. A missing day is a gap in the recording, not a "
          + "row we left out."
        )
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)
        .fixedSize(horizontal: false, vertical: true)
      }

      if repo.days.isEmpty {
        MavUnavailableCard(
          name: "Scored days", reason: "No day has been scored yet.")
      } else {
        VStack(spacing: 0) {
          ForEach(Array(repo.days.reversed().enumerated()), id: \.element.day) { index, metric in
            if index > 0 { MavDivider() }
            MavRow(
              title: metric.day,
              detail: metric.avgHrv.map {
                "\(MavMetricMapper.variabilityTitle(metric.hrvLabel)) "
                + "\(MavMetricMapper.decimal($0, places: 1)) ms"
              } ?? "No variability scored"
            )
            .accessibilityElement(children: .combine)
          }
        }
        .mavSurface(MavTheme.tileShape)
      }
    }
  }
}

/// What the core recorded about itself. Not a debug screen — a person is entitled to know what
/// their own device wrote down.
struct MavDiagnosticsView: View {
  @EnvironmentObject private var repo: Repository
  @EnvironmentObject private var model: AppModel
  @EnvironmentObject private var connectors: ConnectorManager
  @State private var integrity: String?
  @State private var sizeBytes: Int64?

  var body: some View {
    MavDetailScaffold(title: "Diagnostics") {
      MavSectionHeader(title: "Store")
      VStack(spacing: 0) {
        MavRow(title: "Integrity") {
          Text(integrity ?? "OK")
            .mavType(.label)
            .foregroundStyle(integrity == nil ? MavTheme.inkSecondary : MavTheme.destructiveInk())
        }
        MavDivider()
        MavRow(title: "Size") {
          Text(
            sizeBytes.map { ByteCountFormatter.string(fromByteCount: $0, countStyle: .file) } ?? "—"
          )
          .mavType(.label)
          .monospacedDigit()
          .foregroundStyle(MavTheme.inkSecondary)
        }
        MavDivider()
        MavRow(title: "Scored days") {
          Text("\(repo.days.count)")
            .mavType(.label)
            .monospacedDigit()
            .foregroundStyle(MavTheme.inkSecondary)
        }
      }
      .mavSurface(MavTheme.tileShape)

      MavSectionHeader(title: "Connector")
      VStack(spacing: 0) {
        MavRow(title: "State") {
          Text(connectors.connection.label)
            .mavType(.label)
            .foregroundStyle(MavTheme.inkSecondary)
        }
        MavDivider()
        MavRow(title: "Connector") {
          Text(connectors.connection.connectorID ?? "None")
            .mavType(.label)
            .foregroundStyle(MavTheme.inkSecondary)
        }
        if let error = connectors.connection.errorMessage {
          MavDivider()
          MavRow(title: "Last error", detail: error)
        }
      }
      .mavSurface(MavTheme.tileShape)

      if let snapshot = model.dailySnapshot {
        MavSectionHeader(title: "Today's snapshot")
        MavTile {
          VStack(alignment: .leading, spacing: 7) {
            Text(snapshot.snapshotHash)
              .mavType(.sub)
              .monospaced()
              .foregroundStyle(MavTheme.ink)
              .fixedSize(horizontal: false, vertical: true)
            Text(
              "The digest both platforms must read identically from the same day. That equality "
              + "is the parity contract."
            )
            .mavType(.sub)
            .foregroundStyle(MavTheme.inkSecondary)
            .fixedSize(horizontal: false, vertical: true)
          }
        }
      }
    }
    .task {
      sizeBytes = await repo.storeHandle()?.databaseFileSizeBytes()
      if let path = try? StorePaths.defaultDatabasePath() {
        integrity = DatabaseIntegrity.quickCheckFailure(atPath: path)
      }
    }
  }
}

/// The journal. Answers persist on-device under the same natural key the core's journal lane will
/// adopt, so nothing typed today is lost when that lane lands.
struct MavJournalView: View {
  @EnvironmentObject private var repo: Repository
  @State private var entries: [JournalEntry] = []
  @State private var day = Repository.localDayKey(Date())

  private let questions = JournalCatalogStore.starterQuestions

  var body: some View {
    MavDetailScaffold(title: "Journal") {
      MavTile {
        Text(
          "What you log here is yours and stays on this phone. It is never fed into a score "
          + "without an admitted analytic asking for it."
        )
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)
        .fixedSize(horizontal: false, vertical: true)
      }

      MavSectionHeader(title: day)
      VStack(spacing: 0) {
        ForEach(Array(questions.enumerated()), id: \.offset) { index, question in
          if index > 0 { MavDivider() }
          MavToggleRow(
            title: question,
            isOn: Binding(
              get: { answer(question) ?? false },
              set: { newValue in
                Task {
                  await repo.saveJournalAnswer(day: day, question: question, answeredYes: newValue)
                  entries = await repo.journalEntries()
                }
              }))
        }
      }
      .mavSurface(MavTheme.tileShape)
    }
    .task { entries = await repo.journalEntries() }
  }

  private func answer(_ question: String) -> Bool? {
    entries.first { $0.day == day && $0.question == question }?.answeredYes
  }
}
