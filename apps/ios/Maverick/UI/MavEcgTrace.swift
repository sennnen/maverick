import SwiftUI

/// The recorded trace on ECG graph paper (ADR-034).
///
/// The wearer is being told something about their heart; the waveform is the only part of that
/// they can judge for themselves, so it leads rather than being withheld. Drawn at a true time
/// base against a 1 mm / 5 mm grid, the same geometry as the report, so the screen and the PDF
/// cannot disagree about what was recorded.
struct MavEcgTrace: View {
  let waveform: [Float]
  let sampleRateHz: Int
  var height: CGFloat = 168
  /// Seconds shown per screen width. The view scrolls to reach the rest.
  var secondsPerScreen: CGFloat = 4
  var scrollable = true

  private static let millimetresPerSecond: CGFloat = 25
  private static let millimetresPerMillivolt: CGFloat = 10

  private var seconds: CGFloat {
    sampleRateHz > 0 ? CGFloat(waveform.count) / CGFloat(sampleRateHz) : 0
  }

  var body: some View {
    if waveform.isEmpty || sampleRateHz <= 0 {
      EmptyView()
    } else if scrollable {
      GeometryReader { proxy in
        let millimetre = proxy.size.width / (secondsPerScreen * Self.millimetresPerSecond)
        ScrollView(.horizontal, showsIndicators: false) {
          canvas(millimetre: millimetre)
            .frame(width: seconds * Self.millimetresPerSecond * millimetre, height: height)
        }
      }
      .frame(height: height)
    } else {
      GeometryReader { proxy in
        canvas(millimetre: proxy.size.width / (seconds * Self.millimetresPerSecond))
      }
      .frame(height: height)
    }
  }

  private func canvas(millimetre: CGFloat) -> some View {
    Canvas { context, size in
      paper(&context, size: size, millimetre: millimetre)
      trace(&context, size: size, millimetre: millimetre)
    }
    .accessibilityElement()
    .accessibilityLabel(
      "Electrocardiogram trace, \(Int(seconds.rounded())) seconds, \(sampleRateHz) hertz, "
        + "on standard ECG paper."
    )
  }

  /// One grid token, two weights: the app's paper stays in the theme's register while the report
  /// uses conventional ECG red. Same geometry, different surface.
  private func paper(_ context: inout GraphicsContext, size: CGSize, millimetre: CGFloat) {
    guard millimetre > 0 else { return }
    let fine = MavTheme.grid.opacity(0.55)
    let bold = MavTheme.grid
    var index = 0
    while CGFloat(index) * millimetre <= size.width {
      let x = CGFloat(index) * millimetre
      let heavy = index % 5 == 0
      var line = Path()
      line.move(to: CGPoint(x: x, y: 0))
      line.addLine(to: CGPoint(x: x, y: size.height))
      context.stroke(line, with: .color(heavy ? bold : fine), lineWidth: heavy ? 0.7 : 0.4)
      index += 1
    }
    index = 0
    while CGFloat(index) * millimetre <= size.height {
      let y = CGFloat(index) * millimetre
      let heavy = index % 5 == 0
      var line = Path()
      line.move(to: CGPoint(x: 0, y: y))
      line.addLine(to: CGPoint(x: size.width, y: y))
      context.stroke(line, with: .color(heavy ? bold : fine), lineWidth: heavy ? 0.7 : 0.4)
      index += 1
    }
  }

  private func trace(_ context: inout GraphicsContext, size: CGSize, millimetre: CGFloat) {
    let finite = waveform.filter(\.isFinite)
    guard !finite.isEmpty else { return }
    let centre = finite.sorted()[finite.count / 2]
    // Millivolt samples get the clinical 10 mm/mV. Anything else has no established scale, so the
    // trace is fitted to the panel rather than implying a calibration nobody set.
    let perMillivolt = millimetre * Self.millimetresPerMillivolt
    let span = CGFloat(finite.map { abs($0 - centre) }.max() ?? 1)
    let gain = span * perMillivolt > size.height / 2 ? (size.height / 2) / max(span, 0.000_001)
      : perMillivolt
    let midpoint = size.height / 2
    var path = Path()
    var started = false
    for (index, sample) in waveform.enumerated() where sample.isFinite {
      let x =
        CGFloat(index) / CGFloat(sampleRateHz) * Self.millimetresPerSecond * millimetre
      let y = min(max(midpoint - CGFloat(sample - centre) * gain, 0), size.height)
      if started {
        path.addLine(to: CGPoint(x: x, y: y))
      } else {
        path.move(to: CGPoint(x: x, y: y))
        started = true
      }
    }
    context.stroke(path, with: .color(MavTheme.accent), lineWidth: 1.4)
  }
}
