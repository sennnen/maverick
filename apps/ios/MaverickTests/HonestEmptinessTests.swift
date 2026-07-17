import XCTest
@testable import Mav

/// PL-P7 leakage guard: a live session snapshot (HR, PRV) must never surface as Strain or Sleep
/// values. Those hubs read only the repository's day/workout/sleep facades, which stay empty until
/// the core serves the matching read models.
@MainActor
final class HonestEmptinessTests: XCTestCase {
  func testStrainAndSleepInputsStayEmptyUntilTheCoreServesThem() async {
    let repo = Repository()
    XCTAssertTrue(repo.days.isEmpty)
    XCTAssertTrue(repo.sleeps.isEmpty)
    XCTAssertTrue(repo.importedSleep.isEmpty)

    let workouts = await repo.workoutRows()
    XCTAssertTrue(workouts.isEmpty)
    let strainSeries = await repo.exploreSeries(key: "strain", source: Repository.whoopSource)
    XCTAssertTrue(strainSeries.isEmpty)
  }
}
