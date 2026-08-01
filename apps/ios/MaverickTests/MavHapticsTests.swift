import XCTest

@testable import Mav

/// The haptic vocabulary (ADR-032). The Android twin is `MavHapticsTest.kt`.
///
/// The wire names matter more than they look: a manifest declares them and the host snapshot lists
/// them, so a rename here silently stops a connector's declaration from matching and the feature
/// quietly disappears. They are asserted literally for that reason.
final class MavHapticsTests: XCTestCase {

  func testWireNamesAreExactlyTheVocabularyADR032Fixed() {
    XCTAssertEqual(MavHapticSignal.milestone.id, "milestone")
    XCTAssertEqual(MavHapticSignal.goalComplete.id, "goal_complete")
    XCTAssertEqual(MavHapticSignal.setLogged.id, "set_logged")
    XCTAssertEqual(MavHapticSignal.restComplete.id, "rest_complete")
    XCTAssertEqual(MavHapticSignal.zoneAlert(zone: 1).id, "zone_alert_1")
    XCTAssertEqual(MavHapticSignal.zoneAlert(zone: 5).id, "zone_alert_5")
  }

  func testTheVocabularyIsNineSignalsAndEveryNameIsDistinct() {
    let all = MavHapticSignal.allCases
    XCTAssertEqual(all.count, 9)
    XCTAssertEqual(Set(all.map(\.id)).count, 9, "a signal name is duplicated")
    XCTAssertTrue(all.allSatisfy { !$0.explanation.isEmpty }, "a signal has no explanation")
  }

  func testNothingDeclaredMeansNothingIsSupported() {
    let support = MavHapticSupport.none
    XCTAssertFalse(support.canBuzz)
    for signal in MavHapticSignal.allCases {
      XCTAssertFalse(
        support.supports(signal), "\(signal.id) claimed support with an empty declaration")
    }
  }

  func testAPartialDeclarationSupportsExactlyWhatItNamed() {
    // A strap that can tap but not run a five-zone pattern is a real shape, not a hypothetical.
    let support = MavHapticSupport(signals: ["milestone", "goal_complete"])
    XCTAssertTrue(support.canBuzz)
    XCTAssertTrue(support.supports(.milestone))
    XCTAssertTrue(support.supports(.goalComplete))
    XCTAssertFalse(support.supports(.setLogged))
    XCTAssertFalse(support.supports(.zoneAlert(zone: 3)))
  }

  func testTheUnavailableReasonDistinguishesNoStrapFromAStrapThatCannotBuzz() {
    let support = MavHapticSupport.none
    XCTAssertEqual(support.reason(deviceName: nil), "No strap is connected, so there is nothing to buzz.")
    XCTAssertEqual(support.reason(deviceName: ""), "No strap is connected, so there is nothing to buzz.")
    XCTAssertEqual(
      support.reason(deviceName: "WHOOP 4.0"),
      "WHOOP 4.0 does not report a haptic motor, so it cannot buzz.")
  }
}
