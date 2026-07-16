import SwiftUI

// Storage & diagnostics (Data group) — iOS port of Android's `AuraDiagnosticsSheet`
// (android/app/src/main/java/com/noop/ui/aura/AuraToolSheets.kt). Read-only on-device store
// stats plus maintenance actions, all running locally.
//
// Parity note: Android ships THREE maintenance actions (Compact database / VACUUM, Run
// integrity test / PRAGMA quick_check, Back up now). This iOS port ships only TWO:
//   - "Run integrity test" — safe, because `WhoopStore.DatabaseIntegrity.quickCheckFailure(atPath:)`
//     is an already-public, already-battle-tested helper (used by DataBackup's pre-export
//     verification) that opens its own short-lived, read-only connection beside the app's live
//     GRDB pool — exactly the sanctioned pattern for probing the store without touching the actor.
//   - "Back up now" — reuses the existing `FolderBackup.backupNow`, the same function the
//     Backup & Sync screen's button calls.
// "Compact database" (VACUUM) is DELIBERATELY OMITTED: there is no safe, already-exposed way to
// run it. `WhoopStore`'s public surface has no VACUUM/raw-SQL entry point, and the app target
// does not link GRDB directly (only `WhoopStore` does, via SwiftPM — `import GRDB` is not
// available here), so `WhoopStore.registryWriter`'s `any DatabaseWriter` can't actually be driven
// from this file. Doing it anyway would mean either adding new public API to the WhoopStore
// package or wiring a brand-new direct GRDB dependency into the app target — both are build-graph
// changes outside a settings-screen edit, unverifiable without a full Xcode build, and exactly the
// kind of "hack around it" this port was told to avoid. See the parity report for detail.
struct AuraDiagnosticsView: View {
  @EnvironmentObject private var repo: Repository

  @State private var dbBytes: Int64?
  @State private var dayCount: Int?
  @State private var workoutCount: Int?
  @State private var lastBackupMs = FolderBackup.lastBackupMs
  @State private var busy = false
  @State private var lastResult: String?

  var body: some View {
    AuraSheet(title: "Storage & diagnostics", family: .vitals) {
      store
      maintenance
    }
    .task { await refresh() }
  }

  // MARK: On-device store

  private var store: some View {
    VStack(alignment: .leading, spacing: 12) {
      AuraSectionHeader(title: "On-device store")
      VStack(spacing: 0) {
        AuraInfoRow(label: "Database", value: dbBytes.map(Self.formatBytes) ?? "…")
        divider
        AuraInfoRow(label: "Days stored", value: dayCount.map { "\($0)" } ?? "…")
        divider
        AuraInfoRow(label: "Workouts", value: workoutCount.map { "\($0)" } ?? "…")
        divider
        AuraInfoRow(label: "Last backup", value: lastBackupMs > 0
                    ? Date(timeIntervalSince1970: Double(lastBackupMs) / 1000)
                        .formatted(date: .abbreviated, time: .shortened)
                    : "Never")
      }
      .padding(.vertical, 4)
      .background(AuraDesign.card, in: AuraDesign.tileShape)
      .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
    }
  }

  // MARK: Maintenance

  private var maintenance: some View {
    VStack(alignment: .leading, spacing: 12) {
      AuraSectionHeader(title: "Maintenance")
      VStack(spacing: 0) {
        AuraNavRow(icon: "checkmark.seal", title: "Run integrity test",
                   detail: busy ? "Working…" : "PRAGMA quick_check",
                   tint: AuraDesign.Family.vitals.glow) { runIntegrityTest() }
        divider
        AuraNavRow(icon: "shippingbox", title: "Back up now",
                   detail: "To your backup folder") { backupNow() }
      }
      .padding(.vertical, 4)
      .background(AuraDesign.card, in: AuraDesign.tileShape)
      .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
      if let lastResult {
        Text(lastResult).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.65))
          .padding(.horizontal, 4)
      }
      Text("Everything above runs locally. Database compaction isn't available on this build yet; back up regularly instead.")
        .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.45))
        .padding(.horizontal, 4)
    }
  }

  private var divider: some View {
    Rectangle().fill(AuraDesign.ink.opacity(0.08)).frame(height: 1).padding(.leading, 18)
  }

  // MARK: - Actions

  private func refresh() async {
    guard let handle = await repo.storeHandle() else { return }
    dbBytes = await handle.databaseFileSizeBytes()
    if let volume = await repo.dataVolumeSnapshot() {
      dayCount = volume.importedDays
      workoutCount = volume.workouts
    }
    lastBackupMs = FolderBackup.lastBackupMs
  }

  private func runIntegrityTest() {
    guard !busy else { return }
    busy = true
    Task {
      let path: String
      do { path = try StorePaths.defaultDatabasePath() }
      catch {
        lastResult = "Integrity test failed: \(error.localizedDescription)"
        busy = false
        return
      }
      // `quickCheckFailure` does synchronous file I/O — hop off the main actor so the UI doesn't
      // stall on a large store, mirroring how DataBackup's own export-side check runs detached.
      let complaint = await Task.detached(priority: .utility) {
        DatabaseIntegrity.quickCheckFailure(atPath: path)
      }.value
      lastResult = complaint.map { "Integrity: \($0)" } ?? "Integrity: OK, no corruption found."
      busy = false
    }
  }

  private func backupNow() {
    guard !busy else { return }
    busy = true
    Task {
      let ok = await FolderBackup.backupNow(checkpoint: { await repo.checkpointForBackup() })
      lastResult = ok ? "Backup written." : "Backup failed. Pick a folder in Backup & Sync first."
      lastBackupMs = FolderBackup.lastBackupMs
      busy = false
    }
  }

  private static func formatBytes(_ bytes: Int64) -> String {
    let f = ByteCountFormatter()
    f.countStyle = .file
    f.allowedUnits = [.useKB, .useMB, .useGB]
    return f.string(fromByteCount: bytes)
  }
}
