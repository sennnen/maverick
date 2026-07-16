import SwiftUI

// High-contrast data-viz + chrome components. Clean Helvetica numerals, visible
// tracks/markers, adaptive ink so everything reads on black and on the glow tiles.

private extension Double {
  var clamped01: Double { Swift.min(Swift.max(self, 0), 1) }
}

// MARK: - Slider (track + glowing marker)

/// A clearly-visible value slider (padel "Strike Power" style): a solid track, a
/// filled portion, and a bright ink marker with a coloured glow.
struct AuraSlider: View {
  var value: Double
  var glow: Color = AuraDesign.accent

  var body: some View {
    GeometryReader { g in
      let w = g.size.width
      let x = Swift.min(Swift.max(w * value.clamped01, 10), w - 10)
      ZStack(alignment: .leading) {
        Capsule().fill(AuraDesign.ink.opacity(0.18)).frame(height: 4)
        Capsule().fill(AuraDesign.ink.opacity(0.55)).frame(width: x, height: 4)
        Circle()
          .fill(AuraDesign.ink)
          .frame(width: 18, height: 18)
          .shadow(color: glow.opacity(0.95), radius: 9)
          .offset(x: x - 9)
      }
      .frame(height: g.size.height, alignment: .center)
    }
    .frame(height: 20)
    .accessibilityHidden(true)
  }
}

// MARK: - Mini stat (value + label + bar)

struct AuraMiniStat: View {
  var value: String
  var unit: String = ""
  var label: String
  var level: Double
  var tint: Color

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      HStack(alignment: .firstTextBaseline, spacing: 3) {
        Text(value)
          .font(AuraDesign.number(30))
          .foregroundStyle(AuraDesign.ink)
          .lineLimit(1).minimumScaleFactor(0.5)
        if !unit.isEmpty {
          Text(unit).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.68))
        }
      }
      Text(label)
        .font(AuraDesign.caption)
        .foregroundStyle(AuraDesign.ink.opacity(0.78))
        .lineLimit(1).minimumScaleFactor(0.8)
      GeometryReader { g in
        ZStack(alignment: .leading) {
          Capsule().fill(AuraDesign.ink.opacity(0.14)).frame(height: 3)
          Capsule().fill(tint).frame(width: g.size.width * min(max(level, 0), 1), height: 3)
        }
      }
      .frame(height: 3)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}

// MARK: - Thin ring

struct AuraRing: View {
  var progress: Double
  var text: String
  var tint: Color
  var size: CGFloat = 60
  var lineWidth: CGFloat = 4

  @State private var fill: Double = 0
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    ZStack {
      Circle().stroke(AuraDesign.ink.opacity(0.16), lineWidth: lineWidth)
      Circle()
        .trim(from: 0, to: fill)
        .stroke(tint, style: StrokeStyle(lineWidth: lineWidth, lineCap: .round))
        .rotationEffect(.degrees(-90))
      Text(text)
        .font(AuraDesign.number(size * 0.4))
        .foregroundStyle(AuraDesign.ink)
        .minimumScaleFactor(0.6).lineLimit(1)
    }
    .frame(width: size, height: size)
    .onAppear {
      if reduceMotion { fill = progress.clamped01 }
      else { withAnimation(.spring(response: 0.6, dampingFraction: 0.85)) { fill = progress.clamped01 } }
    }
    .onChange(of: progress) { _, _ in
      withAnimation(reduceMotion ? nil : .spring(response: 0.45, dampingFraction: 0.85)) { fill = progress.clamped01 }
    }
    .accessibilityLabel(Text(text))
  }
}

// MARK: - Delta label

struct AuraDelta: View {
  var value: Double
  var suffix: String = "/AVG"

  var body: some View {
    let up = value >= 0
    HStack(spacing: 3) {
      Image(systemName: up ? "arrow.up.right" : "arrow.down.right")
        .font(.system(size: 9, weight: .bold))
      Text("\(abs(Int(value.rounded()))) \(suffix)").font(AuraDesign.caption)
    }
    .foregroundStyle(up ? AuraDesign.good : AuraDesign.bad)
    .padding(.horizontal, 8)
    .padding(.vertical, 4)
    .background(AuraDesign.scrim, in: Capsule())
  }
}

// MARK: - Status chip

struct AuraStatusChip: View {
  enum Kind {
    case positive, caution, negative, neutral
    var color: Color {
      switch self {
      case .positive: AuraDesign.good
      case .caution: AuraDesign.fair
      case .negative: AuraDesign.bad
      case .neutral: AuraDesign.ink.opacity(0.7)
      }
    }
  }

  let text: String
  let kind: Kind
  var pulsing = false

  var body: some View {
    HStack(spacing: 6) {
      Circle().fill(kind.color).frame(width: 7, height: 7).modifier(PulseModifier(active: pulsing))
      Text(text).font(AuraDesign.caption).foregroundStyle(kind.color)
    }
    .padding(.horizontal, 9)
    .padding(.vertical, 5)
    .background(AuraDesign.scrim, in: Capsule())
  }
}

private struct PulseModifier: ViewModifier {
  let active: Bool
  @State private var scaled = false
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  func body(content: Content) -> some View {
    content
      .scaleEffect(scaled ? 1.3 : 1)
      .opacity(scaled ? 0.6 : 1)
      .onAppear {
        guard active, !reduceMotion else { return }
        withAnimation(.easeInOut(duration: 0.9).repeatForever(autoreverses: true)) { scaled = true }
      }
  }
}

// MARK: - Section header

struct AuraSectionHeader: View {
  let title: String
  var action: (() -> Void)?
  var actionTitle: String?

  var body: some View {
    HStack(alignment: .firstTextBaseline) {
      Text(title).font(AuraDesign.heading(19)).foregroundStyle(AuraDesign.ink)
      Spacer()
      if let actionTitle, let action {
        Button(action: action) {
          Text(actionTitle).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.6))
        }
        .buttonStyle(.plain)
      }
    }
  }
}

// MARK: - Live heart-rate pill (glass chrome)

struct AuraLiveHRPill: View {
  let bpm: Int?
  let deviceName: String
  let batteryPercent: Int?
  let bonded: Bool
  var action: () -> Void = {}

  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    Button(action: action) {
      HStack(spacing: 11) {
        Image(systemName: "heart.fill")
          .font(.system(size: 15, weight: .semibold))
          .foregroundStyle(bonded ? AuraDesign.bad : AuraDesign.ink.opacity(0.5))
          .symbolEffect(.pulse, options: .repeating, isActive: bpm != nil && !reduceMotion)

        HStack(alignment: .firstTextBaseline, spacing: 4) {
          Text(bpm.map { "\($0)" } ?? "--")
            .font(AuraDesign.number(22))
            .foregroundStyle(AuraDesign.ink)
            .contentTransition(.numericText())
          Text("bpm").font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
        }

        Text(deviceName).font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.55)).lineLimit(1)
        Spacer(minLength: 8)

        if let batteryPercent {
          Text("\(batteryPercent)%")
            .font(AuraDesign.caption)
            .foregroundStyle(batteryPercent <= 20 ? AuraDesign.bad : AuraDesign.ink.opacity(0.55))
        }
      }
      .padding(.horizontal, 18)
      .padding(.vertical, 14)
      .frame(maxWidth: .infinity, alignment: .leading)
      .auraGlass(.capsule, interactive: true)
      .contentShape(.capsule)
    }
    .buttonStyle(AuraPressStyle())
    .accessibilityLabel(Text("Live heart rate"))
    .accessibilityValue(Text(bpm.map { "\($0)" } ?? "--"))
  }
}
