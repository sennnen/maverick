import SwiftUI

// Shared hub chrome: title + top-right cog (the ONE app-wide settings sheet)
// and pencil (in-hub card customisation). Used identically by all four hubs.

// MARK: - Cross-hub environment

/// Set by the shell so any hub content can switch tabs (e.g. tapping a pillar
/// ring on Today jumps to that pillar's hub).
struct AuraTabSwitchKey: EnvironmentKey {
  static let defaultValue: (AuraTab) -> Void = { _ in }
}

/// Set by the shell; hubs call it to present the app-wide settings sheet.
struct AuraOpenSettingsKey: EnvironmentKey {
  static let defaultValue: () -> Void = {}
}

extension EnvironmentValues {
  var auraSwitchTab: (AuraTab) -> Void {
    get { self[AuraTabSwitchKey.self] } set { self[AuraTabSwitchKey.self] = newValue }
  }
  var auraOpenSettings: () -> Void {
    get { self[AuraOpenSettingsKey.self] } set { self[AuraOpenSettingsKey.self] = newValue }
  }
}

enum AuraTab: Int, CaseIterable, Identifiable {
  case today = 0, recovery = 1, strain = 2, sleep = 3
  var id: Int { rawValue }
  var title: LocalizedStringKey {
    switch self {
    case .today: "Today"; case .recovery: "Recovery"
    case .strain: "Strain"; case .sleep: "Sleep"
    }
  }
  var icon: String {
    switch self {
    case .today: "circle.grid.2x2"
    case .recovery: "bolt.heart"
    case .strain: "flame"
    case .sleep: "moon.zzz"
    }
  }
}

// MARK: - Per-hub card visibility (the pencil's restricted edit mode)

/// Secondary-card show/hide per hub, CSV-persisted (mirrors MoreSectionPrefs
/// idiom so relaunch + tab hops keep the choice). Core pillars are never listed
/// here — only secondary tiles are toggleable.
struct AuraHubCards {
  static func storageKey(_ hub: String) -> String { "aura.hiddenCards.\(hub)" }
  static func decode(_ csv: String) -> Set<String> {
    Set(csv.split(separator: ",").map(String.init).filter { !$0.isEmpty })
  }
  static func encode(_ hidden: Set<String>) -> String { hidden.sorted().joined(separator: ",") }
}

// MARK: - Hub header

struct AuraHubHeader: View {
  let title: String
  var subtitle: String = ""
  /// nil = hub has no customisable cards (pencil hidden).
  var editing: Binding<Bool>?

  @Environment(\.auraOpenSettings) private var openSettings

  var body: some View {
    HStack(alignment: .center, spacing: 10) {
      VStack(alignment: .leading, spacing: 3) {
        Text(title).font(AuraDesign.display(34)).foregroundStyle(AuraDesign.ink)
        if !subtitle.isEmpty {
          Text(subtitle).font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.66))
        }
      }
      Spacer(minLength: 8)

      if let editing {
        chromeButton(editing.wrappedValue ? "checkmark" : "pencil",
                     label: editing.wrappedValue ? "Done editing" : "Edit cards",
                     active: editing.wrappedValue) {
          withAnimation(.spring(response: 0.4, dampingFraction: 0.85)) {
            editing.wrappedValue.toggle()
          }
        }
      }
      chromeButton("gearshape", label: "Settings", active: false) { openSettings() }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(.top, 4)
  }

  private func chromeButton(_ icon: String, label: LocalizedStringKey, active: Bool,
                            action: @escaping () -> Void) -> some View {
    Button(action: action) {
      Image(systemName: icon)
        .font(.system(size: 15, weight: .semibold))
        .foregroundStyle(active ? Color.black : AuraDesign.ink.opacity(0.9))
        .frame(width: 40, height: 40)
        .background {
          if active { Circle().fill(AuraDesign.accent) }
        }
        .auraGlass(Circle(), interactive: true)
        .contentShape(Circle())
    }
    .buttonStyle(AuraPressStyle())
    .accessibilityLabel(Text(label))
  }
}

// MARK: - Editable secondary card wrapper

/// Wraps a secondary hub card. In edit mode it shows a toggle badge and dims
/// hidden cards; at rest hidden cards vanish. The hero/pillar cards are never
/// wrapped, so they can't be removed (restricted edit).
struct AuraEditableCard<Content: View>: View {
  let key: String
  @Binding var hiddenCSV: String
  let editing: Bool
  @ViewBuilder var content: () -> Content

  private var hidden: Bool { AuraHubCards.decode(hiddenCSV).contains(key) }

  var body: some View {
    if editing {
      content()
        .opacity(hidden ? 0.35 : 1)
        .overlay(alignment: .topTrailing) {
          Button {
            withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) {
              var set = AuraHubCards.decode(hiddenCSV)
              if hidden { set.remove(key) } else { set.insert(key) }
              hiddenCSV = AuraHubCards.encode(set)
            }
          } label: {
            Image(systemName: hidden ? "eye.slash" : "eye")
              .font(.system(size: 13, weight: .semibold))
              .foregroundStyle(hidden ? AuraDesign.ink.opacity(0.6) : Color.black)
              .frame(width: 32, height: 32)
              .background(hidden ? AnyShapeStyle(AuraDesign.card) : AnyShapeStyle(AuraDesign.accent),
                          in: Circle())
              .overlay(Circle().strokeBorder(.white.opacity(0.15), lineWidth: 1))
          }
          .buttonStyle(.plain)
          .padding(10)
          .accessibilityLabel(Text(hidden ? "Show card" : "Hide card"))
        }
    } else if !hidden {
      content()
    }
  }
}
