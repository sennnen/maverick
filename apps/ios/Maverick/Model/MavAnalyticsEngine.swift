import Foundation

/// The production analytics loop: plan, queue, drain, publish.
///
/// The Android twin of this is `MavAnalyticsEngine.kt`, and the two are deliberately the same
/// shape, because neither of them decides anything. Which of the forty-one models are worth
/// running on this device, in what order, and whether the answer is already known are all
/// questions the Rust core answers — `mav_engine::analytics`. What is left on each platform is
/// its own half: when a pass happens, how hard it pushes, and getting all of it off the thread
/// that draws.
///
/// One pass runs at a time. A second caller is turned away rather than queued: a scene
/// activation during a background refresh should cost nothing, and waiting on an inference to
/// return from `sceneDidBecomeActive` is how a resume turns into a hitch.
final class MavAnalyticsEngine: @unchecked Sendable {
  /// Interactive or deferred, mirroring `mav_engine::analytics::RunMode`.
  enum RunMode: String {
    /// The wearer is looking at the screen.
    case interactive
    /// Nobody is watching; leave the accelerator alone between stages.
    case deferred

    /// How many stages one drain round may take. The core bounds the plan the same way; this is
    /// the platform's half of the same number.
    var burst: Int {
      switch self {
      case .interactive: return 32
      case .deferred: return 4
      }
    }
  }

  enum Outcome {
    case completed
    case partial
    case failed
    case skippedBusy
  }

  private let runtime: MavAnalyticsRuntime
  private let runner: MavModelBridge.Runner
  private let clock: () -> Int64
  /// Serialises passes and every mutation below. Named rather than a bare lock so a hang shows
  /// up in a sample with a label on it.
  private let queue = DispatchQueue(label: "com.sennnen.mav.analytics", qos: .utility)
  private var running = false
  private var failures: [String: Int] = [:]

  /// The latest snapshot, for a view to observe. Read on any thread; written only on `queue`.
  private let state = MavAnalyticsState()

  init(
    runtime: MavAnalyticsRuntime,
    runner: MavModelBridge.Runner,
    clock: @escaping () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) }
  ) {
    self.runtime = runtime
    self.runner = runner
    self.clock = clock
  }

  var snapshot: MavAnalyticsSnapshot { state.snapshot }

  /// Observe snapshot changes. Delivered on the main queue, because the only caller is a view.
  func onChange(_ handler: @escaping @Sendable (MavAnalyticsSnapshot) -> Void) {
    state.onChange = handler
  }

  /// Run one pass for `deviceID`.
  ///
  /// Synchronous on `queue` and never called from the main thread. `completion` carries what the
  /// pass achieved, which is what a `BGTask` needs in order to call `setTaskCompleted`.
  func runPass(
    deviceID: UInt64,
    mode: RunMode,
    permissionMissing: String? = nil,
    completion: @escaping @Sendable (Outcome) -> Void
  ) {
    queue.async { [weak self] in
      guard let self else { return completion(.failed) }
      if self.running {
        return completion(.skippedBusy)
      }
      self.running = true
      defer { self.running = false }
      completion(self.onePass(deviceID: deviceID, mode: mode, permissionMissing: permissionMissing))
    }
  }

  private func onePass(deviceID: UInt64, mode: RunMode, permissionMissing: String?) -> Outcome {
    state.setWorking(true)
    let now = clock()

    // Queue whatever the day's stored optical signal can feed. The core reads the store and
    // builds the tensors; nothing here ever sees a raw sample.
    do {
      try runtime.admitPPGStages(deviceID: deviceID, atMs: now)
    } catch {
      state.setWorking(false)
      return .failed
    }

    var failed = 0
    // Drain until the queue is empty. An encoder completing can queue its heads *inside* the
    // core, so an empty queue — not a fixed count — is the terminating condition.
    var rounds = 0
    while rounds < Self.maxRounds {
      rounds += 1
      let outcome = MavModelBridge(host: runtime.host(), runner: runner, clock: clock)
        .drain(limit: mode.burst)
      failed += outcome.failed
      if outcome.completed == 0 && outcome.failed == 0 { break }
    }

    let plan = (try? runtime.plan(
      deviceID: deviceID,
      atMs: now,
      mode: mode,
      profileFields: runtime.profileFields()
    )) ?? MavPlan()
    let completedAt = (try? runtime.cacheCompletedAt()) ?? [:]
    recordFailures(stages: plan.stages, failed: failed)

    state.publish(
      MavAnalyticsSnapshot(
        signals: MavSignalReducer.reduce(
          stages: plan.stages,
          coverage: plan.coverage,
          completedAtMs: completedAt,
          failures: failures,
          deferred: mode == .deferred && failed > 0,
          missingPermission: permissionMissing
        ),
        working: false,
        lastPassAtMs: now
      )
    )
    return failed > 0 ? .partial : .completed
  }

  /// Count a failure against every stage that was ready and did not complete.
  ///
  /// Per model rather than per pass, so one missing artefact exhausts its own budget while the
  /// rest keep trying — a single global counter would stop the whole zoo for one bad model.
  private func recordFailures(stages: [MavPlannedStage], failed: Int) {
    guard failed > 0 else {
      for stage in stages where stage.state == .cached { failures.removeValue(forKey: stage.model) }
      return
    }
    for stage in stages where stage.state == .ready {
      failures[stage.model, default: 0] += 1
    }
  }

  /// Forget every retry budget, so a wearer tapping retry gets a genuine fresh attempt.
  func resetRetries() {
    queue.async { [weak self] in self?.failures.removeAll() }
  }

  /// Drop whatever the runner is holding resident.
  ///
  /// Queued behind any pass in flight rather than done immediately: releasing a model out from
  /// under an inference is at best a reload and at worst a fault in Core ML's own memory. The
  /// caller is a lifecycle transition, so a few hundred milliseconds late is free.
  func releaseRunnerCache() {
    queue.async { [runner] in runner.releaseCache() }
  }

  /// Bound on drain rounds in one pass. The core's queue is bounded at 32 and each round empties
  /// up to a burst of it, so this sits far above any real terminating case; it exists so a core
  /// that somehow kept re-queueing could not spend a whole background window in this loop.
  private static let maxRounds = 16
}

/// The mutable half, isolated so the engine itself stays a value-shaped thing.
private final class MavAnalyticsState: @unchecked Sendable {
  private let lock = NSLock()
  private var current = MavAnalyticsSnapshot()
  var onChange: (@Sendable (MavAnalyticsSnapshot) -> Void)?

  var snapshot: MavAnalyticsSnapshot {
    lock.lock()
    defer { lock.unlock() }
    return current
  }

  func publish(_ next: MavAnalyticsSnapshot) {
    lock.lock()
    current = next
    let handler = onChange
    lock.unlock()
    guard let handler else { return }
    DispatchQueue.main.async { handler(next) }
  }

  func setWorking(_ working: Bool) {
    lock.lock()
    let next = MavAnalyticsSnapshot(
      signals: current.signals,
      working: working,
      lastPassAtMs: current.lastPassAtMs
    )
    current = next
    let handler = onChange
    lock.unlock()
    guard let handler else { return }
    DispatchQueue.main.async { handler(next) }
  }
}

/// The core calls this engine needs, behind a protocol.
///
/// Not indirection for its own sake: the generated `MavRuntime` cannot be constructed without a
/// database and a compiled core, so a protocol here is what lets every state transition above be
/// tested with no device, no model and no Rust.
protocol MavAnalyticsRuntime {
  func host() -> MavModelBridge.Host
  func admitPPGStages(deviceID: UInt64, atMs: Int64) throws
  func plan(
    deviceID: UInt64,
    atMs: Int64,
    mode: MavAnalyticsEngine.RunMode,
    profileFields: [String]
  ) throws -> MavPlan
  func profileFields() -> [String]
  /// When each model last answered, from the core's persisted cache.
  func cacheCompletedAt() throws -> [String: Int64]
}
