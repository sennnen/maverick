import SwiftUI

struct MavTodayView: View {
  @EnvironmentObject private var store: MavStore

  var body: some View {
    MavHub(title: "Today", subtitle: "Private, local, inspectable", family: .heart) { snapshot in
      VStack(alignment: .leading, spacing: 16) {
        HStack(spacing: 12) {
          Image(systemName: "heart.fill").font(.system(size: 28)).foregroundStyle(AuraDesign.Family.heart.glow)
          VStack(alignment: .leading, spacing: 4) {
            Text(snapshot?.currentBpm.map(String.init) ?? "--").font(AuraDesign.mega(64)).foregroundStyle(AuraDesign.ink)
            Text("Live heart rate · bpm").font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.68))
          }
        }
        AuraLiveHRPill(bpm: snapshot?.currentBpm, deviceName: snapshot?.deviceName ?? "No device", batteryPercent: nil, bonded: snapshot?.connectionState == "connected")
      }
      .auraGlowTile(.heart)
      AuraSectionHeader(title: "Daily signals")
      HStack(spacing: 12) {
        AuraMiniStat(value: "--", label: "Recovery", level: 0, tint: AuraDesign.Family.charge.glow)
        AuraMiniStat(value: "--", label: "Strain", level: 0, tint: AuraDesign.Family.effort.glow)
        AuraMiniStat(value: "--", label: "Sleep", level: 0, tint: AuraDesign.Family.rest.glow)
      }
      .auraGlowTile(nil, padding: AuraDesign.tilePadding)
      Text("Scores appear after admitted streams are stored and analysed.").font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.62))
    }
  }
}

struct MavRecoveryView: View {
  var body: some View {
    MavHub(title: "Recovery", subtitle: "Readiness from overnight data", family: .charge) { snapshot in
      HStack(spacing: 22) {
        AuraRing(progress: 0, text: "--", tint: AuraDesign.Family.charge.glow, size: 144, lineWidth: 8)
        VStack(alignment: .leading, spacing: 8) {
          Text("Recovery").font(AuraDesign.title).foregroundStyle(AuraDesign.ink)
          Text(snapshot?.recoveryUnavailableReason ?? "No recovery result yet.").font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.72))
        }
      }
      .auraGlowTile(.charge)
      MavUnavailableCard(title: "Vitals", detail: "Mav shows raw provenance before presenting a score.")
    }
  }
}

struct MavStrainView: View {
  var body: some View {
    MavHub(title: "Strain", subtitle: "Daily load and activity", family: .effort) { _ in
      HStack(spacing: 22) {
        AuraRing(progress: 0, text: "--", tint: AuraDesign.Family.effort.glow, size: 144, lineWidth: 8)
        VStack(alignment: .leading, spacing: 8) {
          Text("Strain").font(AuraDesign.title).foregroundStyle(AuraDesign.ink)
          Text("No admitted strain algorithm in current core schema.").font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.72))
        }
      }
      .auraGlowTile(.effort)
      MavUnavailableCard(title: "Activities", detail: "Connector ingestion will populate this view. Nothing is estimated here.")
    }
  }
}

struct MavSleepView: View {
  var body: some View {
    MavHub(title: "Sleep", subtitle: "Overnight timing and stages", family: .rest) { _ in
      HStack(spacing: 22) {
        AuraRing(progress: 0, text: "--", tint: AuraDesign.Family.rest.glow, size: 144, lineWidth: 8)
        VStack(alignment: .leading, spacing: 8) {
          Text("Sleep").font(AuraDesign.title).foregroundStyle(AuraDesign.ink)
          Text("No admitted sleep result in current core schema.").font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.72))
        }
      }
      .auraGlowTile(.rest)
      MavUnavailableCard(title: "Sleep detail", detail: "Stages and SpO₂ stay absent until a connector provides verified data.")
    }
  }
}

private struct MavHub<Content: View>: View {
  @EnvironmentObject private var store: MavStore
  let title: String
  let subtitle: String
  let family: AuraDesign.Family
  @ViewBuilder var content: (MavSnapshot?) -> Content

  var body: some View {
    let snapshot: MavSnapshot? = if case let .ready(snapshot) = store.state { snapshot } else { nil }
    ScrollView {
      VStack(alignment: .leading, spacing: AuraDesign.cardSpacing) {
        AuraHubHeader(title: title, subtitle: subtitle)
        content(snapshot)
        if case let .failed(message) = store.state { MavUnavailableCard(title: "Runtime", detail: message) }
      }
      .padding(.horizontal, AuraDesign.screenMargin).padding(.top, 14).padding(.bottom, 116)
    }
    .scrollIndicators(.hidden)
    .auraScreen(family)
  }
}

private struct MavUnavailableCard: View {
  let title: String
  let detail: String
  var body: some View {
    VStack(alignment: .leading, spacing: 7) {
      Text(title).font(AuraDesign.title).foregroundStyle(AuraDesign.ink)
      Text(detail).font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.62))
    }
    .auraGlowTile(nil)
  }
}

struct MavSettingsView: View {
  @EnvironmentObject private var store: MavStore
  var body: some View {
    AuraSheet(title: "Settings") {
      MavSettingsRow(icon: "cpu", title: "Core", detail: "Local Mav runtime") { store.retry() }
      MavSettingsRow(icon: "internaldrive", title: "Storage", detail: storageDetail) {}
      MavSettingsRow(icon: "sensor.tag.radiowaves.forward", title: "Connectors", detail: "Installed separately; none bundled") {}
      MavSettingsRow(icon: "stethoscope", title: "Diagnostics", detail: diagnosticDetail) { store.retry() }
    }
  }
  private var storageDetail: String { if case let .ready(snapshot) = store.state { "Schema \(snapshot.storageSchema)" } else { "Runtime unavailable" } }
  private var diagnosticDetail: String { if case let .ready(snapshot) = store.state { snapshot.connectionState } else { "Runtime unavailable" } }
}

private struct MavSettingsRow: View {
  let icon: String; let title: String; let detail: String; let action: () -> Void
  var body: some View {
    Button(action: action) {
      HStack(spacing: 14) {
        Image(systemName: icon).frame(width: 26).foregroundStyle(AuraDesign.accentInk)
        Text(title).font(AuraDesign.label).foregroundStyle(AuraDesign.ink)
        Spacer()
        Text(detail).font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.5)).lineLimit(1)
        Image(systemName: "chevron.right").font(.caption).foregroundStyle(AuraDesign.ink.opacity(0.35))
      }
      .padding(.horizontal, 18).padding(.vertical, 15)
    }
    .buttonStyle(.plain)
    .background(AuraDesign.card, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
  }
}
