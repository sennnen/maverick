import SwiftUI

@main
struct MaverickApp: App {
  @StateObject private var store = MavStore()
  @StateObject private var model = AppModel()
  @StateObject private var repo = Repository()
  @StateObject private var live = LiveState()
  @StateObject private var profile = ProfileStore()
  @StateObject private var health = HealthKitBridge()
  @StateObject private var router = NavRouter()
  @AppStorage(AppearanceMode.storageKey) private var appearanceRaw = AppearanceMode.system.rawValue

  var body: some Scene {
    WindowGroup {
      RootTabView()
        .environmentObject(store)
        .environmentObject(model)
        .environmentObject(repo)
        .environmentObject(live)
        .environmentObject(profile)
        .environmentObject(health)
        .environmentObject(router)
        .preferredColorScheme(AppearanceMode.resolve(appearanceRaw).colorScheme)
        .onChange(of: store.state) { _, state in
          if case let .ready(snapshot) = state {
            model.apply(snapshot: snapshot, to: live)
            model.syncProgress = store.syncProgress
          }
        }
        .onAppear { repo.reload = { store.retry() } }
    }
  }
}
