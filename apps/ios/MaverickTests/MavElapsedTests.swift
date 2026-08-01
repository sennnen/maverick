import XCTest

@testable import Mav

/// The live session's clock.
///
/// Both platforms formatted elapsed time as `mm:ss` with no hour rollover, so a ninety-minute
/// session read "90:00" and a two-hour one read "120:00". The Android twin is `MavElapsedTest.kt`
/// and asserts the same boundaries, because a clock that is right on one phone and wrong on the
/// other is worse than one that is wrong on both.
final class MavElapsedTests: XCTestCase {

  func testUnderAnHourStaysMinutesAndSeconds() {
    XCTAssertEqual(MavElapsed.format(0), "00:00")
    XCTAssertEqual(MavElapsed.format(9), "00:09")
    XCTAssertEqual(MavElapsed.format(60), "01:00")
    XCTAssertEqual(MavElapsed.format(3_599), "59:59")
  }

  func testTheHourBoundaryRollsOver() {
    XCTAssertEqual(MavElapsed.format(3_600), "1:00:00")
    XCTAssertEqual(MavElapsed.format(5_400), "1:30:00")
    XCTAssertEqual(MavElapsed.format(7_201), "2:00:01")
    // The regression itself: this used to render "90:00".
    XCTAssertEqual(MavElapsed.format(90 * 60), "1:30:00")
  }

  func testMinutesAndSecondsStayPaddedInsideAnHourReading() {
    XCTAssertEqual(MavElapsed.format(3_600 + 5 * 60 + 7), "1:05:07")
  }

  func testSpokenFormIsADurationAndSingularisesCorrectly() {
    XCTAssertEqual(MavElapsed.spoken(0), "0 seconds")
    XCTAssertEqual(MavElapsed.spoken(1), "1 second")
    XCTAssertEqual(MavElapsed.spoken(2), "2 seconds")
    XCTAssertEqual(MavElapsed.spoken(60), "1 minute 0 seconds")
    XCTAssertEqual(MavElapsed.spoken(123), "2 minutes 3 seconds")
    XCTAssertEqual(MavElapsed.spoken(3_600), "1 hour 0 seconds")
    XCTAssertEqual(MavElapsed.spoken(5_400), "1 hour 30 minutes 0 seconds")
    XCTAssertEqual(MavElapsed.spoken(7_265), "2 hours 1 minute 5 seconds")
  }
}
