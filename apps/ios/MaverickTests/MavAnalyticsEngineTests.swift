import XCTest

@testable import Mav

/// The production loop, driven against a queue this test controls.
///
/// The real runtime needs a database, a compiled core and a compiled model, none of which a unit
/// test has. What is worth testing is not the inference — that is covered on device — but the
/// loop around it: that a pass drains until the queue empties, that a second pass does not run
/// the zoo twice, and that the clock the core is told about is the platform's.
final class MavAnalyticsEngineTests: XCTestCase {

  private final class FakeHost: MavModelBridge.Host, @unchecked Sendable {
    var pending: [String]
    var submitted: [(UInt64, Int64)] = []
    var cancelled: [UInt64] = []
    private var nextId: UInt64 = 1

    init(_ pending: [String]) { self.pending = pending }

    func nextModelInference() throws -> ModelInferenceRequest? {
      guard !pending.isEmpty else { return nil }
      let slug = pending.removeFirst()
      defer { nextId += 1 }
      return ModelInferenceRequest(
        requestId: nextId,
        modelSlug: slug,
        inputs: [ModelTensor(name: "in", values: [0.5])]
      )
    }

    func submitModelInference(
      requestId: UInt64,
      outputs: [ModelTensor],
      modelSha256: String,
      completedAtMs: Int64
    ) throws -> ModelInferenceResult {
      submitted.append((requestId, completedAtMs))
      return ModelInferenceResult(
        requestId: requestId,
        modelSlug: "slug",
        outputs: outputs,
        modelSha256: modelSha256
      )
    }

    func cancelModelInference(requestId: UInt64) throws -> Bool {
      cancelled.append(requestId)
      return true
    }
  }

  private final class FakeRunner: MavModelBridge.Runner, @unchecked Sendable {
    let failOn: String?
    private(set) var released = 0

    init(failOn: String?) { self.failOn = failOn }

    func run(slug: String, inputs: [String: [Float]]) throws -> [String: [Float]] {
      if slug == failOn { throw MavModelError.unknown("\(slug) is not in the bundle") }
      return ["out": [1.0]]
    }
    func admittedSHA256(for slug: String) throws -> String { String(repeating: "a", count: 64) }
    func releaseCache() { released += 1 }
  }

  private final class FakeRuntime: MavAnalyticsRuntime, @unchecked Sendable {
    let fakeHost: FakeHost
    var plan = MavPlan()
    var completedAt: [String: Int64] = [:]
    var admitCalls = 0

    init(_ host: FakeHost) { self.fakeHost = host }

    func host() -> MavModelBridge.Host { fakeHost }
    var health: [MavStageHealth] = []
    func admitPPGStages(deviceID: UInt64, atMs: Int64) throws -> [MavStageHealth] {
      admitCalls += 1
      return health
    }
    func plan(
      deviceID: UInt64,
      atMs: Int64,
      mode: MavAnalyticsEngine.RunMode,
      profileFields: [String]
    ) throws -> MavPlan { plan }
    func profileFields() -> [String] { ["sex", "age", "height", "weight"] }
    func cacheCompletedAt() throws -> [String: Int64] { completedAt }
  }

  private func run(
    _ engine: MavAnalyticsEngine,
    mode: MavAnalyticsEngine.RunMode = .interactive
  ) -> MavAnalyticsEngine.Outcome {
    let done = expectation(description: "pass")
    // The completion lands on the engine's queue and the value is read back here, so it travels
    // through a box with a lock rather than a captured `var`.
    let result = OutcomeBox()
    engine.runPass(deviceID: 1, mode: mode) { outcome in
      result.set(outcome)
      done.fulfill()
    }
    wait(for: [done], timeout: 5)
    return result.value
  }

  private final class OutcomeBox: @unchecked Sendable {
    private let lock = NSLock()
    private var outcome: MavAnalyticsEngine.Outcome = .failed

    func set(_ next: MavAnalyticsEngine.Outcome) {
      lock.lock()
      outcome = next
      lock.unlock()
    }

    var value: MavAnalyticsEngine.Outcome {
      lock.lock()
      defer { lock.unlock() }
      return outcome
    }
  }

  func testAPassDrainsUntilTheQueueIsEmpty() {
    let host = FakeHost(["pulse_ppg", "pulsenet_foundation", "cva_encoder"])
    let runtime = FakeRuntime(host)
    let engine = MavAnalyticsEngine(
      runtime: runtime,
      runner: FakeRunner(failOn: nil),
      clock: { 1_700_000_000_000 }
    )
    XCTAssertEqual(run(engine), .completed)
    XCTAssertEqual(host.submitted.count, 3)
    XCTAssertEqual(runtime.admitCalls, 1)
  }

  /// The core reads no clock. If the platform stopped sending one, the cache would file every
  /// result at the epoch and every reading would look decades stale.
  func testThePlatformClockTravelsWithEveryResult() {
    let host = FakeHost(["pulse_ppg"])
    let engine = MavAnalyticsEngine(
      runtime: FakeRuntime(host),
      runner: FakeRunner(failOn: nil),
      clock: { 1_234_567 }
    )
    _ = run(engine)
    XCTAssertEqual(host.submitted.map(\.1), [1_234_567])
  }

  func testAModelThatCannotRunIsCancelledRatherThanLeftInFlight() {
    let host = FakeHost(["pulse_ppg", "cva_encoder"])
    let engine = MavAnalyticsEngine(
      runtime: FakeRuntime(host),
      runner: FakeRunner(failOn: "pulse_ppg"),
      clock: { 0 }
    )
    XCTAssertEqual(run(engine), .partial)
    XCTAssertEqual(host.cancelled.count, 1, "the failed request must not stall the queue")
    XCTAssertEqual(host.submitted.count, 1, "the other model still ran")
  }

  func testAPassWithNothingQueuedCompletesWithoutRunningAnything() {
    let host = FakeHost([])
    let engine = MavAnalyticsEngine(
      runtime: FakeRuntime(host),
      runner: FakeRunner(failOn: nil),
      clock: { 0 }
    )
    XCTAssertEqual(run(engine, mode: .deferred), .completed)
    XCTAssertEqual(host.submitted.count, 0)
  }

  func testTheSnapshotStopsReportingWorkOnceAPassEnds() {
    let host = FakeHost(["pulse_ppg"])
    let engine = MavAnalyticsEngine(
      runtime: FakeRuntime(host),
      runner: FakeRunner(failOn: nil),
      clock: { 99 }
    )
    _ = run(engine)
    XCTAssertFalse(engine.snapshot.working)
    XCTAssertEqual(engine.snapshot.lastPassAtMs, 99)
  }

  /// `setTaskCompleted` twice is a hard crash in BackgroundTasks, and an expiration racing a
  /// completion is a real scheduling case rather than a hypothetical one.
  func testTheBackgroundCompletionFlagOnlyLetsOneWinnerThrough() {
    let flag = MavAtomicFlag()
    XCTAssertFalse(flag.testAndSet(), "the first caller should win")
    XCTAssertTrue(flag.testAndSet(), "the second caller must be told it lost")
    XCTAssertTrue(flag.testAndSet())
  }

  /// Backgrounding must not release a model out from under a running inference, which is why the
  /// release goes through the engine's queue rather than straight to the runner.
  func testReleasingTheCacheWaitsForThePassInFlight() {
    let host = FakeHost(["pulse_ppg", "cva_encoder"])
    let runner = FakeRunner(failOn: nil)
    let engine = MavAnalyticsEngine(
      runtime: FakeRuntime(host),
      runner: runner,
      clock: { 0 }
    )
    let released = expectation(description: "released")
    engine.runPass(deviceID: 1, mode: .interactive) { _ in }
    engine.releaseRunnerCache()
    // Ordered behind the pass on the same serial queue, so by the time a third block runs both
    // the pass and the release have happened.
    engine.runPass(deviceID: 1, mode: .interactive) { _ in released.fulfill() }
    wait(for: [released], timeout: 5)
    XCTAssertEqual(runner.released, 1)
    XCTAssertEqual(host.submitted.count, 2, "the release must not have cut the pass short")
  }

  /// A runner that cancels the pass it is running, after `cancelAfter` inferences.
  private final class CancellingRunner: MavModelBridge.Runner, @unchecked Sendable {
    private let lock = NSLock()
    private var ran = 0
    private let cancelAfter: Int
    /// Set after construction, because the engine needs the runner to exist first.
    var engine: MavAnalyticsEngine?

    init(cancelAfter: Int) { self.cancelAfter = cancelAfter }

    func run(slug: String, inputs: [String: [Float]]) throws -> [String: [Float]] {
      lock.lock()
      ran += 1
      let reached = ran == cancelAfter
      lock.unlock()
      if reached { engine?.cancelCurrentPass() }
      return ["out": [1.0]]
    }
    func admittedSHA256(for slug: String) throws -> String { String(repeating: "a", count: 64) }
    func releaseCache() {}
  }

  /// A cancelled pass stops draining instead of finishing the queue it was given.
  ///
  /// This is the background window being taken back. Before the engine had a cancellation signal
  /// the expiration handler completed the `BGTask` and the pass carried on running inferences
  /// behind it — with the comment above it claiming otherwise. Android has always stopped at
  /// `ensureActive()`; this is the same boundary.
  ///
  /// Deferred mode drains four per round, so eight queued models are two rounds. Cancelling
  /// during the first inference must leave the second round unstarted: four submitted, not eight.
  func testACancelledPassStopsDrainingAtTheNextRound() {
    let host = FakeHost((0..<8).map { _ in "pulse_ppg" })
    let runner = CancellingRunner(cancelAfter: 1)
    let engine = MavAnalyticsEngine(
      runtime: FakeRuntime(host),
      runner: runner,
      clock: { 0 }
    )
    runner.engine = engine
    _ = run(engine, mode: .deferred)
    XCTAssertEqual(
      host.submitted.count, MavAnalyticsEngine.RunMode.deferred.burst,
      "the round in flight finishes and the next one must not start"
    )
  }

  /// Cancellation is per pass, not sticky. One expired background window must not disable
  /// analytics until the app is reinstalled.
  func testACancellationDoesNotCarryIntoTheNextPass() {
    let host = FakeHost((0..<8).map { _ in "pulse_ppg" })
    let engine = MavAnalyticsEngine(
      runtime: FakeRuntime(host),
      runner: FakeRunner(failOn: nil),
      clock: { 0 }
    )
    engine.cancelCurrentPass()
    XCTAssertEqual(run(engine, mode: .deferred), .completed)
    XCTAssertEqual(host.submitted.count, 8, "a fresh pass inherited a stale cancellation")
  }

  func testTheDeferredModeAsksForLessWorkThanTheInteractiveOne() {
    XCTAssertLessThan(
      MavAnalyticsEngine.RunMode.deferred.burst,
      MavAnalyticsEngine.RunMode.interactive.burst
    )
    // The wire values are what the core parses; a typo here is an FFI error at runtime.
    XCTAssertEqual(MavAnalyticsEngine.RunMode.deferred.rawValue, "deferred")
    XCTAssertEqual(MavAnalyticsEngine.RunMode.interactive.rawValue, "interactive")
  }
}
