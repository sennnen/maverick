import SwiftUI

@main
struct MaverickApp: App {
  @StateObject private var store = MavStore()
  @StateObject private var analytics = MavAnalyticsModel()
  @StateObject private var model = AppModel()
  @StateObject private var repo = Repository()
  @StateObject private var live = LiveState()
  @StateObject private var profile = ProfileStore()
  @StateObject private var health = HealthKitBridge()
  @StateObject private var connectors = ConnectorManager()
  @AppStorage(AppearanceMode.storageKey) private var appearanceRaw = AppearanceMode.system.rawValue
  @Environment(\.scenePhase) private var scenePhase

  init() {
    // BGTaskScheduler refuses a registration made after launch finishes, so this cannot move
    // into a `.task` or an `onAppear`.
    MavBackgroundAnalytics.register()
  }

  var body: some Scene {
    WindowGroup {
      // No NavigationStack here: each tab owns its own, and nesting one inside another leaves the
      // inner navigation bars with nowhere to draw.
      MavShell()
        .environmentObject(store)
        .environmentObject(model)
        .environmentObject(repo)
        .environmentObject(live)
        .environmentObject(profile)
        .environmentObject(health)
        .environmentObject(connectors)
        .environmentObject(analytics)
        .preferredColorScheme(AppearanceMode.resolve(appearanceRaw).colorScheme)
        // Chrome is monochrome on purpose. Tinting the whole app ochre made every selected
        // tab, switch and segment shout in the interaction voice; accent is applied per-element,
        // at most once a screen.
        .tint(MavTheme.ink)
        .onOpenURL { url in
        if url.isFileURL {
          connectors.importFile(url, origin: .share)
        } else {
          connectors.importRemote(url)
        }
      }
        .onAppear {
        attachAnalytics()
        // Ask for the background windows on every launch: iOS drops pending requests when the
        // app is force-quit, and re-submitting is the only way back.
        MavBackgroundAnalytics.schedule()
        repo.reload = { store.retry() }
        repo.lowPowerSink = { connectors.setLowPower($0) }
        // The battery saver is core state, not app state (ADR-030), so the switch is seeded from
        // the runtime rather than assumed off across a relaunch.
        if let runtime = AuraZoneMath.runtime { repo.adoptLowPower(runtime.lowPower()) }
      }
        .onReceive(connectors.$connection) { connection in
        #if DEBUG
          // Fixture mode owns the disconnected presentation. A late duplicate disconnected event
          // must not erase its device chip after the review surface has already populated.
          if model.usingDebugFixture, !connection.connected { return }
        #endif
        model.apply(connection: connection, to: live)
        if let device = connection.deviceID { connectors.refreshDays(deviceID: device) }
      }
        .onReceive(connectors.$days) { history in
        guard !history.isEmpty else { return }
        #if DEBUG
          // ConnectorManager can publish cached history after the two-second fixture seed. Keep
          // the deterministic review day until an actual device is connected.
          if model.usingDebugFixture, !connectors.connection.connected { return }
        #endif
        repo.acceptDays(history)
        model.dailySnapshot = history.last
        model.usingDebugFixture = false
      }
        .task {
        // Debug only, and only while nothing real has arrived: seed the fixture so the layout can
        // be judged without a strap. Every surface it feeds is badged, and a release build has no
        // fixture to seed from — the file does not compile into it.
        #if DEBUG
          try? await Task.sleep(for: .seconds(2))
          // A debug build without a live device is a review build: populate every adaptive
          // surface, visibly marked SAMPLE, even if stale historical rows exist locally.
          guard !live.connected else { return }
          let days = MavDebugFixture.snapshots()
          repo.acceptDays(days)
          model.dailySnapshot = days.last
          model.usingDebugFixture = true
          // A fixture link too, so the chip has a battery percentage and a state to show. Without
          // it the whole of the device sheet is an unpaired empty screen and cannot be judged.
          MavDebugFixture.apply(to: live, model: model)
        #endif
      }
    }
    // The whole of the analytics lifecycle, in one place.
    //
    // `.onAppear` fires once, at launch, and a resume from the app switcher is not a launch. A
    // wearer who left the app three hours ago and came back would otherwise be reading a
    // three-hour-old pass with no way to tell — Android has run a pass on every `onResume` since
    // this landed, and this is the missing half of that parity.
    .onChange(of: scenePhase) { _, phase in
      switch phase {
      case .active:
        attachAnalytics()
      case .background:
        // Both halves of leaving: ask for the next window while the app is still alive to ask,
        // and stop holding models the wearer is not waiting on. A backgrounded app holding a
        // compiled model set is the first thing the system reclaims.
        MavBackgroundAnalytics.schedule()
        analytics.releaseResources()
      default:
        break
      }
    }
  }

  /// Build the engine if it does not exist yet, then run an interactive pass.
  ///
  /// Safe to call repeatedly: `attach` keeps the first engine, so a resume reuses the retry
  /// budgets and the published snapshot rather than starting from nothing. When the core is not
  /// open yet there is nothing to attach to and the next activation tries again.
  private func attachAnalytics() {
    guard let runtime = AuraZoneMath.runtime else { return }
    analytics.attach {
      MavAnalyticsEngine(
        runtime: MavCoreAnalyticsRuntime(runtime: runtime),
        runner: MavModelRunner()
      )
    }
    // The wearer is looking at the screen, so this pass is allowed to be expensive. A pass
    // already in flight turns this one away rather than queueing it.
    analytics.refresh()
  }
}
