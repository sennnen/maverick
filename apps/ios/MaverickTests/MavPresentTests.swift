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

  func testAnIdleOrUnknownSyncShowsNothing() {
    XCTAssertNil(MavPresent.syncProgressLabel(
      state: "historical_idle", recordsSeen: 0, recordsInserted: 0, duplicates: 0, failureCode: nil))
    XCTAssertNil(MavPresent.syncProgressLabel(
      state: "something_new", recordsSeen: 5, recordsInserted: 5, duplicates: 0, failureCode: nil))
  }

  func testPreparingStatesShareOneLabel() {
    XCTAssertEqual(
      MavPresent.syncProgressLabel(
        state: "historical_awaiting_range", recordsSeen: 0, recordsInserted: 0, duplicates: 0,
        failureCode: nil),
      "Preparing history sync")
    XCTAssertEqual(
      MavPresent.syncProgressLabel(
        state: "historical_awaiting_send_acceptance", recordsSeen: 0, recordsInserted: 0,
        duplicates: 0, failureCode: nil),
      "Preparing history sync")
  }

  func testReceivingCountsTheRecordsSeen() {
    XCTAssertEqual(
      MavPresent.syncProgressLabel(
        state: "historical_receiving", recordsSeen: 0, recordsInserted: 0, duplicates: 0,
        failureCode: nil),
      "Syncing history")
    XCTAssertEqual(
      MavPresent.syncProgressLabel(
        state: "historical_receiving", recordsSeen: 1, recordsInserted: 0, duplicates: 0,
        failureCode: nil),
      "Syncing history — 1 record")
    XCTAssertEqual(
      MavPresent.syncProgressLabel(
        state: "historical_awaiting_durable_commit", recordsSeen: 7, recordsInserted: 2,
        duplicates: 0, failureCode: nil),
      "Syncing history — 7 records")
  }

  func testCompletionSummarizesInsertedAndDuplicates() {
    XCTAssertEqual(
      MavPresent.syncProgressLabel(
        state: "historical_complete", recordsSeen: 0, recordsInserted: 0, duplicates: 0,
        failureCode: nil),
      "History synced")
    XCTAssertEqual(
      MavPresent.syncProgressLabel(
        state: "historical_complete", recordsSeen: 15, recordsInserted: 12, duplicates: 3,
        failureCode: nil),
      "History synced — 12 new, 3 duplicate")
  }

  func testFailureCarriesTheStableCode() {
    XCTAssertEqual(
      MavPresent.syncProgressLabel(
        state: "historical_failed", recordsSeen: 7, recordsInserted: 0, duplicates: 0,
        failureCode: 5004),
      "History sync failed (MAV-5004)")
    XCTAssertEqual(
      MavPresent.syncProgressLabel(
        state: "historical_failed", recordsSeen: 0, recordsInserted: 0, duplicates: 0,
        failureCode: nil),
      "History sync failed")
  }
}
