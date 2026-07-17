import XCTest
@testable import Mav

@MainActor
final class AppModelApplyTests: XCTestCase {
  private func snapshot(
    connectionState: String,
    currentBpm: Int? = 72,
    batteryPercent: Int? = nil,
    charging: Bool? = nil
  ) -> MavSnapshot {
    MavSnapshot(
      coreVersion: "0.1.0",
      storageSchema: 1,
      revision: 1,
      asOfUnixMs: 1_752_600_500_000,
      connectionState: connectionState,
      deviceName: "MG",
      batteryPercent: batteryPercent,
      charging: charging,
      lastSampleUnixMs: 1_752_600_500_000,
      currentBpm: currentBpm,
      meanMilliBpm: 72_000,
      inRangeSamples: 1,
      excludedSamples: 0,
      prv: nil,
      prvUnavailableReason: nil,
      recoveryUnavailableReason: "Recovery model not admitted",
      hash: "abc"
    )
  }

  func testStreamingLinkIsConnectedAndCarriesTheLiveReadout() {
    let model = AppModel()
    let live = LiveState()
    model.apply(snapshot: snapshot(connectionState: "streaming", batteryPercent: 81, charging: false), to: live)

    XCTAssertTrue(live.connected)
    XCTAssertTrue(live.bonded)
    XCTAssertEqual(live.heartRate, 72)
    XCTAssertEqual(live.batteryPct, 81)
    XCTAssertEqual(live.charging, false)
    XCTAssertEqual(live.advertisingName, "MG")
    XCTAssertEqual(model.bpm, 72)
    XCTAssertEqual(model.recoveryUnavailableReason, "Recovery model not admitted")
  }

  func testSubscribingCountsAsConnected() {
    let model = AppModel()
    let live = LiveState()
    model.apply(snapshot: snapshot(connectionState: "subscribing"), to: live)
    XCTAssertTrue(live.connected)
  }

  func testAStoredHeartRateDoesNotOutliveTheLink() {
    let model = AppModel()
    let live = LiveState()
    model.apply(snapshot: snapshot(connectionState: "streaming", batteryPercent: 81), to: live)
    model.apply(snapshot: snapshot(connectionState: "disconnected"), to: live)

    XCTAssertFalse(live.connected)
    XCTAssertNil(live.heartRate)
    XCTAssertNil(live.batteryPct)
    XCTAssertNil(model.bpm)
  }
}
