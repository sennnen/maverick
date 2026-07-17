import SwiftUI

// Small themed real-data screens shared across hubs. The hub screens live in
// their own files (AuraRecoveryView / AuraSleepHubView / AuraStrainView / …).

// MARK: - Shared

struct AuraInfoRow: View {
  let label: String
  let value: String
  var body: some View {
    HStack {
      Text(label).font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.9))
      Spacer()
      Text(value).font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.78))
    }
    .padding(.horizontal, 18).padding(.vertical, 15)
  }
}

private struct RowDivider: View {
  var body: some View { Rectangle().fill(AuraDesign.ink.opacity(0.08)).frame(height: 1).padding(.leading, 18) }
}

private extension View {
  func auraGroup() -> some View {
    background(AuraDesign.card, in: AuraDesign.tileShape)
      .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
  }
}

private func screenTitle(_ t: String) -> some View {
  Text(t).font(AuraDesign.display(34)).foregroundStyle(AuraDesign.ink)
    .frame(maxWidth: .infinity, alignment: .leading).padding(.top, 4)
}

// MARK: - Live

struct AuraLiveView: View {
  @EnvironmentObject private var live: LiveState
  @EnvironmentObject private var model: AppModel
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var pulse = false

  private var bpm: Int? { model.bpm ?? live.heartRate }

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 22) {
        screenTitle("Live")

        VStack(spacing: 20) {
          HStack {
            AuraStatusChip(text: live.bonded ? "Connected" : "Searching",
                           kind: live.bonded ? .positive : .neutral, pulsing: !live.bonded)
            Spacer()
          }
          Image(systemName: "heart.fill")
            .font(.system(size: 46)).foregroundStyle(AuraDesign.Family.heart.glow)
            .scaleEffect(pulse ? 1.12 : 1)
            .onAppear { if bpm != nil, !reduceMotion { withAnimation(.easeInOut(duration: 0.7).repeatForever(autoreverses: true)) { pulse = true } } }
          HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(bpm.map { "\($0)" } ?? "--").font(AuraDesign.mega(96)).foregroundStyle(AuraDesign.ink)
            Text("bpm").font(AuraDesign.number(26)).foregroundStyle(AuraDesign.ink.opacity(0.5))
          }
        }
        .frame(maxWidth: .infinity, minHeight: 300)
        .auraGlowTile(.heart, padding: 22, radius: 34)

        VStack(spacing: 0) {
          AuraInfoRow(label: "Strap", value: live.advertisingName ?? "WHOOP")
          RowDivider()
          AuraInfoRow(label: "Battery", value: live.batteryPct.map { "\(Int($0.rounded()))%" } ?? "--")
          RowDivider()
          if let prv = model.prv {
            AuraInfoRow(label: "PRV · RMSSD",
                        value: String(format: "%.1f ms", Double(prv.rmssdMicros) / 1000))
            RowDivider()
            AuraInfoRow(label: "PRV intervals",
                        value: "\(prv.intervalCount) used · \(prv.excludedIntervalCount) excluded")
          } else {
            // The core's structured reason; the platform never invents availability.
            AuraInfoRow(label: "PRV", value: model.prvUnavailableReason ?? "--")
          }
        }
        .padding(.vertical, 4).auraGroup()

        if model.prv != nil {
          Text("PRV is optical pulse-rate variability, not ECG HRV.")
            .font(AuraDesign.sub)
            .foregroundStyle(AuraDesign.ink.opacity(0.55))
            .padding(.horizontal, 4)
        }
      }
      .padding(.horizontal, AuraDesign.screenMargin).padding(.bottom, 130)
    }
    .scrollIndicators(.hidden).auraScreen(.heart)
  }
}
