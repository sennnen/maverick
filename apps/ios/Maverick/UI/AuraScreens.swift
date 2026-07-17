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
  @EnvironmentObject private var store: MavStore
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var pulse = false
  @State private var showPrvDetail = false

  private var bpm: Int? { model.bpm ?? live.heartRate }

  private var staleLabel: String? {
    guard case let .ready(snapshot) = store.state else { return nil }
    return MavPresent.sampleAgeLabel(
      asOfUnixMs: snapshot.asOfUnixMs,
      lastSampleUnixMs: snapshot.lastSampleUnixMs,
      connected: live.connected
    )
  }

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
          if let staleLabel {
            Text(staleLabel).font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.55))
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
            // Tap for the small PRV detail: the full admitted metric set + provenance.
            VStack(spacing: 0) {
              AuraInfoRow(label: "PRV · RMSSD", value: MavPresent.microsAsMs(prv.rmssdMicros))
              RowDivider()
              AuraInfoRow(label: "PRV intervals",
                          value: "\(prv.intervalCount) used · \(prv.excludedIntervalCount) excluded")
            }
            .contentShape(Rectangle())
            .onTapGesture { withAnimation { showPrvDetail.toggle() } }
            if showPrvDetail {
              RowDivider()
              AuraInfoRow(label: "SDNN", value: MavPresent.microsAsMs(prv.sdnnMicros))
              RowDivider()
              AuraInfoRow(label: "Mean interval", value: MavPresent.microsAsMs(prv.meanIntervalMicros))
              RowDivider()
              AuraInfoRow(label: "pNN50",
                          value: "\(MavPresent.milliPercentAsPercent(prv.pnn50MilliPercent)) · NN50 \(prv.nn50Count)")
              RowDivider()
              AuraInfoRow(label: "Algorithm", value: "\(prv.algorithm) v\(prv.algorithmVersion)")
              RowDivider()
              AuraInfoRow(label: "Provenance", value: "#\(prv.provenanceId) · \(prv.intervalSource)")
            }
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
