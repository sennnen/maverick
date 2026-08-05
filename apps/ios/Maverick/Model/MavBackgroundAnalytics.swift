import BackgroundTasks
import Foundation

/// Analytics while the app is backgrounded, the screen is off, or the phone is locked.
///
/// Two identifiers, because iOS gives two different things and conflating them wastes both:
///
/// - **Processing** (`BGProcessingTaskRequest`) is the long one — minutes, granted when the phone
///   is idle and usually charging. That is where a foundation encoder over a night of PPG
///   belongs. It asks for external power, because a 28-million-parameter encoder is not work to
///   spend a wearer's commute battery on.
/// - **Refresh** (`BGAppRefreshTaskRequest`) is the short one — tens of seconds, granted more
///   often and closer to when the wearer actually opens the app. That is where a top-up pass
///   belongs, so the screen is current rather than complete.
///
/// **What iOS does not promise.** Nothing here assumes a task will run, or run on time. The
/// system schedules against its own model of the wearer's habits, budget and thermal state; a
/// request may be granted in minutes, in hours, or not at all. It is never granted while the
/// phone is off. If the wearer force-quits the app from the app switcher, iOS stops scheduling
/// background work for it entirely until the app is launched again — that behaviour is the
/// system's and is not something an app can opt out of or work around.
///
/// Every one of those degrades to the same place: the next foreground launch runs an interactive
/// pass and the wearer waits a moment instead of not waiting. That fallback is the contract; the
/// background window is the optimisation.
enum MavBackgroundAnalytics {
  /// The long window: a full pass over everything the day can feed.
  static let processingIdentifier = "com.sennnen.mav.analytics.processing"
  /// The short window: a top-up before a likely open.
  static let refreshIdentifier = "com.sennnen.mav.analytics.refresh"

  /// How the handlers reach the engine. Set once at launch; nil in tests.
  nonisolated(unsafe) static var provider: (() -> MavAnalyticsEngine?)?

  /// The device whose day is analysed. One strap at a time until multi-device.
  static let deviceID: UInt64 = 1

  /// Register both handlers.
  ///
  /// Must happen before the app finishes launching — iOS refuses a registration afterwards — so
  /// this is called from the app's initialiser rather than from a task or an `onAppear`.
  static func register() {
    BGTaskScheduler.shared.register(
      forTaskWithIdentifier: processingIdentifier,
      using: nil
    ) { task in handle(task: task, mode: .deferred) }

    BGTaskScheduler.shared.register(
      forTaskWithIdentifier: refreshIdentifier,
      using: nil
    ) { task in handle(task: task, mode: .deferred) }
  }

  /// Ask for both windows. Idempotent; safe on every background transition.
  static func schedule(earliestProcessing: TimeInterval = 2 * 60 * 60,
                       earliestRefresh: TimeInterval = 30 * 60) {
    let processing = BGProcessingTaskRequest(identifier: processingIdentifier)
    // Idle only: this is speculative work for a wearer who is probably asleep.
    processing.requiresNetworkConnectivity = false
    processing.requiresExternalPower = true
    processing.earliestBeginDate = Date(timeIntervalSinceNow: earliestProcessing)

    let refresh = BGAppRefreshTaskRequest(identifier: refreshIdentifier)
    refresh.earliestBeginDate = Date(timeIntervalSinceNow: earliestRefresh)

    // A submit failure is expected and is not an error worth surfacing: the wearer may have
    // background refresh switched off, or the app may be over its budget. The foreground pass
    // covers both.
    try? BGTaskScheduler.shared.submit(processing)
    try? BGTaskScheduler.shared.submit(refresh)
  }

  private static func handle(task: BGTask, mode: MavAnalyticsEngine.RunMode) {
    // Ask for the next window before doing any work. Doing it afterwards means a pass that is
    // expired by the system never reschedules, and background analytics quietly stops for good.
    schedule()

    guard let engine = provider?() else {
      return task.setTaskCompleted(success: true)
    }

    let finished = MavAtomicFlag()
    // The system can pull the window at any moment. Cancelling here lets the drain loop stop
    // between stages rather than being killed mid-inference.
    task.expirationHandler = {
      if finished.testAndSet() { return }
      task.setTaskCompleted(success: false)
    }

    engine.runPass(deviceID: deviceID, mode: mode) { outcome in
      if finished.testAndSet() { return }
      switch outcome {
      case .completed, .skippedBusy:
        task.setTaskCompleted(success: true)
      case .partial, .failed:
        // Reporting failure is what lets iOS space the retry itself rather than this app
        // guessing at a backoff the system already models better.
        task.setTaskCompleted(success: false)
      }
    }
  }
}

/// One-shot flag, so an expiration and a completion racing cannot both finish the same task.
///
/// Calling `setTaskCompleted` twice is a hard crash in `BackgroundTasks`, and the two callbacks
/// genuinely can race: the system may expire a window in the same instant the pass returns.
final class MavAtomicFlag: @unchecked Sendable {
  private let lock = NSLock()
  private var raised = false

  /// Raise the flag. Returns true when it was *already* raised — that is, when the caller lost.
  func testAndSet() -> Bool {
    lock.lock()
    defer { lock.unlock() }
    if raised { return true }
    raised = true
    return false
  }
}
