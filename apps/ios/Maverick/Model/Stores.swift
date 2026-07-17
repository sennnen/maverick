import Foundation
import SwiftUI

// The environment stores the copied Aura views observe — Maverick's member surface, backed by the
// Rust core's host snapshot instead of Maverick's Swift-side BLE engine and GRDB store. Values the
// core cannot fill yet stay honestly nil/empty; nothing is estimated app-side.

/// Live connection + biometric readout, derived from the core snapshot's connection block.
@MainActor
final class LiveState: ObservableObject {
  @Published var connected = false
  @Published var bonded = false
  @Published var heartRate: Int?
  @Published var batteryPct: Double?
  @Published var charging: Bool?
  @Published var advertisingName: String?
}

/// A manual workout in progress (strain-hub live banner). Never set until live sessions land.
struct ActiveWorkout: Equatable {
  var startMs: Int
  var liveStrain: Double = 0
}

/// A strength session in progress. Inert until the strength lane lands.
final class StrengthSession: ObservableObject {}

/// The app-wide model behind the Aura views — the Mav twin of Maverick's `AppModel`. It owns the
/// core runtime worker (via `MavStore`) and republishes snapshot facts into the stores the
/// views observe.
@MainActor
final class AppModel: ObservableObject {
  @Published var bpm: Int?
  @Published var activeWorkout: ActiveWorkout?
  @Published var strengthSession: StrengthSession?
  @Published var illnessSignal: IllnessSignalEngine.Result?
  @Published var cyclePhase: CyclePhaseEngine.Result?
  /// The admitted session PRV read model, when the core computed one. Never presented as HRV.
  @Published var prv: MavPrv?
  @Published var prvUnavailableReason: String?
  @Published var recoveryUnavailableReason: String?

  let countdown = CountdownTimer()
  let behavior = BehaviorStore()
  let strandML = StrandMLEngine()

  /// Wrist haptics — no transport wiring in the host yet, so this is inert.
  func buzz(loops: UInt8 = 2) {}

  /// Republish the core snapshot into the live/model surfaces the views read. The runtime's
  /// link-up states are `subscribing` and `streaming` (there is no `connected` state in
  /// `host-snapshot/v1`), and a stored heart rate or battery figure never outlives the link.
  func apply(snapshot: MavSnapshot, to live: LiveState) {
    let connected = snapshot.connectionState == "subscribing"
      || snapshot.connectionState == "streaming"
    bpm = connected ? snapshot.currentBpm : nil
    prv = snapshot.prv
    prvUnavailableReason = snapshot.prvUnavailableReason
    recoveryUnavailableReason = snapshot.recoveryUnavailableReason
    live.connected = connected
    live.bonded = connected
    live.heartRate = connected ? snapshot.currentBpm : nil
    live.batteryPct = connected ? snapshot.batteryPercent.map(Double.init) : nil
    live.charging = connected ? snapshot.charging : nil
    live.advertisingName = snapshot.deviceName
  }
}

/// Behaviour/automation preferences the settings sheet edits. UserDefaults-backed.
@MainActor
final class BehaviorStore: ObservableObject {
  private let d = UserDefaults.standard
  private static let zoneAlertsKey = "behavior.zoneAlertModes"

  @Published var zoneAlertModes: [ZoneAlertMode] {
    didSet { d.set(zoneAlertModes.map(\.rawValue), forKey: Self.zoneAlertsKey) }
  }

  init() {
    let raw = d.stringArray(forKey: Self.zoneAlertsKey) ?? []
    let stored = raw.compactMap(ZoneAlertMode.init(rawValue:))
    zoneAlertModes = stored.count == 5 ? stored : Array(repeating: .off, count: 5)
  }
}

/// System-health bridge surface. Mav has no HealthKit wiring yet, so this reports
/// `.unavailable` and every action is inert — the settings row explains itself honestly.
@MainActor
final class HealthKitBridge: ObservableObject {
  enum AuthState: Equatable {
    case unknown, unavailable, denied, authorized
    case entitlementMissing
  }

  @Published private(set) var auth: AuthState = .unavailable
  @Published private(set) var lastSync: Date?
  @Published private(set) var syncing = false
  @Published private(set) var lastError: String?

  func requestAuthorization() async {}
  func sync(days: Int = 30) async {}
}

/// On-device ML signal surface (AuraMLSignalsCard). Inert until the native-inference lane lands.
@MainActor
final class StrandMLEngine: ObservableObject {
  @Published var backboneActive = false
  @Published var stressLoad: Double?
  @Published var vo2max: Double?
  @Published var respirationRate: Double?
  @Published var afib: AFibScreener.AFibResult?
}

/// Shape twin of StrandML's screener result (the rhythm row renders it).
enum AFibScreener {
  struct AFibResult: Equatable, Sendable {
    let confidence: Double
    let irregular: Bool
    let reliable: Bool
  }
}

/// The user's body profile — @AppStorage-backed, same keys and ranges as Maverick's ProfileStore.
@MainActor
final class ProfileStore: ObservableObject {
  private let d = UserDefaults.standard

  @Published var age: Int { didSet { d.set(age, forKey: "profile.age") } }
  @Published var sex: String { didSet { d.set(sex, forKey: "profile.sex") } }
  @Published var weightKg: Double { didSet { d.set(weightKg, forKey: "profile.weightKg") } }
  @Published var heightCm: Double { didSet { d.set(heightCm, forKey: "profile.heightCm") } }
  /// Manual max-heart-rate override in bpm; 0 = automatic (Tanaka).
  @Published var hrMaxOverride: Int { didSet { d.set(hrMaxOverride, forKey: "profile.hrMaxOverride") } }

  /// Effective HR-max: the manual override if set, else the Tanaka estimate round(208 − 0.7·age).
  var hrMax: Int { hrMaxOverride > 0 ? hrMaxOverride : Int((208.0 - 0.7 * Double(age)).rounded()) }

  init() {
    let age = d.integer(forKey: "profile.age")
    self.age = age == 0 ? 30 : min(max(age, 5), 120)
    sex = d.string(forKey: "profile.sex") ?? "male"
    let w = d.double(forKey: "profile.weightKg")
    weightKg = w == 0 ? 75 : min(max(w, 20), 300)
    let h = d.double(forKey: "profile.heightCm")
    heightCm = h == 0 ? 178 : min(max(h, 90), 250)
    hrMaxOverride = min(max(d.integer(forKey: "profile.hrMaxOverride"), 0), 230)
  }
}

/// Backup & Sync surface. Whole-store backup lands after the storage lane freezes its export
/// format; until then there is never a recorded backup and `backupNow` reports failure.
enum FolderBackup {
  /// Unix millis of the last recorded backup; 0 = never (there is no backup lane yet).
  static var lastBackupMs: Int64 { 0 }

  static func backupNow(checkpoint: () async -> Bool) async -> Bool { false }

  static func catchUpIfDue(checkpoint: () async -> Bool) async {}
}

/// Opaque handle over the core's on-disk store, for the diagnostics card's size readout.
struct MavStoreHandle {
  let databaseURL: URL

  func databaseFileSizeBytes() async -> Int64? {
    (try? FileManager.default.attributesOfItem(atPath: databaseURL.path)[.size] as? Int64) ?? nil
  }
}
