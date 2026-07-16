import SwiftUI

/// Theme preference (System / Light / Dark), persisted under one @AppStorage key shared by the
/// app root and the settings picker. `.system` follows the OS; the others force a scheme.
enum AppearanceMode: String, CaseIterable, Identifiable, Sendable {
  case system
  case light
  case dark

  var id: String { rawValue }

  /// The @AppStorage key shared by the app root and the Settings picker.
  static let storageKey = "theme.appearance"

  var label: String {
    switch self {
    case .system: String(localized: "System")
    case .light: String(localized: "Light")
    case .dark: String(localized: "Dark")
    }
  }

  var symbol: String {
    switch self {
    case .system: "circle.lefthalf.filled"
    case .light: "sun.max"
    case .dark: "moon.stars"
    }
  }

  /// The `ColorScheme` to force, or nil to follow the system.
  var colorScheme: ColorScheme? {
    switch self {
    case .system: nil
    case .light: .light
    case .dark: .dark
    }
  }

  /// Resolve a stored raw value (tolerant of unknown/missing → `.system`).
  static func resolve(_ raw: String) -> AppearanceMode {
    AppearanceMode(rawValue: raw) ?? .system
  }
}
