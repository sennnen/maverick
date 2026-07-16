import Foundation
import Combine
import UserNotifications

/// The general-purpose countdown timer (§7): sauna, cold plunge, cooking, stretching —
/// set a duration, get a strap buzz at zero. Not a workout, not an alarm clock.
///
/// Firing model: while running the end instant is persisted + a local notification is
/// scheduled at it (so a backgrounded/killed app still tells the user); in the
/// foreground a 1 s tick drives the on-screen count and, at zero, the RING state —
/// the strap re-buzzes every few seconds until acknowledged, because a single buzz
/// mid-sauna is easy to miss. Double-tap acknowledges ONLY while ringing (§2): a
/// timer that is merely set never eats the user's configured double-tap action.
///
/// Strap I/O is injected (`buzz` / `stopBuzz`) so the engine stays BLE-free.
@MainActor
final class CountdownTimer: ObservableObject {

    /// The instant the running timer fires, nil when idle/paused.
    @Published private(set) var endDate: Date?
    /// Seconds banked while paused, nil when not paused.
    @Published private(set) var pausedRemaining: Int?
    /// True from fire until `acknowledge()` — the window double-tap claims (§2).
    @Published private(set) var isRinging = false
    /// UI tick: republished every second while running so views re-derive `remaining`.
    @Published private(set) var heartbeat = 0

    /// Last-used duration in seconds — the sticky default the picker opens on.
    @Published var lastDurationSeconds: Int {
        didSet { UserDefaults.standard.set(lastDurationSeconds, forKey: Self.durationKey) }
    }

    /// One-shot strap buzz (the confirmed `buzzStrapOnce` sequence) — fired on ring.
    var buzz: () -> Void = {}
    /// Best-effort strap haptic clear (see `AppModel.stopHaptics`) — fired on acknowledge.
    var stopBuzz: () -> Void = {}

    private var tick: Timer?
    private var ringBuzzCount = 0
    private static let endKey = "countdownTimer.endDate"
    private static let durationKey = "countdownTimer.lastDuration"
    private static let notificationId = "countdown-timer-done"
    /// Re-buzz cadence while ringing, and the cap so an unacknowledged ring can't
    /// buzz a wrist forever (≈30 s of insistence).
    private static let ringRebuzzSeconds = 4, ringMaxBuzzes = 8

    var isRunning: Bool { endDate != nil }

    /// Seconds to zero for display: live countdown, paused bank, or nil when idle.
    var remaining: Int? {
        if let pausedRemaining { return pausedRemaining }
        guard let endDate else { return nil }
        return max(0, Int(endDate.timeIntervalSinceNow.rounded()))
    }

    init() {
        lastDurationSeconds = UserDefaults.standard.object(forKey: Self.durationKey) as? Int ?? 10 * 60
        // Relaunch mid-run: revive a future end instant; a past one already notified —
        // clear it quietly rather than replaying a stale ring at launch.
        if let end = UserDefaults.standard.object(forKey: Self.endKey) as? Double {
            let date = Date(timeIntervalSince1970: end)
            if date > Date() { resume(until: date) }
            else { UserDefaults.standard.removeObject(forKey: Self.endKey) }
        }
    }

    // MARK: Controls

    func start(seconds: Int) {
        guard seconds > 0 else { return }
        acknowledge()
        lastDurationSeconds = seconds
        resume(until: Date().addingTimeInterval(TimeInterval(seconds)))
    }

    func pause() {
        guard let endDate else { return }
        pausedRemaining = max(1, Int(endDate.timeIntervalSinceNow.rounded()))
        self.endDate = nil
        stopTick()
        UserDefaults.standard.removeObject(forKey: Self.endKey)
        cancelNotification()
    }

    func resumePaused() {
        guard let banked = pausedRemaining else { return }
        pausedRemaining = nil
        resume(until: Date().addingTimeInterval(TimeInterval(banked)))
    }

    func reset() {
        endDate = nil
        pausedRemaining = nil
        stopTick()
        UserDefaults.standard.removeObject(forKey: Self.endKey)
        cancelNotification()
        acknowledge()
    }

    /// Stop the ring: strap haptics cleared, notification dismissed. The double-tap
    /// path (§2) and the on-screen Stop both land here. Safe to call when idle.
    func acknowledge() {
        guard isRinging else { return }
        isRinging = false
        ringBuzzCount = 0
        stopBuzz()
        UNUserNotificationCenter.current()
            .removeDeliveredNotifications(withIdentifiers: [Self.notificationId])
    }

    // MARK: Internals

    private func resume(until date: Date) {
        endDate = date
        UserDefaults.standard.set(date.timeIntervalSince1970, forKey: Self.endKey)
        scheduleNotification(at: date)
        startTick()
    }

    private func startTick() {
        stopTick()
        let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in self?.onTick() }
        }
        RunLoop.main.add(timer, forMode: .common)
        tick = timer
    }

    private func stopTick() {
        tick?.invalidate()
        tick = nil
    }

    private func onTick() {
        if isRinging {
            // Insist until acknowledged, spaced + capped.
            ringBuzzCount += 1
            if ringBuzzCount % Self.ringRebuzzSeconds == 0,
               ringBuzzCount / Self.ringRebuzzSeconds < Self.ringMaxBuzzes {
                buzz()
            }
            return
        }
        guard let endDate else { stopTick(); return }
        heartbeat &+= 1
        if endDate.timeIntervalSinceNow <= 0 { fire() }
    }

    private func fire() {
        endDate = nil
        UserDefaults.standard.removeObject(forKey: Self.endKey)
        isRinging = true
        ringBuzzCount = 0
        buzz()
        // Tick keeps running to drive the re-buzz cadence until acknowledged.
    }

    /// Local notification at the fire instant so a backgrounded app still lands the
    /// "time's up". Status-only auth check (no surprise permission prompt) — the strap
    /// buzz and on-screen state are the primary signals.
    private func scheduleNotification(at date: Date) {
        cancelNotification()
        let center = UNUserNotificationCenter.current()
        center.getNotificationSettings { settings in
            guard settings.authorizationStatus == .authorized else { return }
            let content = UNMutableNotificationContent()
            content.title = String(localized: "Timer done")
            content.body = String(localized: "Your countdown just finished.")
            content.sound = .default
            let seconds = max(1, date.timeIntervalSinceNow)
            let trigger = UNTimeIntervalNotificationTrigger(timeInterval: seconds, repeats: false)
            center.add(UNNotificationRequest(identifier: Self.notificationId,
                                             content: content, trigger: trigger))
        }
    }

    private func cancelNotification() {
        UNUserNotificationCenter.current()
            .removePendingNotificationRequests(withIdentifiers: [Self.notificationId])
    }
}
