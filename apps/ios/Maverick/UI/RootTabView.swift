#if os(iOS)
import SwiftUI

/// iOS navigation shell — the Aura four-hub IA: **Today · Recovery · Strain ·
/// Sleep** behind a floating glass tab bar. Settings is never a tab: every hub's
/// top-right cog opens the ONE app-wide `AuraSettingsSheet` presented here.
struct RootTabView: View {
    @EnvironmentObject private var repo: Repository
    @EnvironmentObject private var router: NavRouter

    @State private var selectedTab: AuraTab = .today
    @State private var showSettings = false
    /// Cross-screen router presents (Live for active workout, Trends, journal
    /// fallbacks for legacy pillar deep-links).
    @State private var routedSheet: RoutedSheet?

    private enum RoutedSheet: Int, Identifiable {
        case live, trends, journal
        var id: Int { rawValue }
    }

    /// Calm-easing curve (cubic-bezier(0.22,1,0.36,1)).
    private static let ease = Animation.timingCurve(0.22, 1, 0.36, 1, duration: 0.24)

    var body: some View {
        ZStack(alignment: .bottom) {
            TabView(selection: $selectedTab) {
                hub(AuraTodayView()).tag(AuraTab.today)
                hub(AuraRecoveryView()).tag(AuraTab.recovery)
                hub(AuraStrainView()).tag(AuraTab.strain)
                hub(AuraSleepHubView()).tag(AuraTab.sleep)
            }
            .toolbar(.hidden, for: .tabBar)
            // ~240ms opacity swap between tab roots, calm easing.
            .animation(Self.ease, value: selectedTab)
            // Decisive horizontal flick moves between hubs; vertical scrolling wins otherwise.
            .simultaneousGesture(
                DragGesture(minimumDistance: 24)
                    .onEnded { v in
                        let dx = v.translation.width, dy = v.translation.height
                        guard abs(dx) > 60, abs(dx) > abs(dy) * 1.6 else { return }
                        let next = min(3, max(0, selectedTab.rawValue + (dx < 0 ? 1 : -1)))
                        if let tab = AuraTab(rawValue: next), tab != selectedTab {
                            withAnimation(Self.ease) { selectedTab = tab }
                        }
                    }
            )

            AuraTabBar(selection: $selectedTab, onReselect: { _ in
                Task { await repo.refresh() }
            })
        }
        .environment(\.auraSwitchTab) { tab in
            withAnimation(Self.ease) { selectedTab = tab }
        }
        .environment(\.auraOpenSettings) { showSettings = true }
        .sheet(isPresented: $showSettings) {
            AuraSettingsSheet().presentationDragIndicator(.visible)
        }
        .sheet(item: $routedSheet) { which in
            switch which {
            case .live: NavigationStack { AuraLiveView() }.presentationDragIndicator(.visible)
            case .trends: AuraTrendsView().presentationDragIndicator(.visible)
            case .journal: AuraJournalView().presentationDragIndicator(.visible)
            }
        }
        .task {
            AuraNotificationRouter.shared.openJournal = { routedSheet = .journal }
            AuraNotificationRouter.shared.install()
            await repo.refresh()
            // Backup & Sync on-launch catch-up: detached + utility priority so a
            // 100MB+ whole-DB ZIP never blocks startup; gated on the auto toggle.
            let backupRepo = repo
            Task.detached(priority: .utility) {
                await FolderBackup.catchUpIfDue(checkpoint: { await backupRepo.checkpointForBackup() })
            }
        }
        // Cross-screen navigation requests. Device management lives in the cog
        // sheet now; legacy pillar deep-links land on their nearest Aura home.
        .onChange(of: router.requestedDestination) { _, dest in
            guard let dest else { return }
            switch dest {
            case .devices:
                showSettings = true
            case .trends:
                routedSheet = .trends
            case .activeWorkout, .liveSession:
                routedSheet = .live
            case .insightsHub, .labBook, .fusedRecord:
                routedSheet = .journal
            case .rhythm:
                withAnimation(Self.ease) { selectedTab = .sleep }
            }
            router.requestedDestination = nil
        }
        .onChange(of: router.quickActionsRequested) { _, req in
            if req {
                routedSheet = .live
                router.quickActionsRequested = false
            }
        }
    }

    /// Each hub in its own NavigationStack (pushed details get a back button;
    /// hubs draw their own in-content headers, so the system bar stays hidden).
    private func hub<V: View>(_ view: V) -> some View {
        NavigationStack {
            view
                .background(AuraDesign.bg.ignoresSafeArea())
                .toolbar(.hidden, for: .navigationBar)
        }
        .toolbar(.hidden, for: .tabBar)
    }
}

// MARK: - Floating glass tab bar

/// One frosted capsule, four hubs. Real iOS 26 Liquid Glass where available,
/// `.ultraThinMaterial` below; Starship marks the active hub (interactive hue).
private struct AuraTabBar: View {
    @Binding var selection: AuraTab
    var onReselect: (AuraTab) -> Void = { _ in }

    var body: some View {
        HStack(spacing: 2) {
            ForEach(AuraTab.allCases) { tab in
                tabButton(tab)
            }
        }
        .padding(.vertical, 7)
        .padding(.horizontal, 8)
        .liquidGlass(in: Capsule())
        .background(.white.opacity(0.06), in: Capsule())
        .overlay(
            Capsule().strokeBorder(
                LinearGradient(colors: [.white.opacity(0.22), .white.opacity(0.04)],
                               startPoint: .top, endPoint: .bottom),
                lineWidth: 0.75)
        )
        .shadow(color: .black.opacity(0.22), radius: 18, x: 0, y: 8)
        .padding(.horizontal, 22)
        .padding(.bottom, 4)
    }

    private func tabButton(_ tab: AuraTab) -> some View {
        let active = selection == tab
        return Button {
            if active {
                onReselect(tab)
            } else {
                withAnimation(.timingCurve(0.22, 1, 0.36, 1, duration: 0.24)) { selection = tab }
            }
        } label: {
            VStack(spacing: 3) {
                Image(systemName: tab.icon)
                    .font(.system(size: 18, weight: active ? .semibold : .regular))
                Text(tab.title)
                    .font(.system(size: 10, weight: active ? .semibold : .medium))
            }
            .foregroundStyle(active ? AuraDesign.accentInk : AuraDesign.ink.opacity(0.6))
            .frame(maxWidth: .infinity)
            .padding(.vertical, 3)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(tab.title)
        .accessibilityAddTraits(active ? [.isButton, .isSelected] : .isButton)
    }
}

// MARK: - Liquid Glass (iOS 26) with a Material fallback

private extension View {
    @ViewBuilder func liquidGlass(in shape: some Shape) -> some View {
        self.glassEffect(.regular, in: shape)
    }
}
#endif
