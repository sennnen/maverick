import Foundation

// The interim-milestone and goal-completion engine for a cardio session.
//
// Ported from the Aura prototype's `WorkoutMilestones`, which is the one piece of that flow worth
// carrying across unchanged: it is pure. No clock reads, no BLE, no storage. The caller feeds it
// the session's current metrics and it says which signals newly fired.
//
// Two properties matter and are asserted rather than assumed:
//
//  - **Each mark fires exactly once.** `State` records what has already fired, so re-evaluating the
//    same metrics is silent.
//  - **A catch-up collapses into one signal.** If the app was backgrounded across three kilometre
//    marks, the wrist buzzes once on return, not three times. A late buzz is a wrong buzz; three
//    late buzzes are a nuisance.
//
// The signals it returns are `MavHapticSignal` values (ADR-032), which the connector may or may not
// be able to render. Deciding *when* is this engine's job; deciding *whether* is the connector's.
//
// The Kotlin twin is `MavMilestones.kt`.

enum MavMilestones {

  /// Resolved once at session start, from the confirm screen plus the milestone deep settings.
  struct Config: Equatable, Sendable {
    var goal: WorkoutGoal = .none
    var zoneTarget: WorkoutZoneTarget?
    /// Interim distance spacing in kilometres, always positive.
    var distanceEveryKm: Double = 1
    var timeMode: TimeMilestoneMode = .halfway
    var calorieMode: CalorieMilestoneMode = .halfway
  }

  /// What has already fired. `Codable` so a persisted session can carry it: a relaunch must not
  /// replay every buzz since the start.
  struct State: Codable, Equatable, Sendable {
    var interimMarks = 0
    var halfwayFired = false
    var goalFired = false
    var zoneTargetFired = false

    init() {}
  }

  /// Something the wearer should be told about, and the reason it is worth telling them.
  enum Event: Equatable, Sendable {
    /// Progress update — a light tap.
    case milestone
    /// The end condition is met — a hard buzz. The session keeps recording; a goal is a target,
    /// not a guillotine, and only the wearer ends a workout.
    case goalComplete
    /// The session's zone target was banked — a light tap, and a checkmark on the bars.
    case zoneTargetMet

    /// The haptic signal this event asks for, if any. `zoneTargetMet` borrows `milestone` rather
    /// than claiming a vocabulary entry of its own: to the wrist it is the same light tap, and
    /// ADR-032's vocabulary is closed.
    var signal: MavHapticSignal {
      switch self {
      case .milestone, .zoneTargetMet: .milestone
      case .goalComplete: .goalComplete
      }
    }
  }

  /// Advance `state` against the session's current metrics.
  ///
  /// - Parameters:
  ///   - elapsedSec: seconds since the session started.
  ///   - distanceM: metres travelled, zero when there is no route.
  ///   - kcal: energy burned so far.
  ///   - zoneSeconds: seconds banked per heart-rate zone, index 0 being zone 1.
  static func evaluate(
    state: inout State,
    config: Config,
    elapsedSec: Int,
    distanceM: Double,
    kcal: Double,
    zoneSeconds: [Double]
  ) -> [Event] {
    var events: [Event] = []

    // Interim marks follow from the *kind* of end condition. A free workout buzzes nothing —
    // silence is the honest default when the wearer named no target.
    switch config.goal.kind {
    case .none:
      break

    case .distance:
      let every = max(config.distanceEveryKm, 0.001)
      let marks = Int((distanceM / 1_000) / every)
      if marks > state.interimMarks {
        state.interimMarks = marks
        // The goal buzz below already covers the final mark, so it is not announced twice.
        if !reached(config.goal, elapsedSec: elapsedSec, distanceM: distanceM, kcal: kcal) {
          events.append(.milestone)
        }
      }

    case .time:
      switch config.timeMode {
      case .off:
        break
      case .halfway:
        if !state.halfwayFired, config.goal.isActive,
          Double(elapsedSec) >= config.goal.value * 60 / 2
        {
          state.halfwayFired = true
          events.append(.milestone)
        }
      case .every10, .every15:
        let every = config.timeMode == .every10 ? 600 : 900
        let marks = elapsedSec / every
        if marks > state.interimMarks {
          state.interimMarks = marks
          if !reached(config.goal, elapsedSec: elapsedSec, distanceM: distanceM, kcal: kcal) {
            events.append(.milestone)
          }
        }
      }

    case .calories:
      switch config.calorieMode {
      case .off:
        break
      case .halfway:
        if !state.halfwayFired, config.goal.isActive, kcal >= config.goal.value / 2 {
          state.halfwayFired = true
          events.append(.milestone)
        }
      case .every50, .every100:
        let every = config.calorieMode == .every50 ? 50.0 : 100.0
        let marks = Int(kcal / every)
        if marks > state.interimMarks {
          state.interimMarks = marks
          if !reached(config.goal, elapsedSec: elapsedSec, distanceM: distanceM, kcal: kcal) {
            events.append(.milestone)
          }
        }
      }
    }

    if !state.goalFired, config.goal.isActive,
      reached(config.goal, elapsedSec: elapsedSec, distanceM: distanceM, kcal: kcal)
    {
      state.goalFired = true
      events.append(.goalComplete)
    }

    if !state.zoneTargetFired, let target = config.zoneTarget, (1...5).contains(target.zone),
      zoneSeconds.indices.contains(target.zone - 1),
      zoneSeconds[target.zone - 1] >= Double(target.minutes) * 60
    {
      state.zoneTargetFired = true
      events.append(.zoneTargetMet)
    }

    return events
  }

  /// Whether the end condition is satisfied. Values are stored natively — kilometres, minutes,
  /// kilocalories — so the display unit never reaches this comparison.
  static func reached(
    _ goal: WorkoutGoal, elapsedSec: Int, distanceM: Double, kcal: Double
  ) -> Bool {
    guard goal.isActive else { return false }
    switch goal.kind {
    case .none: return false
    case .distance: return distanceM / 1_000 >= goal.value
    case .time: return Double(elapsedSec) >= goal.value * 60
    case .calories: return kcal >= goal.value
    }
  }

  /// How far through the end condition the session is, 0…1, or nil when there is no goal. Drives
  /// the live screen's progress bar.
  static func progress(
    _ goal: WorkoutGoal, elapsedSec: Int, distanceM: Double, kcal: Double
  ) -> Double? {
    guard goal.isActive else { return nil }
    let done: Double
    switch goal.kind {
    case .none: return nil
    case .distance: done = distanceM / 1_000
    case .time: done = Double(elapsedSec) / 60
    case .calories: done = kcal
    }
    return min(max(done / goal.value, 0), 1)
  }
}
