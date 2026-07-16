import SwiftUI

// AuraMLSignalsCard — the read-only surface for the on-device ML layer (StrandML). Observes the engine
// DIRECTLY (a nested ObservableObject doesn't republish through AppModel) and shows only the signals that
// have a value yet, so it stays quiet until the strap has streamed enough beats. Every line is honestly
// labelled as an on-device estimate / screen, never a medical reading.

struct AuraMLSignalsCard: View {
  @ObservedObject var engine: StrandMLEngine

  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      AuraSectionHeader(title: "On-device signals")

      if engine.backboneActive {
        HStack(spacing: 8) {
          Image(systemName: "cpu")
            .font(.system(size: 12, weight: .semibold)).foregroundStyle(AuraDesign.good)
          Text("Pulse-PPG model active on device")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.6))
          Spacer()
        }
        .padding(.horizontal, 4)
      }

      if !hasAny {
        Text("Wear your strap for a few minutes. Cardiac rhythm, stress load and cardio fitness are estimated on device, privately.")
          .font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.6))
          .fixedSize(horizontal: false, vertical: true)
          .auraDarkCard(padding: 18)
      } else {
        VStack(spacing: 0) {
          if let afib = engine.afib, afib.reliable { rhythmRow(afib); divider }
          if let load = engine.stressLoad { stressRow(load); divider }
          if let vo2 = engine.vo2max { row("Cardio fitness", String(format: "%.0f", vo2), "VO₂max", .charge); divider }
          if let br = engine.respirationRate { row("Respiration", String(format: "%.0f", br), "br/min", .vitals) }
        }
        .padding(.vertical, 4)
        .background(AuraDesign.card, in: AuraDesign.tileShape)
        .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))

        Text("Estimated on your phone from the strap's optical signal. A screen, not a diagnosis.")
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.4))
          .fixedSize(horizontal: false, vertical: true)
          .padding(.horizontal, 4)
      }
    }
  }

  private var hasAny: Bool {
    engine.afib?.reliable == true || engine.stressLoad != nil
      || engine.vo2max != nil || engine.respirationRate != nil
  }

  @ViewBuilder private var divider: some View {
    Rectangle().fill(AuraDesign.ink.opacity(0.08)).frame(height: 1).padding(.leading, 18)
  }

  private func rhythmRow(_ afib: AFibScreener.AFibResult) -> some View {
    let irregular = afib.irregular
    return HStack(spacing: 14) {
      Image(systemName: irregular ? "waveform.path.ecg" : "heart")
        .font(.system(size: 16, weight: .medium))
        .foregroundStyle(irregular ? AuraDesign.fair : AuraDesign.good)
        .frame(width: 26)
      VStack(alignment: .leading, spacing: 2) {
        Text("Heart rhythm").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
        Text(irregular ? "Irregular rhythm noticed" : "Looks regular")
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
      }
      Spacer()
      AuraStatusChip(text: irregular ? "Check" : "Regular",
                     kind: irregular ? .caution : .positive)
    }
    .padding(.horizontal, 18).padding(.vertical, 13)
  }

  private func stressRow(_ load: Double) -> some View {
    let word: String = load >= 66 ? "High" : load >= 40 ? "Moderate" : "Low"
    return HStack(spacing: 14) {
      Image(systemName: "brain.head.profile")
        .font(.system(size: 16, weight: .medium))
        .foregroundStyle(AuraDesign.Family.effort.glow)
        .frame(width: 26)
      VStack(alignment: .leading, spacing: 2) {
        Text("Stress load").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
        Text("\(word) · from HRV & heart-rate")
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
      }
      Spacer()
      Text("\(Int(load.rounded()))")
        .font(AuraDesign.number(22)).foregroundStyle(AuraDesign.ink).monospacedDigit()
    }
    .padding(.horizontal, 18).padding(.vertical, 13)
  }

  private func row(_ title: String, _ value: String, _ unit: String,
                   _ family: AuraDesign.Family) -> some View {
    HStack(spacing: 14) {
      Image(systemName: "figure.run.circle")
        .font(.system(size: 16, weight: .medium))
        .foregroundStyle(family.glow)
        .frame(width: 26)
      Text(title).font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
      Spacer()
      HStack(alignment: .firstTextBaseline, spacing: 4) {
        Text(value).font(AuraDesign.number(22)).foregroundStyle(AuraDesign.ink).monospacedDigit()
        Text(unit).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.5))
      }
    }
    .padding(.horizontal, 18).padding(.vertical, 13)
  }
}
