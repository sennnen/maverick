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

  private func health(
    _ model: String,
    _ applicability: MavApplicability,
    _ substitutions: [String] = []
  ) -> (String, MavStageHealth) {
    (model, MavStageHealth(model: model, applicability: applicability, substitutions: substitutions))
  }

  /// The case the whole health path exists for: every stage answered, and answered about padding.
  /// It must not arrive as `.ready`, because a surface switching on `.ready` to draw a number
  /// would draw one.
  func testSignalComputedEntirelyFromPaddingIsUnfoundedNotReady() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("popsicle_ovulation_detection", signal: "cycle_awareness", state: .cached)],
      completedAtMs: ["popsicle_ovulation_detection": 1_000],
      health: Dictionary(uniqueKeysWithValues: [
        health("popsicle_ovulation_detection", .unfounded, ["out_of_range"])
      ])
    )
    XCTAssertEqual(signals.first?.state, .unfounded(atMs: 1_000, substitutions: ["out_of_range"]))
  }

  func testPartlySubstitutedSignalIsReadyAndCarriesTheQualification() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("cva_encoder", state: .cached)],
      completedAtMs: ["cva_encoder": 5],
      health: Dictionary(uniqueKeysWithValues: [health("cva_encoder", .degraded, ["padded"])])
    )
    XCTAssertEqual(signals.first?.state, .ready(atMs: 5, displayable: true, applicability: .degraded))
  }

  /// A signal is only as sound as its weakest stage.
  func testSignalTakesTheWorstVerdictAmongItsStages() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("cva_encoder", state: .cached), stage("cva_probes_male", state: .cached)],
      completedAtMs: ["cva_encoder": 9, "cva_probes_male": 9],
      health: Dictionary(uniqueKeysWithValues: [
        health("cva_encoder", .sound),
        health("cva_probes_male", .unfounded, ["missing"]),
      ])
    )
    guard case .unfounded = signals.first?.state else {
      return XCTFail("expected unfounded, got \(String(describing: signals.first?.state))")
    }
  }

  func testUnmeasuredStageDoesNotDegradeASoundSignal() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("cva_encoder", state: .cached)],
      completedAtMs: ["cva_encoder": 3]
    )
    XCTAssertEqual(signals.first?.state, .ready(atMs: 3, displayable: true, applicability: .sound))
  }

  func testWorstVerdictRanksUnfoundedAboveDegradedAboveUnmeasured() {
    XCTAssertEqual(MavApplicability.worst([]), .sound)
    XCTAssertEqual(MavApplicability.worst([.sound, .degraded, .unfounded]), .unfounded)
    XCTAssertEqual(MavApplicability.worst([.sound, .degraded, .unmeasured]), .degraded)
  }

  /// An unknown wire name must never be read as the flattering answer.
  func testUnrecognisedVerdictParsesAsUnmeasuredRatherThanSound() {
    XCTAssertEqual(MavApplicability.parse("sound"), .sound)
    XCTAssertEqual(MavApplicability.parse("degraded"), .degraded)
    XCTAssertEqual(MavApplicability.parse("unfounded"), .unfounded)
    XCTAssertEqual(MavApplicability.parse("unmeasured"), .unmeasured)
    XCTAssertEqual(MavApplicability.parse("a_verdict_from_a_newer_core"), .unmeasured)
  }

  /// An unfounded verdict outranks staleness: the reading was never founded to go stale.
  func testUnfoundedOutranksStale() {
    let signals = MavSignalReducer.reduce(
      stages: [stage("cva_encoder", state: .cached)],
      completedAtMs: ["cva_encoder": 2],
      invalidated: ["cva_encoder"],
      health: Dictionary(uniqueKeysWithValues: [health("cva_encoder", .unfounded, ["missing"])])
    )
    guard case .unfounded = signals.first?.state else {
      return XCTFail("expected unfounded, got \(String(describing: signals.first?.state))")
    }
  }
}
