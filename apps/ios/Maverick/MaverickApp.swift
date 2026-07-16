import SwiftUI

@main
struct MaverickApp: App {
  @StateObject private var store = MavStore()

  var body: some Scene {
    WindowGroup {
      MavAuraRootView()
        .environmentObject(store)
        .preferredColorScheme(.dark)
    }
  }
}
