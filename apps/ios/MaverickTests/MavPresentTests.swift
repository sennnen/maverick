import XCTest
@testable import Mav

final class MavPresentTests: XCTestCase {
  private let asOf: Int64 = 1_752_600_500_000

  func testAFreshStreamingSampleShowsNoLabel() {
    XCTAssertNil(MavPresent.sampleAgeLabel(asOfUnixMs: asOf, lastSampleUnixMs: asOf - 10_000, connected: true))
    XCTAssertNil(MavPresent.sampleAgeLabel(
      asOfUnixMs: asOf, lastSampleUnixMs: asOf - MavPresent.freshSampleMs, connected: true))
  }

  func testAStaleStreamingSampleIsVisiblyStale() {
    XCTAssertEqual(
      MavPresent.sampleAgeLabel(asOfUnixMs: asOf, lastSampleUnixMs: asOf - 20_000, connected: true),
      "Last sample 20 s ago")
  }

  func testADisconnectedSnapshotAlwaysCarriesItsAge() {
    XCTAssertEqual(
      MavPresent.sampleAgeLabel(asOfUnixMs: asOf, lastSampleUnixMs: asOf - 5_000, connected: false),
      "Last sample 5 s ago")
    XCTAssertEqual(
      MavPresent.sampleAgeLabel(asOfUnixMs: asOf, lastSampleUnixMs: asOf - 90_000, connected: false),
      "Last sample 1 m ago")
    XCTAssertEqual(
      MavPresent.sampleAgeLabel(asOfUnixMs: asOf, lastSampleUnixMs: asOf - 3 * 3_600_000, connected: false),
      "Last sample 3 h ago")
    XCTAssertEqual(
      MavPresent.sampleAgeLabel(asOfUnixMs: asOf, lastSampleUnixMs: asOf - 2 * 86_400_000, connected: false),
      "Last sample 2 d ago")
  }

  func testNoSamplesShowWaitingOnlyWhileTheLinkIsUp() {
    XCTAssertEqual(
      MavPresent.sampleAgeLabel(asOfUnixMs: asOf, lastSampleUnixMs: nil, connected: true),
      "Waiting for first sample")
    XCTAssertNil(MavPresent.sampleAgeLabel(asOfUnixMs: asOf, lastSampleUnixMs: nil, connected: false))
  }

  func testFixedPointDisplayConversionsAreExact() {
    let us = Locale(identifier: "en_US")
    XCTAssertEqual(MavPresent.microsAsMs(67_454, locale: us), "67.5 ms")
    XCTAssertEqual(MavPresent.microsAsMs(828_000, locale: us), "828.0 ms")
    XCTAssertEqual(MavPresent.milliPercentAsPercent(50_000, locale: us), "50.0%")
  }
}
