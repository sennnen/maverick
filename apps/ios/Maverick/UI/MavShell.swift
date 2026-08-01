import SwiftUI

// The whole app's structure: three tabs, one toolbar, one settings sheet, one device sheet.
//
// This is deliberately built out of the *system's* components rather than lookalikes. `TabView` with
// `Tab` gives the real Liquid Glass tab bar, and toolbar placements give the real Liquid Glass
// toolbar — which means the chrome picks up scroll-edge behaviour, the morph on scroll, correct
// safe-area insets, and every accessibility affordance for free. A hand-drawn capsule gets none of
// that and drifts the moment the OS moves.
//
// `.topBarLeading` and `.topBarTrailing` also put settings hard against the left edge and the strap
// hard against the right, which a centred grid never quite does.
//
// The shell owns the selected day. Every tab reads it and none keeps its own, which is what makes
// stepping back a day mean the same thing everywhere instead of three different things.

enum MavTab: String, CaseIterable, Identifiable, Hashable {
  case today, vitals, workouts
  var id: String { rawValue }

  var title: String {
    switch self {
    case .today: "Today"
    case .vitals: "Vitals"
    case .workouts: "Workouts"
    }
  }

  var systemImage: String {
    switch self {
    case .today: "sun.horizon"
    case .vitals: "waveform.path.ecg"
    case .workouts: "figure.run"
    }
  }
}

@MainActor
final class MavShellState: ObservableObject {
  @Published var tab: MavTab = .today
  @Published var day = Date()
  @Published var showSettings = false
  @Published var showDevice = false

  /// The Workouts tab's navigation path. It lives here rather than in the view because the flow
  /// needs to push programmatically — choosing a sport moves you to the live session without a
  /// tap on a link — and because a path held in `@State` inside a tab is discarded when the tab
  /// is rebuilt, which would drop you out of a running workout on a tab switch.
  @Published var workoutPath: [MavWorkoutRoute] = []


  /// Forward stops on the newest logical day. The logical day rolls at 04:00, so "today" between
  /// midnight and then is still yesterday's night.
  var canGoForward: Bool {
    Repository.logicalDayKey(day) < Repository.logicalDayKey(Date())
  }

  var dayKey: String { Repository.logicalDayKey(day) }
}

struct MavShell: View {
  @EnvironmentObject private var live: LiveState
  @StateObject private var shell = MavShellState()

  var body: some View {
    TabView(selection: $shell.tab) {
      Tab(MavTab.today.title, systemImage: MavTab.today.systemImage, value: MavTab.today) {
        NavigationStack {
          MavTodayView(shell: shell).mavTabChrome(shell: shell, live: live)
        }
      }
      Tab(MavTab.vitals.title, systemImage: MavTab.vitals.systemImage, value: MavTab.vitals) {
        NavigationStack {
          MavVitalsView(shell: shell).mavTabChrome(shell: shell, live: live)
        }
      }
      Tab(MavTab.workouts.title, systemImage: MavTab.workouts.systemImage, value: MavTab.workouts) {
        NavigationStack(path: $shell.workoutPath) {
          MavWorkoutsView(shell: shell).mavTabChrome(shell: shell, live: live)
        }
      }
    }
    .sheet(isPresented: $shell.showSettings) { MavSettingsSheet() }
    .sheet(isPresented: $shell.showDevice) { MavDeviceSheet() }
  }
}

// MARK: - Toolbar

/// The one toolbar every tab carries. Settings hard left, the date in the centre, the strap hard
/// right, and all three rendered by the system so they are genuinely Liquid Glass rather than a
/// blur that looks a bit like it.
private struct MavTabChrome: ViewModifier {
  @ObservedObject var shell: MavShellState
  @ObservedObject var live: LiveState

  func body(content: Content) -> some View {
    content
      .background(MavAtmosphere().ignoresSafeArea())
      // A navigation bar with no title collapses to nothing and takes its toolbar items with it,
      // so the bar is given an empty inline title to keep it present and its own height.
      .navigationTitle("")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .topBarLeading) {
          Button {
            shell.showSettings = true
          } label: {
            Image(systemName: "gearshape")
          }
          .accessibilityLabel("Settings")
        }

        ToolbarItem(placement: .principal) {
          if shell.tab == .today {
            MavDateStepper(day: $shell.day, canGoForward: shell.canGoForward)
          } else {
            Text(shell.tab.title)
              .mavType(.label)
              .foregroundStyle(MavTheme.ink)
              .accessibilityAddTraits(.isHeader)
          }
        }

        ToolbarItem(placement: .topBarTrailing) {
          MavDeviceChip(
            batteryPercent: live.batteryPct.map { Int($0.rounded()) },
            connected: live.connected,
            deviceName: live.advertisingName
          ) {
            shell.showDevice = true
          }
        }
      }
  }
}

extension View {
  fileprivate func mavTabChrome(shell: MavShellState, live: LiveState) -> some View {
    modifier(MavTabChrome(shell: shell, live: live))
  }
}

// MARK: - The scrolling body every tab shares

/// One scroll container, so the inset that clears the tab bar is decided once. `TabView` already
/// reports the tab bar's height as a safe-area inset, so there is no magic number here.
struct MavTabScroll<Content: View>: View {
  @ViewBuilder var content: Content

  var body: some View {
    ScrollView {
      // Grouping the glass lets the system blend and morph adjacent surfaces instead of stacking
      // independent blurs, which is the difference between one material and a pile of them.
      GlassEffectContainer(spacing: MavTheme.cardSpacing) {
        VStack(alignment: .leading, spacing: MavTheme.cardSpacing) {
          content
        }
      }
      .padding(.horizontal, MavTheme.screenMargin)
      .padding(.bottom, 24)
    }
    .scrollIndicators(.hidden)
  }
}

/// A pushed destination. `NavigationStack` supplies the back button, the swipe-back gesture, and
/// the title — writing those by hand is how an app ends up with a back button that does not work
/// with VoiceOver's escape gesture.
struct MavDetailScaffold<Content: View>: View {
  let title: String
  /// A landscape to run full-bleed behind the whole screen, veiled so ordinary ink still sits on
  /// it. A metric opened from a row keeps the row's own crop, so the card the reader tapped grows
  /// into the page rather than being replaced by an unrelated one.
  var scene: MavScene.Crop?
  @ViewBuilder var content: Content

  var body: some View {
    ScrollView {
      GlassEffectContainer(spacing: MavTheme.cardSpacing) {
        VStack(alignment: .leading, spacing: MavTheme.cardSpacing) {
          content
        }
      }
      .padding(.horizontal, MavTheme.screenMargin)
      .padding(.bottom, 40)
    }
    .scrollIndicators(.hidden)
    .toolbar(.hidden, for: .tabBar)
    .background {
      ZStack(alignment: .top) {
        MavTheme.canvas
        if let scene {
          MavScene(crop: scene, treatment: .veiled)
        }
      }
      .ignoresSafeArea()
    }
    .navigationTitle(title)
    .navigationBarTitleDisplayMode(.inline)
  }
}
