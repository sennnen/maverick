import SwiftUI

struct MavAuraRootView: View {
  @EnvironmentObject private var store: MavStore
  @State private var selectedTab: AuraTab = .today
  @State private var showSettings = false

  private static let ease = Animation.timingCurve(0.22, 1, 0.36, 1, duration: 0.24)

  var body: some View {
    ZStack(alignment: .bottom) {
      TabView(selection: $selectedTab) {
        hub(MavTodayView()).tag(AuraTab.today)
        hub(MavRecoveryView()).tag(AuraTab.recovery)
        hub(MavStrainView()).tag(AuraTab.strain)
        hub(MavSleepView()).tag(AuraTab.sleep)
      }
      .toolbar(.hidden, for: .tabBar)
      .animation(Self.ease, value: selectedTab)
      .simultaneousGesture(
        DragGesture(minimumDistance: 24).onEnded { value in
          let dx = value.translation.width, dy = value.translation.height
          guard abs(dx) > 60, abs(dx) > abs(dy) * 1.6 else { return }
          let next = min(3, max(0, selectedTab.rawValue + (dx < 0 ? 1 : -1)))
          guard let tab = AuraTab(rawValue: next), tab != selectedTab else { return }
          withAnimation(Self.ease) { selectedTab = tab }
        }
      )
      MavAuraTabBar(selection: $selectedTab, onReselect: { _ in store.retry() })
    }
    .environment(\.auraSwitchTab) { tab in withAnimation(Self.ease) { selectedTab = tab } }
    .environment(\.auraOpenSettings) { showSettings = true }
    .sheet(isPresented: $showSettings) { MavSettingsView().presentationDragIndicator(.visible) }
  }

  private func hub<V: View>(_ view: V) -> some View {
    NavigationStack {
      view.background(AuraDesign.bg.ignoresSafeArea()).toolbar(.hidden, for: .navigationBar)
    }
    .toolbar(.hidden, for: .tabBar)
  }
}

private struct MavAuraTabBar: View {
  @Binding var selection: AuraTab
  var onReselect: (AuraTab) -> Void

  var body: some View {
    HStack(spacing: 2) {
      ForEach(AuraTab.allCases) { tab in
        let active = selection == tab
        Button {
          if active { onReselect(tab) }
          else { withAnimation(.timingCurve(0.22, 1, 0.36, 1, duration: 0.24)) { selection = tab } }
        } label: {
          VStack(spacing: 3) {
            Image(systemName: tab.icon).font(.system(size: 18, weight: active ? .semibold : .regular))
            Text(tab.title).font(.system(size: 10, weight: active ? .semibold : .medium))
          }
          .foregroundStyle(active ? AuraDesign.accentInk : AuraDesign.ink.opacity(0.6))
          .frame(maxWidth: .infinity).padding(.vertical, 3).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(tab.title)
        .accessibilityAddTraits(active ? [.isButton, .isSelected] : .isButton)
      }
    }
    .padding(.vertical, 7).padding(.horizontal, 8)
    .auraGlass(Capsule(), interactive: true)
    .background(.white.opacity(0.06), in: Capsule())
    .overlay(Capsule().strokeBorder(LinearGradient(colors: [.white.opacity(0.22), .white.opacity(0.04)], startPoint: .top, endPoint: .bottom), lineWidth: 0.75))
    .shadow(color: .black.opacity(0.22), radius: 18, x: 0, y: 8)
    .padding(.horizontal, 22).padding(.bottom, 4)
  }
}
