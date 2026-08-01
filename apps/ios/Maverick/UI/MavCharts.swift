import SwiftUI

// Everything that draws data. Each type takes an `accessibilitySummary` it cannot be constructed
// without, because a chart with no text description is unusable and a required parameter is a
// better reviewer than a checklist.
//
// Every chart is monochrome. Series are distinguished by shape, stroke weight and labels, never by
// unrelated category colours.

// MARK: - Arc gauge

/// The geometry of the open-bottom arc, in one place because both platforms draw it and a mock
/// once shipped with an assumed arc length that silently pinned every value above 0.86 to full.
///
/// The gap is centred on the bottom of the circle and is wide enough to read as deliberately open
/// rather than as a ring that failed to close. Everything else is derived from it.
enum MavArc {
  /// Half the chord across the opening, as a fraction of the radius.
  static let openingRatio: Double = 25.0 / 30.0
  /// How much of the circle the gap takes, in degrees.
  static var gapDegrees: Double { 2 * asin(openingRatio) * 180 / .pi }
  /// How much of the circle the arc covers. 247.07°.
  static var sweepDegrees: Double { 360 - gapDegrees }
  /// Where the arc starts, measured the way SwiftUI measures: 0° at three o'clock, increasing
  /// clockwise on screen. 90° is the bottom, so the arc starts half a gap past it.
  static var startDegrees: Double { 90 + gapDegrees / 2 }
}

private struct MavArcShape: Shape {
  func path(in rect: CGRect) -> Path {
    var path = Path()
    path.addArc(
      center: CGPoint(x: rect.midX, y: rect.midY),
      radius: min(rect.width, rect.height) / 2 - 2,
      startAngle: .degrees(MavArc.startDegrees),
      endAngle: .degrees(MavArc.startDegrees + MavArc.sweepDegrees),
      clockwise: false)
    return path
  }
}

/// An open-bottom arc. Deliberately not a closed ring, and deliberately not stacked with others:
/// the rail is a scrolling row of separate gauges, which is a different object from a three-ring
/// card and stays a different object.
struct MavArcGauge: View {
  let text: String
  let label: String
  /// 0...1, or nil when the metric has no value — which draws a dashed track and an em dash.
  let fraction: Double?
  let family: MavFamily
  let accessibilitySummary: String
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var progress = 0.0

  var body: some View {
    VStack(spacing: 0) {
      ZStack {
        ZStack {
          MavArcShape()
            .stroke(
              MavTheme.ink.opacity(0.18),
              style: StrokeStyle(lineWidth: 4, lineCap: .round))
          if fraction != nil {
            // `trim` works on the path's own length, so the fill is exact without anyone having to
            // know how long the arc is.
            MavArcShape()
              .trim(from: 0, to: min(max(progress, 0), 1))
              .stroke(family.hue, style: StrokeStyle(lineWidth: 4, lineCap: .round))
          }
        }
        .frame(width: 68, height: 68)
        .offset(y: -2)

        Text(text)
          .mavType(.numeralSmall)
          .foregroundStyle(MavTheme.ink)
          .opacity(fraction == nil ? 0.72 : 1)
          .padding(.bottom, 6)
      }
      .frame(width: 74, height: 68)

      Text(label)
        .mavType(.sub)
        .foregroundStyle(MavTheme.inkSecondary)
        .multilineTextAlignment(.center)
        .lineLimit(2)
        .frame(width: 78)
        .frame(minHeight: 32, alignment: .top)
    }
    .frame(minWidth: 78, minHeight: 108, alignment: .top)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(accessibilitySummary)
    .onAppear { updateProgress(fraction) }
    .onChange(of: fraction) { _, value in updateProgress(value) }
  }

  private func updateProgress(_ value: Double?) {
    let target = min(max(value ?? 0, 0), 1)
    if reduceMotion {
      progress = target
    } else {
      withAnimation(MavTheme.calm) { progress = target }
    }
  }
}

// MARK: - Baseline range bar

/// The value against the core's own normal range. The band and the marker are plain ink: the card
/// underneath already says whether the number is good, and saying it twice is how a screen starts
/// shouting.
struct MavBaselineBar: View {
  let band: MavBand
  let lowText: String
  let highText: String
  let accessibilitySummary: String
  /// The metric this belongs to, so the band is drawn in that metric's step of the hue.
  var family: MavFamily = .vitals

  /// Whether today's value sits inside the wearer's own normal range. This is the whole question
  /// the row is asking, so it is answered by the shape rather than left to be inferred from a
  /// marker's position against a hairline.
  private var isInRange: Bool {
    band.markerFraction >= band.lowFraction && band.markerFraction <= band.highFraction
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 7) {
      GeometryReader { proxy in
        let width = proxy.size.width
        ZStack(alignment: .leading) {
          // The full span, recessed. Thin, because it is context rather than content.
          Capsule().fill(MavTheme.hairline).frame(height: 6)

          // The normal range, in the metric's own colour and unmistakably a *region* rather than
          // a line. The previous version drew this at 3pt in ink-at-30%, which read as a slightly
          // darker part of the track and answered nothing.
          Capsule()
            .fill(family.hue.opacity(0.5))
            .frame(
              width: max((band.highFraction - band.lowFraction) * width, 6),
              height: 6)
            .offset(x: band.lowFraction * width)

          // Where the value actually fell. Ink, so it is the one thing on the bar that is not the
          // hue, and ringed in the surface colour so it stays legible wherever it lands.
          Circle()
            .fill(MavTheme.ink)
            .frame(width: 15, height: 15)
            .overlay { Circle().strokeBorder(MavTheme.surface, lineWidth: 3) }
            // A value outside the range gets a halo, so "unusual" is visible at a glance and
            // without relying on the reader comparing two x-positions.
            .overlay {
              if !isInRange {
                Circle().strokeBorder(MavTheme.ink.opacity(0.35), lineWidth: 2).padding(-4)
              }
            }
            .offset(x: band.markerFraction * width - 7.5)
        }
        .frame(height: 15)
        .frame(maxHeight: .infinity, alignment: .center)
      }
      .frame(height: 15)

      HStack(spacing: 0) {
        Text(lowText)
        Spacer(minLength: 6)
        // The band is named, not just drawn. "Normal range" is the thing the coloured region means
        // and it costs one line to say so.
        Text(isInRange ? "in range" : "outside range")
          .foregroundStyle(MavTheme.inkSecondary)
        Spacer(minLength: 6)
        Text(highText)
      }
      .mavType(.caption)
      .monospacedDigit()
      .foregroundStyle(MavTheme.inkSecondary)
    }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(accessibilitySummary)
  }
}

// MARK: - Sparkline

struct MavSparkline: View {
  let values: [Double]
  let family: MavFamily
  let accessibilitySummary: String

  var body: some View {
    Canvas { context, size in
      guard values.count > 1 else { return }
      let lowest = values.min() ?? 0
      let highest = values.max() ?? 1
      let span = max(highest - lowest, 0.0001)
      let step = size.width / CGFloat(values.count - 1)

      let positions = values.enumerated().map { index, value in
        CGPoint(
          x: CGFloat(index) * step,
          y: size.height - CGFloat((value - lowest) / span) * (size.height - 10) - 5)
      }
      let line = MavChartPath.smooth(positions)
      context.stroke(
        line, with: .color(family.hue),
        style: StrokeStyle(lineWidth: 2.25, lineCap: .round, lineJoin: .round))

      if let last = positions.last {
        context.fill(
          Path(ellipseIn: CGRect(x: last.x - 3.5, y: last.y - 3.5, width: 7, height: 7)),
          with: .color(family.hue))
      }
    }
    .frame(height: 54)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(accessibilitySummary)
  }
}

struct MavCycleHistoryChart: View {
  let lengths: [Int]
  let accessibilitySummary: String

  private var bounds: ClosedRange<Double> {
    // Cycle lengths need a stable human scale. Auto-zooming 28...29 to fill the chart made a
    // one-day difference look enormous.
    let low = 20.0
    let high = max(36.0, Double((lengths.max() ?? 34) + 2))
    return low...high
  }

  private var median: Double {
    let sorted = lengths.sorted()
    guard !sorted.isEmpty else { return 0 }
    if sorted.count.isMultiple(of: 2) {
      return Double(sorted[sorted.count / 2 - 1] + sorted[sorted.count / 2]) / 2
    }
    return Double(sorted[sorted.count / 2])
  }

  var body: some View {
    VStack(spacing: 10) {
      GeometryReader { proxy in
        let height = proxy.size.height
        let span = max(bounds.upperBound - bounds.lowerBound, 1)
        let medianY = height - CGFloat((median - bounds.lowerBound) / span) * height
        ZStack(alignment: .bottom) {
          Path { path in
            path.move(to: CGPoint(x: 0, y: medianY))
            path.addLine(to: CGPoint(x: proxy.size.width, y: medianY))
          }
          .stroke(MavTheme.inkSecondary, style: StrokeStyle(lineWidth: 1, dash: [4, 5]))

          HStack(alignment: .bottom, spacing: 10) {
            ForEach(Array(lengths.enumerated()), id: \.offset) { _, value in
              VStack(spacing: 6) {
                Text("\(value)")
                  .mavType(.caption)
                  .monospacedDigit()
                  .foregroundStyle(MavTheme.ink)
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                  .fill(MavTheme.ink)
                  .frame(
                    width: 38,
                    height: max(
                      CGFloat((Double(value) - bounds.lowerBound) / span) * (height - 24),
                      8))
              }
              .frame(maxWidth: .infinity)
            }
          }
        }
      }
      .frame(height: 132)

      HStack {
        ForEach(lengths.indices, id: \.self) { index in
          Text("C\(index + 1)")
            .mavType(.caption)
            .foregroundStyle(MavTheme.inkSecondary)
            .frame(maxWidth: .infinity)
        }
      }
    }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(accessibilitySummary)
  }
}

// MARK: - Series chart

/// The scrubbable chart on a metric detail. The normal band is a wash behind the line, so "is this
/// normal for me" is answered by position rather than by a colour.
struct MavSeriesChart: View {
  struct Point: Identifiable, Equatable {
    let label: String
    let value: Double
    var id: String { label }
  }

  let points: [Point]
  let band: (low: Double, high: Double)?
  let family: MavFamily
  let accessibilitySummary: String
  @Binding var selection: Int?

  private var bounds: (low: Double, high: Double) {
    let values = points.map(\.value) + (band.map { [$0.low, $0.high] } ?? [])
    let lowest = values.min() ?? 0
    let highest = values.max() ?? 1
    let pad = max((highest - lowest) * 0.15, 0.5)
    return (lowest - pad, highest + pad)
  }

  /// Value → y, in the chart's own coordinate space.
  private func y(_ value: Double, in size: CGSize) -> CGFloat {
    let (low, high) = bounds
    let span = max(high - low, 0.0001)
    return size.height - CGFloat((value - low) / span) * size.height
  }

  private func step(in size: CGSize) -> CGFloat {
    points.count > 1 ? size.width / CGFloat(points.count - 1) : size.width
  }

  var body: some View {
    GeometryReader { proxy in
      let size = proxy.size
      let step = step(in: size)

      ZStack(alignment: .topLeading) {
        if let band {
          Rectangle()
            .fill(MavTheme.ink.opacity(0.05))
            .frame(height: max(y(band.low, in: size) - y(band.high, in: size), 1))
            .offset(y: y(band.high, in: size))
        }

        Canvas { context, _ in
          for fraction in [0.12, 0.5, 0.88] {
            var line = Path()
            line.move(to: CGPoint(x: 0, y: size.height * fraction))
            line.addLine(to: CGPoint(x: size.width, y: size.height * fraction))
            context.stroke(line, with: .color(MavTheme.grid), lineWidth: 1)
          }

          guard points.count > 1 else { return }
          let positions = points.enumerated().map { index, point in
            CGPoint(x: CGFloat(index) * step, y: y(point.value, in: size))
          }
          let line = MavChartPath.smooth(positions)
          context.stroke(
            line, with: .color(family.hue),
            style: StrokeStyle(lineWidth: 2.5, lineCap: .round, lineJoin: .round))
          if positions.count <= 12 {
            for position in positions {
              context.fill(
                Path(ellipseIn: CGRect(x: position.x - 2, y: position.y - 2, width: 4, height: 4)),
                with: .color(family.hue.opacity(0.72)))
            }
          }
        }

        if let selection, points.indices.contains(selection) {
          Rectangle()
            .fill(MavTheme.ink.opacity(0.3))
            .frame(width: 1, height: size.height)
            .offset(x: CGFloat(selection) * step)
          Circle()
            .fill(MavTheme.ink)
            .frame(width: 9, height: 9)
            .overlay { Circle().strokeBorder(MavTheme.canvas, lineWidth: 2) }
            .offset(
              x: CGFloat(selection) * step - 4.5,
              y: y(points[selection].value, in: size) - 4.5)
        }
      }
      .contentShape(.rect)
      .gesture(
        DragGesture(minimumDistance: 0)
          .onChanged { value in
            guard points.count > 1 else { return }
            let index = Int((value.location.x / step).rounded())
            selection = min(max(index, 0), points.count - 1)
          }
          .onEnded { _ in }
      )
    }
    .frame(height: 204)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(accessibilitySummary)
    .accessibilityValue(
      selection.flatMap { points.indices.contains($0) ? points[$0] : nil }
        .map { "\($0.label), \($0.value.formatted(.number.precision(.fractionLength(0))))" } ?? "")
    .accessibilityAdjustableAction { direction in
      guard !points.isEmpty else { return }
      let current = selection ?? points.count - 1
      switch direction {
      case .increment: selection = min(current + 1, points.count - 1)
      case .decrement: selection = max(current - 1, 0)
      @unknown default: break
      }
    }
  }
}

private enum MavChartPath {
  /// A restrained Catmull–Rom conversion. It rounds the joins without inventing the dramatic
  /// overshoot that made the old graphs look synthetic.
  static func smooth(_ points: [CGPoint]) -> Path {
    guard let first = points.first else { return Path() }
    guard points.count > 2 else {
      var path = Path()
      path.move(to: first)
      points.dropFirst().forEach { path.addLine(to: $0) }
      return path
    }
    var path = Path()
    path.move(to: first)
    for index in 0..<(points.count - 1) {
      let p0 = points[max(index - 1, 0)]
      let p1 = points[index]
      let p2 = points[index + 1]
      let p3 = points[min(index + 2, points.count - 1)]
      let control1 = CGPoint(
        x: p1.x + (p2.x - p0.x) / 6,
        y: p1.y + (p2.y - p0.y) / 6)
      let control2 = CGPoint(
        x: p2.x - (p3.x - p1.x) / 6,
        y: p2.y - (p3.y - p1.y) / 6)
      path.addCurve(to: p2, control1: control1, control2: control2)
    }
    return path
  }
}

// MARK: - Zone ladder

/// Time in each heart-rate zone. One row per zone, hardest at the top, one quantity, bar length
/// relative to the biggest zone that week.
///
/// This replaces a stacked bar that borrowed five unrelated family hues and printed a per-zone
/// "target" beside each one. Nothing in the core admits a weekly zone target, so that second number
/// was invented, and it is gone.
struct MavZoneLadder: View {
  struct Zone: Identifiable, Equatable {
    let number: Int
    let name: String
    /// The bpm bounds, which are shown because they come from a measured maximum.
    let bounds: String
    let minutes: Int
    var id: Int { number }
  }

  let zones: [Zone]
  let accessibilitySummary: String

  private var largest: Int { max(zones.map(\.minutes).max() ?? 0, 1) }

  var body: some View {
    VStack(spacing: 13) {
      ForEach(zones.sorted { $0.number > $1.number }) { zone in
        HStack(spacing: 12) {
          VStack(alignment: .leading, spacing: 2) {
            Text("\(zone.number) · \(zone.name)")
              .mavType(.sub)
              .foregroundStyle(MavTheme.inkSecondary)
              .lineLimit(1)
            Text(zone.bounds)
              .mavType(.sub)
              .monospacedDigit()
              .foregroundStyle(MavTheme.inkSecondary)
              .opacity(0.85)
              .lineLimit(1)
          }
          // Wide enough for "4 · Threshold" on one line. The word "Zone" moved to the section
          // heading rather than being repeated five times down the column.
          .frame(width: 118, alignment: .leading)

          GeometryReader { proxy in
            ZStack(alignment: .leading) {
              Capsule().fill(MavTheme.hairline)
              Capsule()
                .fill(MavTheme.ink.opacity(0.3 + 0.14 * Double(zone.number)))
                .frame(
                  width: max(proxy.size.width * Double(zone.minutes) / Double(largest), 0))
            }
          }
          .frame(height: 6)

          Text("\(zone.minutes)m")
            .mavType(.sub)
            .monospacedDigit()
            .foregroundStyle(MavTheme.ink)
            .frame(width: 44, alignment: .trailing)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
          "Zone \(zone.number), \(zone.name), \(zone.bounds), \(zone.minutes) minutes")
      }
    }
    .accessibilityLabel(accessibilitySummary)
  }
}

// MARK: - Week strip

/// Seven days of load. Selection is weight, not hue: the selected bar is full-strength ink and the
/// rest are faint. Selection is not an affirmative action, so it does not get the accent.
struct MavWeekStrip: View {
  struct Day: Identifiable, Equatable {
    let letter: String
    let full: String
    /// 0...1 of the tallest bar.
    let fraction: Double
    var minutes: Int = 0
    let summary: String
    var id: String { full }
  }

  let days: [Day]
  let selected: Int
  let onSelect: (Int) -> Void

  var body: some View {
    HStack(alignment: .bottom, spacing: 4) {
      ForEach(Array(days.enumerated()), id: \.element.id) { index, day in
        Button {
          onSelect(index)
        } label: {
          VStack(spacing: 8) {
            Spacer(minLength: 0)
            Text(day.minutes > 0 ? "\(day.minutes)" : "")
              .mavType(.caption)
              .monospacedDigit()
              .foregroundStyle(
                index == selected ? MavTheme.ink : MavTheme.inkSecondary)
              .frame(height: 18)
            RoundedRectangle(cornerRadius: 5, style: .continuous)
              // A bar is a data mark, so it is the hue rather than ink. Selection is carried by
              // weight — the chosen day is the hue at full strength, the rest are a wash of it.
              .fill(MavFamily.effort.hue.opacity(index == selected ? 1 : 0.28))
              .frame(height: max(day.fraction * 76, 3))
            Text(day.letter)
              .mavType(.sub)
              .foregroundStyle(index == selected ? MavTheme.ink : MavTheme.inkSecondary)
          }
          .frame(maxWidth: .infinity, minHeight: 126)
          .contentShape(.rect)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(day.summary)
        .accessibilityAddTraits(index == selected ? [.isButton, .isSelected] : .isButton)
      }
    }
  }
}
