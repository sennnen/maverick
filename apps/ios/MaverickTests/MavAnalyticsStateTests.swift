import XCTest

@testable import Mav

/// The states a wearer can be shown, tested where no device is needed to know the answer.
///
/// The Android twin is `AnalyticsStateTest.kt` and asserts the same things, because the same core
/// plan reaches both: a wearer told "needs a strap that reports SpO2" on Android and shown an
/// empty card on iOS is being told two different things about the same data.
final class MavAnalyticsStateTests: XCTestCase {

  private func stage(
    _ model: String,
    signal: String = "cardiovascular",
    state: MavStageState = .ready,
    displayable: Bool = true,
    unavailable: MavUnavailable? = nil
  ) -> MavPlannedStage {
    MavPlannedStage(
      model: model,
      signal: signal,
      state: state,
      displayable: displayable,
      unavailable: unavailable
    )
  }

  func testWorkOutstandingReportsHowFarItHasGot() {
    let signals = MavSignalReducer.reduce(stages: [
      stage("cva_encoder", state: .cached),
      stage("cva_probes_male", state: .ready),
    ])
    XCTAssertEqual(signals.first?.state, .working(done: 1, total: 2))
  }

  func testAFinishedSignalCarriesWhenItFinished() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("cva_encoder", state: .cached)],
      completedAtMs: ["cva_encoder": 1_700_000_000_000]
    )
    XCTAssertEqual(signals.first?.state, .ready(atMs: 1_700_000_000_000, displayable: true))
  }

  func testNewDataMarksASignalStaleRatherThanBlankingIt() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("cva_encoder", state: .cached)],
      completedAtMs: ["cva_encoder": 42],
      invalidated: ["cva_encoder"]
    )
    XCTAssertEqual(signals.first?.state, .stale(atMs: 42, displayable: true))
  }

  /// The distinction that matters most: a missing sensor sends someone shopping, a missing
  /// profile field is one tap, and an unported front-end is neither.
  func testTheFourUnavailableCausesStayDistinguishable() {
    let signals = MavSignalReducer.reduce(stages: [
      stage("a", signal: "s", state: .unavailable, unavailable: .missingStreams(["spo2_percent"])),
      stage("b", signal: "s", state: .unavailable, unavailable: .missingProfile(["age"])),
      stage("c", signal: "s", state: .unavailable, unavailable: .upstreamUnavailable("cva_encoder")),
      stage("d", signal: "s", state: .unavailable, unavailable: .preprocessingNotPorted("the 77 features")),
    ])
    guard case let .unavailable(reasons) = signals.first?.state else {
      return XCTFail("expected unavailable, got \(String(describing: signals.first?.state))")
    }
    XCTAssertEqual(reasons.count, 4)
  }

  func testRepeatedCausesAreStatedOnce() {
    let signals = MavSignalReducer.reduce(stages: [
      stage("sleepnet_bdi", signal: "sleep", state: .unavailable,
            unavailable: .preprocessingNotPorted("the per-epoch ibi channel")),
      stage("sleepnet_bdi_v3", signal: "sleep", state: .unavailable,
            unavailable: .preprocessingNotPorted("the per-epoch ibi channel")),
    ])
    guard case let .unavailable(reasons) = signals.first?.state else {
      return XCTFail("expected unavailable")
    }
    XCTAssertEqual(reasons.count, 1, "two stages with one cause should say it once")
  }

  func testAComputedButUninterpretableSignalSaysSoRatherThanShowingANumber() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("sleepnet_bdi", signal: "sleep", state: .cached, displayable: false)],
      completedAtMs: ["sleepnet_bdi": 7]
    )
    XCTAssertEqual(signals.first?.state, .ready(atMs: 7, displayable: false))
  }

  func testAStageThatExhaustsItsRetriesOffersTheButton() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("cva_encoder")],
      failures: ["cva_encoder": MavSignalReducer.retryBudget]
    )
    guard case let .failed(_, attempts, retryable) = signals.first?.state else {
      return XCTFail("expected failed")
    }
    XCTAssertTrue(retryable)
    XCTAssertEqual(attempts, MavSignalReducer.retryBudget)
  }

  func testAStageInsideItsBudgetKeepsWorking() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("cva_encoder")],
      failures: ["cva_encoder": MavSignalReducer.retryBudget - 1]
    )
    XCTAssertEqual(signals.first?.state, .working(done: 0, total: 1))
  }

  func testADeferredPassSaysItIsWaitingRatherThanThatItFailed() {
    let signals = MavSignalReducer.reduce(stages: [stage("cva_encoder")], deferred: true)
    XCTAssertEqual(signals.first?.state, .deferred)
  }

  func testASignalThatFinishedBeforeTheOsDeferredIsStillFinished() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("cva_encoder", state: .cached)],
      deferred: true
    )
    XCTAssertEqual(signals.first?.state, .ready(atMs: 0, displayable: true))
  }

  func testAMissingPermissionOutranksEveryOtherExplanation() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("cva_encoder")],
      missingPermission: "bluetooth"
    )
    XCTAssertEqual(signals.first?.state, .permissionRequired("bluetooth"))
  }

  func testCoverageCountsRunnableAgainstTotal() {
    let signals = MavSignalReducer.reduce(stages: [
      stage("a", signal: "s", state: .cached),
      stage("b", signal: "s", state: .unavailable, unavailable: .missingStreams(["ppg"])),
    ])
    XCTAssertEqual(signals.first?.total, 2)
    XCTAssertEqual(signals.first?.runnable, 1)
  }

  /// The core counts coverage on every plan so that two platforms do not each write the same
  /// loop. This proves the platform actually reads it rather than quietly recounting: the figures
  /// below are deliberately not what counting the group would produce.
  func testTheCoresOwnCoverageIsUsedRatherThanRecounted() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("a", signal: "s", state: .cached)],
      coverage: ["s": MavSignalCoverage(total: 9, runnable: 4)]
    )
    XCTAssertEqual(signals.first?.total, 9)
    XCTAssertEqual(signals.first?.runnable, 4)
  }

  /// Every signal the core can plan has a written-out name. Title-casing the slug instead gives
  /// "Daytime Hrv" and "Ppg Foundation", which is how a product surface starts looking generated.
  func testEverySignalHasCopyRatherThanATitleCasedSlug() {
    let planned = [
      "activity", "energy_expenditure", "step_eligibility", "awake_heart_rate", "daytime_hrv",
      "workout_heart_rate", "cardiovascular", "hypertension_risk", "sleep", "illness_risk",
      "cycle_awareness", "ppg_foundation",
    ]
    for slug in planned {
      let title = MavSignalCopy.title(slug)
      XCTAssertFalse(
        title.contains("_"),
        "\(slug) fell through to the derived form, so its copy is missing"
      )
      XCTAssertNotEqual(
        title,
        slug.replacingOccurrences(of: "_", with: " ").capitalized,
        "\(slug) has no written-out name"
      )
    }
    // An unknown slug is still legible rather than blank.
    XCTAssertEqual(MavSignalCopy.title("a_new_signal"), "A New Signal")
  }

  /// Signals keep the order the core planned them in. A surface that reshuffled between passes
  /// would move a card out from under a finger mid-tap.
  func testSignalOrderFollowsThePlanRatherThanADictionary() {
    let signals = MavSignalReducer.reduce(stages: [
      stage("a", signal: "ppg_foundation", state: .cached),
      stage("b", signal: "cardiovascular", state: .cached),
      stage("c", signal: "sleep", state: .cached),
      stage("d", signal: "cardiovascular", state: .cached),
    ])
    XCTAssertEqual(signals.map(\.name), ["ppg_foundation", "cardiovascular", "sleep"])
  }
}
