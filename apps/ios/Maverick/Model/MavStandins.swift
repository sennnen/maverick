import SwiftUI

// Mav stand-ins for the Maverick destinations whose subsystems live in the Rust core's future
// lanes (AI coach, workouts, strength, alarm, pairing, import, HealthKit, backup, legacy
// settings). Same type names + call shapes as the Maverick originals, so every copied Aura view
// stays byte-identical to its source; each renders an honest Aura-styled pending state.

private struct MavPendingSheet: View {
  let title: String
  let icon: String
  let family: AuraDesign.Family
  let body_: String
  var footer: String?

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: AuraDesign.sectionGap) {
        Text(title).font(AuraDesign.display(30)).foregroundStyle(AuraDesign.ink)
          .padding(.top, 22)
        VStack(alignment: .leading, spacing: 14) {
          Image(systemName: icon)
            .font(.system(size: 22, weight: .medium))
            .foregroundStyle(family.glow)
          Text(body_)
            .font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.72))
            .fixedSize(horizontal: false, vertical: true)
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AuraDesign.card, in: AuraDesign.tileShape)
        .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
        if let footer {
          Text(footer)
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.45))
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 4)
        }
      }
      .padding(.horizontal, AuraDesign.screenMargin)
      .padding(.bottom, 40)
    }
    .scrollIndicators(.hidden)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .background(AuraDesign.bg.ignoresSafeArea())
  }
}

/// Private coach. Maverick's engine calls a user-keyed AI API; Mav stays fully on-device, so the
/// coach waits for the core's day aggregates and an on-device path.
struct AuraCoachView: View {
  var body: some View {
    MavPendingSheet(
      title: "Coach", icon: "sparkles", family: .effort,
      body_: "The coach reads your stored days and answers privately. It unlocks once the core "
        + "serves day aggregates.",
      footer: "Nothing leaves this device."
    )
  }
}

/// Workouts list. Appears once the core's timeline stores activity sessions from a connector.
struct AuraWorkoutsView: View {
  var body: some View {
    MavPendingSheet(
      title: "Workouts", icon: "figure.run", family: .effort,
      body_: "Sessions, zones and routes appear once the core's timeline stores activity from "
        + "a connector."
    )
  }
}

/// Strength log. Arrives with the workout lane.
struct AuraStrengthView: View {
  var body: some View {
    MavPendingSheet(
      title: "Strength", icon: "dumbbell", family: .effort,
      body_: "Sets, reps and progression arrive with the workout lane."
    )
  }
}

/// Smart alarm. Needs the strap transport lane (wrist haptics).
struct AuraAlarmView: View {
  var body: some View {
    MavPendingSheet(
      title: "Smart alarm", icon: "alarm", family: .rest,
      body_: "Wake windows and the wrist buzz need the strap transport lane."
    )
  }
}

/// Historical import. Runs through the core's ingest lane so every row keeps its provenance.
struct AuraMigrateView: View {
  var body: some View {
    MavPendingSheet(
      title: "Import data", icon: "shippingbox.and.arrow.backward", family: .vitals,
      body_: "Historical import (WHOOP export, Apple Health) runs through the core's ingest "
        + "lane so every row keeps its provenance. It isn't wired into this build yet."
    )
  }
}

/// Apple Health detail. The bridge reports unavailable until the HealthKit lane lands.
struct AuraHealthView: View {
  var body: some View {
    MavPendingSheet(
      title: "Apple Health", icon: "heart.text.square", family: .vitals,
      body_: "Apple Health exchange is planned after the core's ingest lane. Mav never uploads "
        + "anything, and strap telemetry always outranks system-health fills."
    )
  }
}

/// Backup & Sync. Lands after the storage lane freezes its export format.
struct BackupSyncView: View {
  var body: some View {
    MavPendingSheet(
      title: "Backup & Sync", icon: "externaldrive", family: .charge,
      body_: "Whole-store backup lands after the storage lane freezes its export format."
    )
  }
}

/// The legacy full-settings screen. Everything Mav can configure lives in the Aura sheet.
struct SettingsView: View {
  @EnvironmentObject private var store: MavStore

  var body: some View {
    MavPendingSheet(
      title: "All settings", icon: "gearshape.2", family: .charge,
      body_: settingsBody,
      footer: "Runtime facts come straight from the core snapshot."
    )
  }

  private var settingsBody: String {
    guard case .ready = store.state else { return "Core runtime unavailable." }
    return "Core runtime ready. Connector code and telemetry remain on this device."
  }
}

/// CSV export runs once the core's read models can feed it; until then it reports honestly.
enum CsvExport {
  enum Outcome {
    case exported(URL)
    case cancelled
    case failure(String)
  }

  @MainActor
  static func run(repo: Repository) async -> Outcome {
    .failure("No stored history to export yet")
  }
}
