import Foundation

/// What one product signal is doing, and what a surface may say about it.
///
/// The Android twin is `MavAnalyticsState.kt` and the cases are deliberately identical: the same
/// core plan reaches both, and a wearer who is told "needs a strap that reports SpO2" on one
/// platform and shown an empty card on the other is being told two different things about the
/// same data.
///
/// "Not ready" covers at least six genuinely different situations. A surface that renders them
/// identically is lying about five of them.
enum MavSignalState: Equatable {
  /// Nothing has been attempted yet this session.
  case idle
  /// Work is queued or running. `done` of `total` stages have answered.
  case working(done: Int, total: Int)
  /// Every stage answered. `atMs` is when the last one did, so a reading can be aged rather than
  /// presented as current.
  case ready(atMs: Int64, displayable: Bool, applicability: MavApplicability = .sound)
  /// Answered, from inputs that have since moved. The previous values stay on screen — blanking a
  /// good reading because a newer one is pending is worse than labelling it.
  case stale(atMs: Int64, displayable: Bool, applicability: MavApplicability = .sound)

  /// Every stage answered, and it answered about padding rather than about the wearer.
  ///
  /// A separate case rather than a flag on `ready`, so that a surface switching on `ready` to
  /// draw a number cannot reach this branch by accident. The model did run and the result may be
  /// stored; it is not a reading.
  case unfounded(atMs: Int64, substitutions: [String])
  /// Nothing here can run on this device as it stands, with one entry per distinct cause.
  case unavailable(reasons: [MavUnavailable])
  /// The OS declined or postponed the work. Retried when the app is next open.
  case deferred
  /// A stage failed. `retryable` is true once the budget is spent, which is what turns a spinner
  /// into a button.
  case failed(model: String, attempts: Int, retryable: Bool)
  /// A permission the work needs has not been granted.
  case permissionRequired(String)
}

/// Why one stage cannot run, mirroring `mav_engine::analytics::Unmet`.
///
/// The distinction between the four is the whole point. A missing sensor is answered by a
/// different strap; a missing profile field by one tap; an unported front-end by neither, and
/// should not send anyone shopping.
enum MavUnavailable: Equatable {
  case missingStreams([String])
  case missingProfile([String])
  case upstreamUnavailable(String)
  case preprocessingNotPorted(String)
}

/// One signal as the UI reads it.
struct MavSignal: Equatable, Identifiable {
  let name: String
  let state: MavSignalState
  /// Total stages in this signal, including the ones that cannot run.
  let total: Int
  /// Stages that could run on this device.
  let runnable: Int

  var id: String { name }
}

/// Everything the analytics surface renders.
struct MavAnalyticsSnapshot: Equatable {
  var signals: [MavSignal] = []
  /// True while any pass is in flight, for the one global spinner.
  var working: Bool = false
  /// When the last complete pass finished, or nil before the first.
  var lastPassAtMs: Int64?

  func signal(_ name: String) -> MavSignal? { signals.first { $0.name == name } }
}

/// The states the core's plan reports.
enum MavStageState: String {
  case ready
  case blocked
  case cached
  case unavailable
}

/// How many of one signal's models this device can run, as the core counted them.
struct MavSignalCoverage: Equatable {
  let total: Int
  let runnable: Int
}

/// One plan, as the engine consumes it: the per-model rows and the core's own per-signal totals.
struct MavPlan: Equatable {
  var stages: [MavPlannedStage] = []
  var coverage: [String: MavSignalCoverage] = [:]
}

/// One plan row, decoupled from the generated uniffi record so the reducer is testable without a
/// compiled core.
struct MavPlannedStage: Equatable {
  let model: String
  let signal: String
  let state: MavStageState
  let displayable: Bool
  var unavailable: MavUnavailable?
}

/// Turns one core plan into the states the UI renders.
///
/// Pure on purpose. Everything that decides what a wearer is told about their data happens here,
/// and none of it needs a device, a runtime, or a model to prove.
enum MavSignalReducer {
  /// How many times one stage is retried before a surface offers the wearer the button.
  static let retryBudget = 3

  /// - Parameter coverage: the core's own per-signal totals, keyed by signal name. The core
  ///   computes these on every plan precisely so two platforms do not each write the same
  ///   counting loop; a signal absent from the map falls back to counting its own group, which is
  ///   what a test that hands in stages without coverage relies on.
  static func reduce(
    stages: [MavPlannedStage],
    coverage: [String: MavSignalCoverage] = [:],
    completedAtMs: [String: Int64] = [:],
    invalidated: Set<String> = [],
    failures: [String: Int] = [:],
    deferred: Bool = false,
    missingPermission: String? = nil,
    health: [String: MavStageHealth] = [:]
  ) -> [MavSignal] {
    // Grouped in first-appearance order rather than by Dictionary iteration, so the surface does
    // not reshuffle itself between passes.
    var order: [String] = []
    var grouped: [String: [MavPlannedStage]] = [:]
    for stage in stages {
      if grouped[stage.signal] == nil { order.append(stage.signal) }
      grouped[stage.signal, default: []].append(stage)
    }
    return order.map { name in
      let group = grouped[name] ?? []
      let counts = coverage[name]
      return MavSignal(
        name: name,
        state: state(
          for: group,
          completedAtMs: completedAtMs,
          invalidated: invalidated,
          failures: failures,
          deferred: deferred,
          missingPermission: missingPermission,
          health: health
        ),
        total: counts?.total ?? group.count,
        runnable: counts?.runnable ?? group.filter { $0.state != .unavailable }.count
      )
    }
  }

  private static func state(
    for group: [MavPlannedStage],
    completedAtMs: [String: Int64],
    invalidated: Set<String>,
    failures: [String: Int],
    deferred: Bool,
    missingPermission: String?,
    health: [String: MavStageHealth]
  ) -> MavSignalState {
    // A permission the work cannot proceed without outranks everything: the wearer can fix it,
    // and every other state would be describing a consequence rather than the cause.
    if let permission = missingPermission { return .permissionRequired(permission) }

    let runnable = group.filter { $0.state != .unavailable }
    if runnable.isEmpty {
      var reasons: [MavUnavailable] = []
      for stage in group {
        if let reason = stage.unavailable, !reasons.contains(reason) { reasons.append(reason) }
      }
      return .unavailable(reasons: reasons)
    }

    // A failure that has spent its budget is the most useful thing to say next: the work is not
    // going to finish on its own, and the wearer decides whether to retry.
    if let spent = runnable.first(where: { (failures[$0.model] ?? 0) >= retryBudget }) {
      return .failed(model: spent.model, attempts: failures[spent.model] ?? 0, retryable: true)
    }

    let done = runnable.filter { $0.state == .cached }.count
    if done < runnable.count {
      // Deferred only matters while something is genuinely outstanding; a signal that finished
      // before the OS said no is finished.
      return deferred ? .deferred : .working(done: done, total: runnable.count)
    }

    let displayable = runnable.contains { $0.displayable }
    let at = runnable.compactMap { completedAtMs[$0.model] }.max() ?? 0
    let verdict = MavApplicability.worst(runnable.compactMap { health[$0.model]?.applicability })
    if verdict == .unfounded {
      var reasons: [String] = []
      for stage in runnable {
        for reason in health[stage.model]?.substitutions ?? [] where !reasons.contains(reason) {
          reasons.append(reason)
        }
      }
      return .unfounded(atMs: at, substitutions: reasons)
    }
    if runnable.contains(where: { invalidated.contains($0.model) }) {
      return .stale(atMs: at, displayable: displayable, applicability: verdict)
    }
    return .ready(atMs: at, displayable: displayable, applicability: verdict)
  }
}

/// How much of a model's input was real, mirroring
/// `mav_analytic::model_zoo::health::Applicability` and Android's `MavApplicability`.
///
/// Not a confidence score and not to be rendered as one. It says what went in, not how right the
/// answer is: nothing in this build has been checked against labelled ground truth, so there is no
/// honest confidence to show.
enum MavApplicability: String, Equatable, Sendable {
  case sound
  case degraded
  case unfounded
  /// The core did not build these tensors, so it has no view. The replay and test path.
  case unmeasured

  /// Parse the core's wire name. An unknown name is `.unmeasured`, never `.sound` — a newer core
  /// must not be able to make this one more trusting than it should be.
  static func parse(_ name: String) -> MavApplicability {
    MavApplicability(rawValue: name) ?? .unmeasured
  }

  /// The verdict for a group: the worst one present.
  ///
  /// A signal fed by several models is only as sound as its weakest input; taking the best would
  /// let one complete stage vouch for the padded ones beside it.
  static func worst(_ values: [MavApplicability]) -> MavApplicability {
    if values.isEmpty { return .sound }
    if values.contains(.unfounded) { return .unfounded }
    if values.contains(.degraded) { return .degraded }
    if values.contains(.unmeasured) { return .unmeasured }
    return .sound
  }
}

/// What the core said about the tensors behind one model, as the FFI reports it.
struct MavStageHealth: Equatable, Sendable {
  let model: String
  let applicability: MavApplicability
  let substitutions: [String]
}
