import SwiftUI

// The Aura data-viz layer. Everything here is interactive: graphs scrub with a
// finger (value + date readout, haptic ticks), the hypnogram scrubs to the
// stage under your finger, zone bars expand on tap. All of it lives on dark
// cards (glow tiles are hero-only) and uses the adaptive contrast tokens.

// MARK: - Dark card (tier-2 surface)

extension View {
  /// The one non-glow card style: dark neutral surface + adaptive hairline.
  func auraDarkCard(padding: CGFloat = 18) -> some View {
    self
      .padding(padding)
      .frame(maxWidth: .infinity, alignment: .leading)
      .background(AuraDesign.card, in: AuraDesign.tileShape)
      .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
  }
}

// MARK: - Score ring

struct AuraScoreRing: View {
  var value: Double?
  var maxValue: Double = 100
  var text: String
  var unit: String = ""
  var label: String
  var status: AuraStatus
  /// When set, the ring wears this FAMILY hue instead of the status colour
  /// (for informational metrics like Effort where high ≠ bad).
  var tintOverride: Color?
  var size: CGFloat = 168
  var lineWidth: CGFloat = 9

  @State private var fill: Double = 0
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  private var target: Double { min(max((value ?? 0) / maxValue, 0), 1) }
  private var tint: Color { tintOverride ?? status.color }

  var body: some View {
    ZStack {
      Circle().stroke(AuraDesign.ink.opacity(0.12), lineWidth: lineWidth)
      Circle()
        .trim(from: 0, to: fill)
        .stroke(tint, style: StrokeStyle(lineWidth: lineWidth, lineCap: .round))
        .rotationEffect(.degrees(-90))
        .shadow(color: tint.opacity(0.55), radius: 10)
      VStack(spacing: 1) {
        HStack(alignment: .firstTextBaseline, spacing: 2) {
          Text(text)
            .font(AuraDesign.number(size * 0.28))
            .foregroundStyle(AuraDesign.ink)
            .lineLimit(1).minimumScaleFactor(0.4)
          if !unit.isEmpty, value != nil {
            Text(unit).font(AuraDesign.caption)
              .foregroundStyle(AuraDesign.ink.opacity(0.6))
          }
        }
        Text(label).font(AuraDesign.caption)
          .foregroundStyle(AuraDesign.ink.opacity(0.65))
          .lineLimit(1).minimumScaleFactor(0.7)
      }
      .padding(.horizontal, lineWidth + 8)
      .frame(maxWidth: size)
    }
    .frame(width: size, height: size)
    .onAppear { animate(to: target, initial: true) }
    .onChange(of: target) { _, t in animate(to: t, initial: false) }
    .accessibilityLabel(Text(label))
    .accessibilityValue(Text(text))
  }

  private func animate(to t: Double, initial: Bool) {
    if reduceMotion { fill = t; return }
    withAnimation(.spring(response: initial ? 0.8 : 0.5, dampingFraction: 0.85)) { fill = t }
  }
}

// MARK: - AuraGraph — the interactive trend chart

/// A labelled, scrubbable series chart: y-gridlines with values, a dashed
/// average line, date axis, the latest point emphasised, and a drag-to-scrub
/// readout (value + full date) with haptic ticks. `.line` draws a smoothed
/// line + soft area; `.bars` draws rounded columns.
struct AuraGraph: View {
  enum Style { case line, bars }

  /// (dayKey yyyy-MM-dd, value), oldest → newest, already range-clipped.
  let points: [(day: String, value: Double)]
  var tint: Color
  var unit: String = ""
  var style: Style = .line
  var decimals: Int = 0
  var height: CGFloat = 150
  /// Extra context per point (e.g. "8h 12m") shown in the scrub readout.
  var detail: ((Int) -> String)? = nil

  @State private var scrub: Int?
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      if points.count > 1 {
        readout
        chart
        axis
      } else {
        VStack(spacing: 6) {
          Image(systemName: "chart.xyaxis.line")
            .font(.system(size: 22)).foregroundStyle(AuraDesign.ink.opacity(0.3))
          Text("Not enough history yet")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
        }
        .frame(maxWidth: .infinity, minHeight: height)
      }
    }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text("Trend chart"))
    .accessibilityValue(Text(points.last.map { "\(fmt($0.value)) \(unit)" } ?? ""))
  }

  // MARK: Readout header

  private var values: [Double] { points.map(\.value) }
  private var avg: Double { values.reduce(0, +) / Double(max(values.count, 1)) }
  private var lo: Double { values.min() ?? 0 }
  private var hi: Double { values.max() ?? 1 }

  private var readout: some View {
    let i = scrub ?? points.count - 1
    let p = points[max(0, min(i, points.count - 1))]
    return HStack(alignment: .firstTextBaseline, spacing: 6) {
      Text(fmt(p.value))
        .font(AuraDesign.number(30)).foregroundStyle(AuraDesign.ink)
        .monospacedDigit()
        .contentTransition(.numericText())
        .animation(reduceMotion ? nil : .snappy(duration: 0.15), value: i)
      if !unit.isEmpty {
        Text(unit).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.6))
      }
      if let detail {
        Text(detail(i)).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.5))
      }
      Spacer(minLength: 8)
      VStack(alignment: .trailing, spacing: 2) {
        Text(scrub == nil ? "Latest" : Self.longDate(p.day))
          .font(AuraDesign.caption).foregroundStyle(tint)
        Text("avg \(fmt(avg)) · \(fmt(lo))–\(fmt(hi))")
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.5))
      }
    }
    .frame(minHeight: 34)
  }

  // MARK: Chart body

  private var chart: some View {
    GeometryReader { g in
      let plotW = g.size.width - 34   // reserve right gutter for y labels
      let h = g.size.height
      let range = Swift.max(hi - lo, 0.001)
      let pad = range * 0.08          // headroom so the line never kisses the edges
      let yLo = lo - pad, yRange = range + pad * 2
      let y: (Double) -> CGFloat = { v in h - CGFloat((v - yLo) / yRange) * h }
      let x: (Int) -> CGFloat = { i in
        points.count == 1 ? plotW / 2 : CGFloat(i) / CGFloat(points.count - 1) * plotW
      }

      ZStack(alignment: .topLeading) {
        // Y grid: min / mid / max lines + right-side value labels.
        ForEach([lo, (lo + hi) / 2, hi], id: \.self) { v in
          let yy = y(v)
          Path { p in p.move(to: .init(x: 0, y: yy)); p.addLine(to: .init(x: plotW, y: yy)) }
            .stroke(AuraDesign.grid, lineWidth: 1)
          Text(fmt(v))
            .font(.system(size: 9, weight: .medium))
            .foregroundStyle(AuraDesign.ink.opacity(0.45))
            .position(x: plotW + 18, y: yy)
        }

        // Average, dashed in the tint.
        let ay = y(avg)
        Path { p in p.move(to: .init(x: 0, y: ay)); p.addLine(to: .init(x: plotW, y: ay)) }
          .stroke(style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
          .foregroundStyle(tint.opacity(0.55))

        switch style {
        case .line:
          linePlot(x: x, y: y, plotW: plotW, h: h)
        case .bars:
          barPlot(y: y, plotW: plotW, h: h)
        }

        // Scrub cursor.
        if let s = scrub {
          let sx = style == .bars ? barCenter(s, plotW: plotW) : x(s)
          Path { p in p.move(to: .init(x: sx, y: 0)); p.addLine(to: .init(x: sx, y: h)) }
            .stroke(AuraDesign.ink.opacity(0.35), lineWidth: 1)
          Circle()
            .fill(tint)
            .frame(width: 11, height: 11)
            .overlay(Circle().strokeBorder(AuraDesign.bg, lineWidth: 2))
            .shadow(color: tint.opacity(0.9), radius: 6)
            .position(x: sx, y: y(points[s].value))
        }
      }
      .contentShape(Rectangle())
      .gesture(
        DragGesture(minimumDistance: 0)
          .onChanged { v in
            let i = index(at: v.location.x, plotW: plotW)
            if i != scrub {
              scrub = i
              if !reduceMotion { UISelectionFeedbackGenerator().selectionChanged() }
            }
          }
          .onEnded { _ in
            Task { try? await Task.sleep(for: .seconds(1.6))
                   withAnimation(.easeOut(duration: 0.2)) { scrub = nil } }
          }
      )
    }
    .frame(height: height)
  }

  private func index(at xPos: CGFloat, plotW: CGFloat) -> Int {
    guard points.count > 1 else { return 0 }
    let f = xPos / max(plotW, 1) * CGFloat(points.count - 1)
    return max(0, min(points.count - 1, Int(f.rounded())))
  }

  private func barCenter(_ i: Int, plotW: CGFloat) -> CGFloat {
    let bw = plotW / CGFloat(points.count)
    return CGFloat(i) * bw + bw / 2
  }

  @ViewBuilder
  private func linePlot(x: @escaping (Int) -> CGFloat, y: @escaping (Double) -> CGFloat,
                        plotW: CGFloat, h: CGFloat) -> some View {
    let pts = points.indices.map { CGPoint(x: x($0), y: y(points[$0].value)) }

    // Soft area under the line.
    smoothPath(pts, closedTo: h)
      .fill(LinearGradient(colors: [tint.opacity(0.30), tint.opacity(0.02)],
                           startPoint: .top, endPoint: .bottom))
    // The line itself.
    smoothPath(pts, closedTo: nil)
      .stroke(tint, style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))
      .shadow(color: tint.opacity(0.5), radius: 5)
    // Latest point, emphasised.
    if let last = pts.last {
      Circle()
        .fill(tint)
        .frame(width: 9, height: 9)
        .overlay(Circle().strokeBorder(AuraDesign.bg, lineWidth: 2))
        .position(last)
    }
  }

  /// Quad-smoothed path through the points (midpoint control), optionally
  /// closed down to `closedTo` (the baseline) for the area fill.
  private func smoothPath(_ pts: [CGPoint], closedTo: CGFloat?) -> Path {
    Path { p in
      guard let first = pts.first else { return }
      p.move(to: first)
      if pts.count == 2 {
        p.addLine(to: pts[1])
      } else {
        for i in 1..<pts.count {
          let prev = pts[i - 1], cur = pts[i]
          let mid = CGPoint(x: (prev.x + cur.x) / 2, y: (prev.y + cur.y) / 2)
          p.addQuadCurve(to: mid, control: prev)
        }
        if let last = pts.last { p.addLine(to: last) }
      }
      if let base = closedTo, let last = pts.last {
        p.addLine(to: CGPoint(x: last.x, y: base))
        p.addLine(to: CGPoint(x: first.x, y: base))
        p.closeSubpath()
      }
    }
  }

  @ViewBuilder
  private func barPlot(y: @escaping (Double) -> CGFloat, plotW: CGFloat, h: CGFloat) -> some View {
    let n = points.count
    let bw = plotW / CGFloat(n)
    ForEach(0..<n, id: \.self) { i in
      let top = y(points[i].value)
      let active = scrub == i || (scrub == nil && i == n - 1)
      RoundedRectangle(cornerRadius: 2.5, style: .continuous)
        .fill(tint.opacity(active ? 1 : 0.45))
        .frame(width: Swift.max(bw * 0.55, 1.5), height: Swift.max(h - top, 3))
        .position(x: barCenter(i, plotW: plotW), y: (h + top) / 2)
    }
  }

  // MARK: Axis

  private var axis: some View {
    HStack {
      Text(Self.shortDate(points.first?.day))
      Spacer()
      if points.count > 4 { Text(Self.shortDate(points[points.count / 2].day)); Spacer() }
      Text(Self.shortDate(points.last?.day))
    }
    .font(.system(size: 9, weight: .medium))
    .foregroundStyle(AuraDesign.ink.opacity(0.45))
    .padding(.trailing, 34)
  }

  // MARK: Format

  private func fmt(_ v: Double) -> String {
    decimals == 0 ? String(Int(v.rounded())) : String(format: "%.\(decimals)f", v)
  }

  private static let inFmt: DateFormatter = {
    let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd"; f.locale = .init(identifier: "en_US_POSIX"); return f
  }()
  static func shortDate(_ day: String?) -> String {
    guard let day, let d = inFmt.date(from: day) else { return "" }
    return d.formatted(.dateTime.day().month(.abbreviated))
  }
  static func longDate(_ day: String?) -> String {
    guard let day, let d = inFmt.date(from: day) else { return "" }
    return d.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated))
  }
}

// MARK: - Hypnogram — scrubbable sleep-stage timeline

struct AuraHypnogram: View {
  let segments: [StageSegment]
  var height: CGFloat = 132

  @State private var scrub: Int?   // index into sorted segments
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  static let rows: [(stage: String, label: String, tint: Color)] = [
    ("wake",  "Awake", AuraDesign.dyn(dark: 0xF5476A, light: 0xD83A44)),
    ("rem",   "REM",   AuraDesign.dyn(dark: 0x2BC8D9, light: 0x0F93A1)),
    ("light", "Light", AuraDesign.dyn(dark: 0x7FA5FF, light: 0x5B82D8)),
    ("deep",  "Deep",  AuraDesign.dyn(dark: 0x3E7BFF, light: 0x2F5FD0)),
  ]

  private var sorted: [StageSegment] { segments }
  private var start: Int { segments.map(\.start).min() ?? 0 }
  private var end: Int { segments.map(\.end).max() ?? 1 }

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      if end > start {
        readout
        chart
        ticks
        legend
      } else {
        Text("No staged sleep recorded")
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
          .frame(maxWidth: .infinity, minHeight: 80)
      }
    }
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(Text("Sleep stages"))
  }

  private var readout: some View {
    HStack(spacing: 8) {
      if let s = scrub {
        let seg = sorted[s]
        let row = Self.rows.first { $0.stage == seg.stage }
        Circle().fill(row?.tint ?? AuraDesign.ink).frame(width: 8, height: 8)
        Text(row?.label ?? seg.stage.capitalized)
          .font(AuraDesign.label).foregroundStyle(AuraDesign.ink)
        Text(mins(seg.end - seg.start))
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.6))
      } else {
        Text("\(mins(end - start)) asleep")
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.45))
      }
      Spacer(minLength: 0)
    }
    .frame(minHeight: 20)
  }

  private var chart: some View {
    GeometryReader { g in
        let span = CGFloat(end - start)
        let rowH = g.size.height / CGFloat(Self.rows.count)
        let rowY: (Int) -> CGFloat = { r in rowH * CGFloat(r) + rowH / 2 }

        ZStack(alignment: .topLeading) {
          ForEach(1..<Self.rows.count, id: \.self) { r in
            Rectangle().fill(AuraDesign.grid)
              .frame(width: g.size.width, height: 2)
              .offset(y: rowH * CGFloat(r))
          }

          ForEach(sorted.indices, id: \.self) { i in
            let s = sorted[i]
            if let r = Self.rows.firstIndex(where: { $0.stage == s.stage }) {
              let x0 = CGFloat(s.start - start) / span * g.size.width
              let w = CGFloat(s.end - s.start) / span * g.size.width
              Rectangle()
                .fill(Self.rows[r].tint.opacity(scrub == nil || scrub == i ? 1 : 0.35))
                .frame(width: max(w, 3), height: rowH * 0.52)
                .position(x: x0 + max(w, 3) / 2, y: rowY(r))
                .shadow(color: scrub == i ? Self.rows[r].tint.opacity(0.8) : .clear, radius: 5)
            }
          }

          // Step connectors between consecutive segments (on top of blocks).
          ForEach(1..<sorted.count, id: \.self) { i in
            let a = sorted[i - 1], b = sorted[i]
            if a.end == b.start,
               let ra = Self.rows.firstIndex(where: { $0.stage == a.stage }),
               let rb = Self.rows.firstIndex(where: { $0.stage == b.stage }), ra != rb {
              let xx = CGFloat(a.end - start) / span * g.size.width
              let blockHalf = rowH * 0.26
              let inset = max(blockHalf - 5, 1)
              let y0 = rowY(ra) + (ra < rb ? inset : -inset)
              let y1 = rowY(rb) + (ra < rb ? -inset : inset)
              let topTint = ra < rb ? Self.rows[ra].tint : Self.rows[rb].tint
              let botTint = ra < rb ? Self.rows[rb].tint : Self.rows[ra].tint
              Rectangle()
                .fill(LinearGradient(
                  gradient: Gradient(colors: [topTint, botTint]),
                  startPoint: .top,
                  endPoint: .bottom))
                .opacity(scrub == nil ? 1 : 0.3)
                .frame(width: 2.5, height: abs(y1 - y0))
                .position(x: xx, y: (y0 + y1) / 2)
            }
          }

          if let s = scrub {
            let seg = sorted[s]
            let mx = CGFloat(seg.start + (seg.end - seg.start) / 2 - start) / span * g.size.width
            Path { p in p.move(to: .init(x: mx, y: 0)); p.addLine(to: .init(x: mx, y: g.size.height)) }
              .stroke(AuraDesign.ink.opacity(0.25), lineWidth: 1)
          }
        }
        .contentShape(Rectangle())
        .gesture(
          DragGesture(minimumDistance: 0)
            .onChanged { v in
              let ts = start + Int(v.location.x / g.size.width * span)
              let hit = sorted.firstIndex { ts >= $0.start && ts < $0.end }
                ?? sorted.indices.min(by: { abs(sorted[$0].start - ts) < abs(sorted[$1].start - ts) })
              if hit != scrub {
                scrub = hit
                if !reduceMotion { UISelectionFeedbackGenerator().selectionChanged() }
              }
            }
            .onEnded { _ in
              Task { try? await Task.sleep(for: .seconds(1.6))
                     withAnimation(.easeOut(duration: 0.2)) { scrub = nil } }
            }
        )
      }
      .frame(height: height)
    }

  private var ticks: some View {
    HStack {
      Text(clock(start))
      Spacer()
      Text(clock(start + (end - start) / 2))
      Spacer()
      Text(clock(end))
    }
    .font(.system(size: 9, weight: .medium))
    .foregroundStyle(AuraDesign.ink.opacity(0.45))
  }

  private var legend: some View {
    HStack(spacing: 18) {
      ForEach(Self.rows, id: \.stage) { row in
        HStack(spacing: 5) {
          Circle().fill(row.tint).frame(width: 7, height: 7)
          Text(row.label)
            .font(.system(size: 10, weight: .medium))
            .foregroundStyle(AuraDesign.ink.opacity(0.7))
        }
      }
    }
    .frame(maxWidth: .infinity)
  }

  private func clock(_ ts: Int) -> String {
    Date(timeIntervalSince1970: TimeInterval(ts)).formatted(date: .omitted, time: .shortened)
  }
  private func mins(_ secs: Int) -> String {
    let m = secs / 60
    return m >= 60 ? "\(m / 60)h \(m % 60)m" : "\(m)m"
  }
}

// MARK: - HR zone bars — tap to inspect

struct AuraZoneBars: View {
  /// Minutes per zone, index 0 = Z1 … 4 = Z5.
  let minutes: [Double]
  /// When provided, each zone shows its bpm band (50–60% … 90–100% of max).
  var hrMax: Int?
  /// Optional per-zone target minutes (§6): a targeted zone's bar fills AGAINST its
  /// target (full bar = target reached, checkmark appears); untargeted zones keep the
  /// relative-to-max scaling. nil entries = no target for that zone.
  var targets: [Double?]?

  @State private var selected: Int?
  @State private var appeared = false
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  static let tints: [Color] = [
    AuraDesign.dyn(dark: 0x8E9BA8, light: 0x7B8894),
    AuraDesign.dyn(dark: 0x2BC8D9, light: 0x0F93A1),
    AuraDesign.dyn(dark: 0x14C078, light: 0x1F9E57),
    AuraDesign.dyn(dark: 0xE0A81E, light: 0xC4841A),
    AuraDesign.dyn(dark: 0xF5476A, light: 0xD83A44),
  ]
  private static let names = ["Recovery", "Endurance", "Aerobic", "Threshold", "Max"]

  var body: some View {
    let total = max(minutes.reduce(0, +), 0.001)
    let maxMin = max(minutes.max() ?? 1, 1)
    VStack(alignment: .leading, spacing: 4) {
      ForEach(minutes.indices, id: \.self) { i in
        let isSel = selected == i
        Button {
          withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) {
            selected = isSel ? nil : i
          }
          if !reduceMotion { UISelectionFeedbackGenerator().selectionChanged() }
        } label: {
          VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 12) {
              Text("Z\(i + 1)")
                .font(.system(size: 11, weight: .semibold)).monospacedDigit()
                .foregroundStyle(isSel ? Self.tints[i] : AuraDesign.ink.opacity(0.6))
                .frame(width: 24, alignment: .leading)
              GeometryReader { g in
                let target = targets?[safe: i] ?? nil
                let fraction = target.map { min(minutes[i] / max($0, 0.001), 1) } ?? (minutes[i] / maxMin)
                ZStack(alignment: .leading) {
                  Capsule().fill(AuraDesign.ink.opacity(0.10)).frame(height: 9)
                  Capsule().fill(Self.tints[i])
                    .frame(width: appeared ? max(9, g.size.width * fraction) : 9,
                           height: 9)
                    .shadow(color: isSel ? Self.tints[i].opacity(0.8) : .clear, radius: 5)
                }
                .frame(height: g.size.height)
              }
              .frame(height: 14)
              HStack(spacing: 4) {
                Text(AuraZoneBars.hm(minutes[i]))
                  .font(.system(size: 11, weight: .medium)).monospacedDigit()
                  .foregroundStyle(AuraDesign.ink.opacity(0.8))
                if let t = targets?[safe: i] ?? nil, minutes[i] >= t {
                  Image(systemName: "checkmark")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(AuraDesign.good)
                }
              }
              .frame(width: 62, alignment: .trailing)
            }
            if isSel {
              Text(detailLine(i, total: total))
                .font(AuraDesign.caption)
                .foregroundStyle(AuraDesign.ink.opacity(0.6))
                .padding(.leading, 36)
                .transition(.opacity.combined(with: .move(edge: .top)))
            }
          }
          .padding(.vertical, 5)
          .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text("Zone \(i + 1)"))
        .accessibilityValue(Text(AuraZoneBars.hm(minutes[i])))
      }
    }
    .onAppear {
      guard !appeared else { return }
      if reduceMotion { appeared = true }
      else { withAnimation(.spring(response: 0.7, dampingFraction: 0.85)) { appeared = true } }
    }
  }

  private func detailLine(_ i: Int, total: Double) -> String {
    var s = "\(Self.names[i]) · \(Int((minutes[i] / total * 100).rounded()))% of session"
    if let hrMax {
      let loP = 50 + i * 10, hiP = 60 + i * 10
      s += " · \(loP * hrMax / 100)–\(hiP * hrMax / 100) bpm"
    }
    return s
  }

  static func hm(_ m: Double) -> String {
    let t = Int(m.rounded())
    return t >= 60 ? "\(t / 60)h \(t % 60)m" : "\(t)m"
  }
}

// MARK: - Range picker

enum AuraTrendRange: String, CaseIterable, Identifiable {
  case week = "1W", month = "1M", sixMonths = "6M"
  var id: String { rawValue }
  var days: Int { switch self { case .week: 7; case .month: 30; case .sixMonths: 182 } }
}

struct AuraRangePicker: View {
  @Binding var selection: AuraTrendRange

  var body: some View {
    HStack(spacing: 4) {
      ForEach(AuraTrendRange.allCases) { r in
        let active = r == selection
        Button {
          withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) { selection = r }
        } label: {
          Text(r.rawValue)
            .font(AuraDesign.caption)
            .foregroundStyle(active ? Color.black : AuraDesign.ink.opacity(0.65))
            .padding(.horizontal, 14)
            .padding(.vertical, 7)
            .background(active ? AuraDesign.accent : .clear, in: Capsule())
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(active ? [.isButton, .isSelected] : .isButton)
      }
    }
    .padding(3)
    .background(AuraDesign.ink.opacity(0.08), in: Capsule())
  }
}

// MARK: - Nav row

struct AuraNavRow: View {
  let icon: String
  let title: String
  var detail: String = ""
  var tint: Color = AuraDesign.ink.opacity(0.85)
  var action: () -> Void

  var body: some View {
    Button(action: action) {
      HStack(spacing: 14) {
        Image(systemName: icon)
          .font(.system(size: 16, weight: .medium))
          .foregroundStyle(tint)
          .frame(width: 26)
        Text(title).font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
        Spacer(minLength: 8)
        if !detail.isEmpty {
          Text(detail).font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.5)).lineLimit(1)
        }
        Image(systemName: "chevron.right")
          .font(.system(size: 12, weight: .semibold))
          .foregroundStyle(AuraDesign.ink.opacity(0.35))
      }
      .padding(.horizontal, 18)
      .padding(.vertical, 15)
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
  }
}

// MARK: - Sparkline (mini line, no axes)

struct AuraSparkline: View {
  let values: [Double]
  let color: Color
  var height: CGFloat = 32

  var body: some View {
    if values.count < 2 {
      Text("No data")
        .font(AuraDesign.caption).foregroundStyle(color.opacity(0.5))
        .frame(maxWidth: .infinity, maxHeight: height, alignment: .leading)
    } else {
      GeometryReader { g in
        let w = g.size.width
        let h = g.size.height
        let lo = values.min() ?? 0
        let hi = values.max() ?? 1
        let range = max(hi - lo, 0.001)
        let pad = range * 0.1
        let yLo = lo - pad
        let yHi = hi + pad
        let yRange = Swift.max(yHi - yLo, 0.001)
        let xStep = w / CGFloat(values.count - 1)
        Path { path in
          for (i, v) in values.enumerated() {
            let pt = CGPoint(x: CGFloat(i) * xStep,
                             y: h - CGFloat((v - yLo) / yRange) * h)
            if i == 0 { path.move(to: pt) } else { path.addLine(to: pt) }
          }
        }
        .stroke(color.opacity(0.5), style: StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
      }
      .frame(height: height)
    }
  }
}

extension Array {
  subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}
