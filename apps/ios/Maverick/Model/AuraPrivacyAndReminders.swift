#if os(iOS)
import Foundation
import UserNotifications
import UIKit

// MARK: - Optional encryption at rest (iOS Data Protection)
//
// OFF by default and deliberately absent from onboarding: unencrypted files
// keep exported feedback/error logs readable for debugging. Toggling ON stamps
// NSFileProtectionCompleteUnlessOpen onto the local store's files — hardware-
// encrypted at rest, while the already-open store keeps writing during a
// locked-phone background sync.

enum AuraDataProtection {
  static let storageKey = "privacy.fileProtectionOn"

  /// Apply (or relax) file protection across the store directory. Returns the
  /// number of files updated, -1 on failure.
  @discardableResult
  static func apply(_ on: Bool) -> Int {
    guard let dir = storeDirectory() else { return -1 }
    let fm = FileManager.default
    let level: FileProtectionType = on ? .completeUnlessOpen : .none
    guard let files = try? fm.contentsOfDirectory(atPath: dir.path) else { return -1 }
    var n = 0
    for f in files {
      let p = dir.appendingPathComponent(f).path
      if (try? fm.setAttributes([.protectionKey: level], ofItemAtPath: p)) != nil { n += 1 }
    }
    return n
  }

  private static func storeDirectory() -> URL? {
    MavStore.databaseURL().deletingLastPathComponent()
  }
}

// MARK: - Morning journal reminder (actionable notification)
//
// A daily local notification with a "Log how you feel" action that deep-links
// straight into the Journal — no server, no push tokens.

enum JournalReminder {
  static let enabledKey = "reminder.journal.enabled"
  static let minutesKey = "reminder.journal.minutes"     // minutes from midnight
  static let categoryId = "noop.journal.checkin"
  static let actionId = "noop.journal.open"
  private static let requestId = "noop.journal.daily"

  /// Register the actionable category once at launch.
  static func registerCategory() {
    let open = UNNotificationAction(identifier: actionId,
                                    title: String(localized: "Log how you feel"),
                                    options: [.foreground])
    let cat = UNNotificationCategory(identifier: categoryId, actions: [open],
                                     intentIdentifiers: [], options: [])
    UNUserNotificationCenter.current().setNotificationCategories([cat])
  }

  /// (Re)schedule or cancel the daily reminder to match the settings.
  static func apply(enabled: Bool, minutes: Int) {
    let center = UNUserNotificationCenter.current()
    center.removePendingNotificationRequests(withIdentifiers: [requestId])
    guard enabled else { return }
    center.requestAuthorization(options: [.alert, .sound]) { granted, _ in
      guard granted else { return }
      let content = UNMutableNotificationContent()
      content.title = String(localized: "Morning check-in")
      content.body = String(localized: "30 seconds of journal now sharpens tonight's recovery insight.")
      content.categoryIdentifier = categoryId
      content.sound = .default
      var comps = DateComponents()
      comps.hour = minutes / 60
      comps.minute = minutes % 60
      let trigger = UNCalendarNotificationTrigger(dateMatching: comps, repeats: true)
      center.add(UNNotificationRequest(identifier: requestId, content: content, trigger: trigger))
    }
  }
}

/// Routes notification taps/actions into the app (journal action → Journal
/// sheet via NavRouter). Kept tiny; foreground presentations stay visible.
final class AuraNotificationRouter: NSObject, UNUserNotificationCenterDelegate {
  static let shared = AuraNotificationRouter()
  /// Set by the app shell at launch.
  var openJournal: (() -> Void)?

  func install() {
    UNUserNotificationCenter.current().delegate = self
    JournalReminder.registerCategory()
  }

  func userNotificationCenter(_ center: UNUserNotificationCenter,
                              willPresent notification: UNNotification) async
    -> UNNotificationPresentationOptions { [.banner, .sound] }

  func userNotificationCenter(_ center: UNUserNotificationCenter,
                              didReceive response: UNNotificationResponse) async {
    let cat = response.notification.request.content.categoryIdentifier
    if cat == JournalReminder.categoryId {
      await MainActor.run { openJournal?() }
    }
  }
}
#endif
