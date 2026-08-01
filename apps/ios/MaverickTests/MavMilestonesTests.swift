import XCTest

@testable import Mav

/// The milestone engine. The Android twin is `MavMilestonesTest.kt` and asserts the same cases.
///
/// The two properties worth protecting are that a mark fires exactly once, and that a catch-up
/// collapses into a single signal. Both are easy to regress into a wrist that buzzes four times
/// when an app returns from the background, which is the failure the wearer notices most.
final class MavMilestonesTests: XCTestCase {

  private func distanceConfig(km: Double, every: Double = 1) -> MavMilestones.Config {
    MavMilestones.Config(
      goal: WorkoutGoal(kind: .distance, value: km), distanceEveryKm: every)
  }

  private func evaluate(
    _ state: inout MavMilestones.State,
    _ config: MavMilestones.Config,
    elapsed: Int = 0,
    distanceM: Double = 0,
    kcal: Double = 0,
    zoneSeconds: [Double] = []
  ) -> [MavMilestones.Event] {
    MavMilestones.evaluate(
      state: &state, config: config, elapsedSec: elapsed, distanceM: distanceM, kcal: kcal,
      zoneSeconds: zoneSeconds)
  }

  // MARK: Silence

  func testAFreeWorkoutNeverBuzzes() {
    var state = MavMilestones.State()
    let config = MavMilestones.Config()
    XCTAssertEqual(evaluate(&state, config, elapsed: 7_200, distanceM: 42_000, kcal: 3_000), [])
    XCTAssertEqual(state, MavMilestones.State())
  }

  // MARK: Distance

  func testEachKilometreMarkFiresOnce() {
    var state = MavMilestones.State()
    let config = distanceConfig(km: 5)

    XCTAssertEqual(evaluate(&state, config, distanceM: 999), [])
    XCTAssertEqual(evaluate(&state, config, distanceM: 1_000), [.milestone])
    // Re-evaluating the same distance is silent.
    XCTAssertEqual(evaluate(&state, config, distanceM: 1_000), [])
    XCTAssertEqual(evaluate(&state, config, distanceM: 1_400), [])
    XCTAssertEqual(evaluate(&state, config, distanceM: 2_000), [.milestone])
  }

  func testACatchUpAcrossSeveralMarksCollapsesIntoOneBuzz() {
    var state = MavMilestones.State()
    let config = distanceConfig(km: 10)

    XCTAssertEqual(evaluate(&state, config, distanceM: 1_000), [.milestone])
    // The app was away for three kilometres. The wrist buzzes once, not three times.
    XCTAssertEqual(evaluate(&state, config, distanceM: 4_000), [.milestone])
    XCTAssertEqual(state.interimMarks, 4)
  }

  func testTheFinalMarkIsNotAnnouncedTwice() {
    var state = MavMilestones.State()
    let config = distanceConfig(km: 3)

    XCTAssertEqual(evaluate(&state, config, distanceM: 1_000), [.milestone])
    XCTAssertEqual(evaluate(&state, config, distanceM: 2_000), [.milestone])
    // Reaching 3 km is both a kilometre mark and the goal. Only the goal is announced.
    XCTAssertEqual(evaluate(&state, config, distanceM: 3_000), [.goalComplete])
  }

  func testTheGoalFiresOnceAndTheSessionKeepsRunning() {
    var state = MavMilestones.State()
    let config = distanceConfig(km: 2)

    XCTAssertEqual(evaluate(&state, config, distanceM: 2_000), [.goalComplete])
    XCTAssertEqual(evaluate(&state, config, distanceM: 2_500), [])
    XCTAssertEqual(evaluate(&state, config, distanceM: 9_000), [])
  }

  func testACustomSpacingChangesWhereTheMarksLand() {
    var state = MavMilestones.State()
    let config = distanceConfig(km: 20, every: 5)

    XCTAssertEqual(evaluate(&state, config, distanceM: 4_999), [])
    XCTAssertEqual(evaluate(&state, config, distanceM: 5_000), [.milestone])
  }

  // MARK: Time

  func testTimeHalfwayFiresOnceAtTheMidpoint() {
    var state = MavMilestones.State()
    let config = MavMilestones.Config(goal: WorkoutGoal(kind: .time, value: 30))

    XCTAssertEqual(evaluate(&state, config, elapsed: 14 * 60), [])
    XCTAssertEqual(evaluate(&state, config, elapsed: 15 * 60), [.milestone])
    XCTAssertEqual(evaluate(&state, config, elapsed: 16 * 60), [])
    XCTAssertEqual(evaluate(&state, config, elapsed: 30 * 60), [.goalComplete])
  }

  func testTimeIntervalModeMarksEveryTenMinutes() {
    var state = MavMilestones.State()
    let config = MavMilestones.Config(
      goal: WorkoutGoal(kind: .time, value: 60), timeMode: .every10)

    XCTAssertEqual(evaluate(&state, config, elapsed: 599), [])
    XCTAssertEqual(evaluate(&state, config, elapsed: 600), [.milestone])
    XCTAssertEqual(evaluate(&state, config, elapsed: 1_200), [.milestone])
  }

  func testTimeModeOffSilencesInterimsButNotTheGoal() {
    var state = MavMilestones.State()
    let config = MavMilestones.Config(goal: WorkoutGoal(kind: .time, value: 20), timeMode: .off)

    XCTAssertEqual(evaluate(&state, config, elapsed: 10 * 60), [])
    XCTAssertEqual(evaluate(&state, config, elapsed: 20 * 60), [.goalComplete])
  }

  // MARK: Calories

  func testCalorieHalfwayAndGoal() {
    var state = MavMilestones.State()
    let config = MavMilestones.Config(goal: WorkoutGoal(kind: .calories, value: 400))

    XCTAssertEqual(evaluate(&state, config, kcal: 199), [])
    XCTAssertEqual(evaluate(&state, config, kcal: 200), [.milestone])
    XCTAssertEqual(evaluate(&state, config, kcal: 400), [.goalComplete])
  }

  func testCalorieIntervalModeMarksEveryFifty() {
    var state = MavMilestones.State()
    let config = MavMilestones.Config(
      goal: WorkoutGoal(kind: .calories, value: 500), calorieMode: .every50)

    XCTAssertEqual(evaluate(&state, config, kcal: 49), [])
    XCTAssertEqual(evaluate(&state, config, kcal: 50), [.milestone])
    XCTAssertEqual(evaluate(&state, config, kcal: 100), [.milestone])
  }

  // MARK: Zone target

  func testTheZoneTargetFiresOnceWhenTheTimeIsBanked() {
    var state = MavMilestones.State()
    let config = MavMilestones.Config(zoneTarget: WorkoutZoneTarget(zone: 2, minutes: 15))

    XCTAssertEqual(evaluate(&state, config, zoneSeconds: [0, 14 * 60, 0, 0, 0]), [])
    XCTAssertEqual(evaluate(&state, config, zoneSeconds: [0, 15 * 60, 0, 0, 0]), [.zoneTargetMet])
    XCTAssertEqual(evaluate(&state, config, zoneSeconds: [0, 40 * 60, 0, 0, 0]), [])
  }

  func testAZoneTargetIsIgnoredWhenThatZoneHasNoReading() {
    var state = MavMilestones.State()
    let config = MavMilestones.Config(zoneTarget: WorkoutZoneTarget(zone: 5, minutes: 1))

    // Three zones reported, target names the fifth. Nothing fires, and nothing crashes.
    XCTAssertEqual(evaluate(&state, config, zoneSeconds: [600, 600, 600]), [])
    XCTAssertFalse(state.zoneTargetFired)
  }

  // MARK: Signals

  func testEventsMapOntoTheClosedHapticVocabulary() {
    XCTAssertEqual(MavMilestones.Event.milestone.signal, .milestone)
    XCTAssertEqual(MavMilestones.Event.zoneTargetMet.signal, .milestone)
    XCTAssertEqual(MavMilestones.Event.goalComplete.signal, .goalComplete)
  }

  // MARK: Progress

  func testProgressIsNilWithoutAGoalAndClampsWithOne() {
    XCTAssertNil(MavMilestones.progress(.none, elapsedSec: 60, distanceM: 500, kcal: 10))

    let goal = WorkoutGoal(kind: .distance, value: 4)
    XCTAssertEqual(
      MavMilestones.progress(goal, elapsedSec: 0, distanceM: 1_000, kcal: 0) ?? 0, 0.25,
      accuracy: 0.0001)
    XCTAssertEqual(
      MavMilestones.progress(goal, elapsedSec: 0, distanceM: 9_000, kcal: 0) ?? 0, 1.0,
      accuracy: 0.0001)
  }
}
