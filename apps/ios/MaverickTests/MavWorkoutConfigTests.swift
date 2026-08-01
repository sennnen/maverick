import XCTest

@testable import Mav

/// The confirm screen's pure half, and the sport catalogue behind it. The Android twin is
/// `MavWorkoutConfigTest.kt` and asserts the same numbers, because a catalogue that lists a sport on
/// one phone and not the other is a parity break the shared core cannot catch.
final class MavWorkoutConfigTests: XCTestCase {

  // MARK: Catalogue

  func testTheCatalogueHasSixCategoriesAndEverySportNameIsUnique() {
    XCTAssertEqual(MavSportCatalog.categories.count, 6)
    XCTAssertEqual(MavSportCatalog.all.count, 18)
    XCTAssertEqual(
      Set(MavSportCatalog.all.map(\.name)).count, 18,
      "a sport name is duplicated, so the sticky-config key would collide")
    XCTAssertTrue(MavSportCatalog.all.allSatisfy { !$0.detail.isEmpty })
    XCTAssertTrue(MavSportCatalog.all.allSatisfy { !$0.systemImage.isEmpty })
  }

  func testExactlyOneSportIsStrengthAndItIsTheOneTheLoggerOpensFor() {
    let strength = MavSportCatalog.all.filter(\.isStrength)
    XCTAssertEqual(strength.count, 1)
    XCTAssertEqual(strength.first?.name, "Strength training")
    // Strength has no route: it must not offer GPS, and it never reaches the confirm screen.
    XCTAssertEqual(strength.first?.isDistance, false)
  }

  func testOnlySportsWithARouteAreDistanceSports() {
    let distance = Set(MavSportCatalog.all.filter(\.isDistance).map(\.name))
    XCTAssertEqual(distance, ["Outdoor run", "Walking", "Hiking", "Cycling"])
    // A treadmill reports no route, so offering a route map on it would be a lie.
    XCTAssertEqual(MavSportCatalog.sport(named: "Treadmill")?.isDistance, false)
    XCTAssertEqual(MavSportCatalog.sport(named: "Rowing")?.isDistance, false)
  }

  func testAnUnknownSportNameResolvesToNothingRatherThanAGuess() {
    XCTAssertNil(MavSportCatalog.sport(named: "Underwater basket weaving"))
  }

  // MARK: Goal defaults and display

  func testMetricAndImperialDistanceDefaults() {
    XCTAssertEqual(MavGoalText.defaultValue(.distance, isImperial: false), 5, accuracy: 0.0001)
    // Three miles, stored in kilometres.
    XCTAssertEqual(MavGoalText.defaultValue(.distance, isImperial: true), 4.828, accuracy: 0.001)
    XCTAssertEqual(MavGoalText.defaultValue(.time, isImperial: false), 30, accuracy: 0.0001)
    XCTAssertEqual(MavGoalText.defaultValue(.calories, isImperial: false), 300, accuracy: 0.0001)
    XCTAssertEqual(MavGoalText.defaultValue(.none, isImperial: false), 0, accuracy: 0.0001)
  }

  func testDisplayTextConvertsToMilesButTheStoredValueStaysKilometres() {
    let fiveKm = WorkoutGoal(kind: .distance, value: 5)
    XCTAssertEqual(MavGoalText.display(fiveKm, isImperial: false), "5")
    XCTAssertEqual(MavGoalText.display(fiveKm, isImperial: true), "3.1")
    // The stored value is untouched by how it was shown.
    XCTAssertEqual(fiveKm.value, 5, accuracy: 0.0001)
  }

  func testAWholeNumberDropsItsDecimalAndAFractionKeepsOne() {
    XCTAssertEqual(MavGoalText.display(WorkoutGoal(kind: .time, value: 30), isImperial: false), "30")
    XCTAssertEqual(
      MavGoalText.display(WorkoutGoal(kind: .time, value: 7.5), isImperial: false), "7.5")
  }

  func testAnInactiveGoalHasNoDisplayText() {
    XCTAssertEqual(MavGoalText.display(.none, isImperial: false), "")
    // Zero is not a goal, whatever kind it claims.
    XCTAssertEqual(MavGoalText.display(WorkoutGoal(kind: .time, value: 0), isImperial: false), "")
  }

  func testTheUnitLabelFollowsTheKind() {
    XCTAssertEqual(MavGoalText.unit(.distance, distanceUnit: "km"), "km")
    XCTAssertEqual(MavGoalText.unit(.distance, distanceUnit: "mi"), "mi")
    XCTAssertEqual(MavGoalText.unit(.time, distanceUnit: "km"), "min")
    XCTAssertEqual(MavGoalText.unit(.calories, distanceUnit: "km"), "kcal")
    XCTAssertEqual(MavGoalText.unit(.none, distanceUnit: "km"), "")
  }

  // MARK: Goal activity

  func testAGoalNeedsBothAKindAndAPositiveValueToBeActive() {
    XCTAssertFalse(WorkoutGoal.none.isActive)
    XCTAssertFalse(WorkoutGoal(kind: .distance, value: 0).isActive)
    XCTAssertFalse(WorkoutGoal(kind: .none, value: 5).isActive)
    XCTAssertTrue(WorkoutGoal(kind: .distance, value: 5).isActive)
  }

  // MARK: Sticky-config keys

  func testSportNamesSlugIntoStableKeys() {
    XCTAssertEqual(ExerciseLibrary.slug("Outdoor run"), "outdoor-run")
    XCTAssertEqual(ExerciseLibrary.slug("Strength training"), "strength-training")
    XCTAssertEqual(ExerciseLibrary.slug("Mind & body"), "mind-body")
    XCTAssertEqual(ExerciseLibrary.slug("Other activity"), "other-activity")
  }

  func testEveryCatalogueSportSlugsToSomethingDistinct() {
    let slugs = MavSportCatalog.all.map { ExerciseLibrary.slug($0.name) }
    XCTAssertEqual(
      Set(slugs).count, slugs.count,
      "two sports share a preferences key, so one would overwrite the other")
    XCTAssertTrue(slugs.allSatisfy { !$0.isEmpty })
  }

  // MARK: Sticky round trip

  func testAConfigSurvivesASaveAndLoadPerSport() throws {
    let defaults = try XCTUnwrap(UserDefaults(suiteName: #function))
    defaults.removePersistentDomain(forName: #function)

    var config = WorkoutConfig()
    config.goal = WorkoutGoal(kind: .distance, value: 8)
    config.zoneTarget = WorkoutZoneTarget(zone: 3, minutes: 25)
    config.gpsEnabled = false
    config.keepScreenOn = true
    WorkoutPrefs.save(config, for: "Outdoor run", defaults: defaults)

    let loaded = WorkoutPrefs.config(for: "Outdoor run", defaults: defaults)
    XCTAssertEqual(loaded, config)

    // A different sport keeps its own settings rather than inheriting the last one used.
    XCTAssertEqual(WorkoutPrefs.config(for: "Cycling", defaults: defaults), WorkoutConfig())
  }
}
