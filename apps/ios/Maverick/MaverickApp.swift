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
  @StateObject private var connectors = ConnectorManager()
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
        .environmentObject(connectors)
        .preferredColorScheme(AppearanceMode.resolve(appearanceRaw).colorScheme)
        .onOpenURL { url in
          if url.isFileURL {
            connectors.importFile(url, origin: .share)
          } else {
            connectors.importRemote(url)
          }
        }
        .onAppear {
          repo.reload = { store.retry() }
          repo.lowPowerSink = { connectors.setLowPower($0) }
        }
        .onReceive(connectors.$connection) { connection in
          model.apply(connection: connection, to: live)
          if let device = connection.deviceID { connectors.refreshDays(deviceID: device) }
        }
        .onReceive(connectors.$days) { history in repo.acceptDays(history) }
    }
  }
}
