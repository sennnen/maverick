import SwiftUI

// Weekly / Monthly Performance Assessment — generated fully on-device from the
// existing day history, with a shareable PDF render. No cloud.

struct AuraReportsView: View {
  @EnvironmentObject private var repo: Repository
  @Environment(\.dismiss) private var dismiss

  enum Span: String, CaseIterable, Identifiable {
    case weekly = "Weekly", monthly = "Monthly"
    var id: String { rawValue }
    var days: Int { self == .weekly ? 7 : 30 }
  }

  @State private var span: Span = .weekly
  @State private var restSeries: [(day: String, value: Double)] = []
  @State private var workoutCount = 0
  @State private var topSport: String?
  @State private var pdfURL: URL?

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: AuraDesign.sectionGap) {
        picker
        reportBody
        exportButton
      }
      .padding(.horizontal, AuraDesign.screenMargin)
      .padding(.bottom, 48)
    }
    .scrollIndicators(.hidden)
    .auraScreen(.charge)
    .safeAreaInset(edge: .top) { bar }
    .task(id: repo.refreshSeq) { await load() }
    .onChange(of: span) { _, _ in pdfURL = nil; Task { await load() } }
  }

  private func load() async {
    restSeries = await repo.exploreSeries(key: "sleep_performance", source: "my-whoop")
    let cutoff = Int(Date().timeIntervalSince1970) - span.days * 86400
    let rows = await repo.workoutRows(days: span.days).filter { $0.startTs >= cutoff }
    workoutCount = rows.count
    let counts = Dictionary(grouping: rows, by: \.sport).mapValues(\.count)
    topSport = counts.max { $0.value < $1.value }?.key
  }

  private var bar: some View {
    HStack {
      Text("Report").font(AuraDesign.heading(20)).foregroundStyle(AuraDesign.ink)
      Spacer()
      Button { dismiss() } label: {
        Image(systemName: "xmark")
          .font(.system(size: 15, weight: .bold)).foregroundStyle(AuraDesign.ink)
          .frame(width: 40, height: 40)
          .background(.ultraThinMaterial, in: Circle())
          .contentShape(Circle())
      }
      .buttonStyle(.plain)
    }
    .padding(.horizontal, AuraDesign.screenMargin)
    .padding(.top, 10).padding(.bottom, 8)
  }

  private var picker: some View {
    HStack(spacing: 4) {
      ForEach(Span.allCases) { s in
        let active = s == span
        Button {
          withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) { span = s }
        } label: {
          Text(s.rawValue)
            .font(AuraDesign.caption)
            .foregroundStyle(active ? Color.black : AuraDesign.ink.opacity(0.65))
            .padding(.horizontal, 16).padding(.vertical, 7)
            .background(active ? AuraDesign.accent : .clear, in: Capsule())
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
      }
    }
    .padding(3)
    .background(AuraDesign.ink.opacity(0.08), in: Capsule())
  }

  // MARK: The report itself (also what the PDF renders)

  private var window: [DailyMetric] { Array(repo.days.suffix(span.days)) }

  private var reportBody: some View {
    AuraReportSheet(title: span == .weekly ? "Weekly Performance Assessment" : "Monthly Performance Assessment",
                    subtitle: rangeText,
                    days: window,
                    restPoints: Array(restSeries.suffix(span.days)),
                    workoutCount: workoutCount,
                    topSport: topSport)
  }

  private var rangeText: String {
    guard let first = window.first?.day, let last = window.last?.day else { return "" }
    return "\(pretty(first)) – \(pretty(last))"
  }
  private func pretty(_ day: String) -> String {
    let f = DateFormatter(); f.dateFormat = "yyyy-MM-dd"; f.locale = .init(identifier: "en_US_POSIX")
    guard let d = f.date(from: day) else { return day }
    return d.formatted(.dateTime.day().month(.abbreviated))
  }

  // MARK: PDF export

  @ViewBuilder private var exportButton: some View {
    if let url = pdfURL {
      ShareLink(item: url) {
        exportLabel("Share PDF", icon: "square.and.arrow.up")
      }
      .buttonStyle(AuraPressStyle())
    } else {
      Button { renderPDF() } label: {
        exportLabel("Export as PDF", icon: "doc.richtext")
      }
      .buttonStyle(AuraPressStyle())
    }
  }

  private func exportLabel(_ text: String, icon: String) -> some View {
    HStack(spacing: 10) {
      Image(systemName: icon).font(.system(size: 15, weight: .semibold))
      Text(text).font(AuraDesign.label)
    }
    .foregroundStyle(Color.black)
    .frame(maxWidth: .infinity)
    .padding(.vertical, 15)
    .background(AuraDesign.accent, in: Capsule())
    .contentShape(Capsule())
  }

  @MainActor private func renderPDF() {
    let content = AuraReportSheet(
      title: span == .weekly ? "Weekly Performance Assessment" : "Monthly Performance Assessment",
      subtitle: rangeText, days: window,
      restPoints: Array(restSeries.suffix(span.days)),
      workoutCount: workoutCount, topSport: topSport)
      .frame(width: 560)
      .padding(28)
      .background(Color.black)
      .environment(\.colorScheme, .dark)

    let renderer = ImageRenderer(content: content)
    renderer.proposedSize = .init(width: 616, height: nil)
    let url = FileManager.default.temporaryDirectory
      .appendingPathComponent("NOOP-\(span.rawValue)-Report.pdf")
    renderer.render { size, draw in
      var box = CGRect(origin: .zero, size: size)
      guard let ctx = CGContext(url as CFURL, mediaBox: &box, nil) else { return }
      ctx.beginPDFPage(nil)
      draw(ctx)
      ctx.endPDFPage()
      ctx.closePDF()
    }
    pdfURL = url
  }
}

// MARK: - Printable summary blocks

struct AuraReportSheet: View {
  let title: String
  let subtitle: String
  let days: [DailyMetric]
  let restPoints: [(day: String, value: Double)]
  let workoutCount: Int
  let topSport: String?

  private func avg(_ v: [Double]) -> Double? { v.isEmpty ? nil : v.reduce(0, +) / Double(v.count) }

  var body: some View {
    VStack(alignment: .leading, spacing: 20) {
      VStack(alignment: .leading, spacing: 4) {
        Text(title).font(AuraDesign.heading(22)).foregroundStyle(AuraDesign.ink)
        Text(subtitle).font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.6))
      }

      // Headline: the three pillars, status-coloured.
      HStack(spacing: 14) {
        pillar("Charge", avg(days.compactMap(\.recovery)), "%",
               status: .recovery(avg(days.compactMap(\.recovery))), family: .charge)
        pillar("Rest", avg(restPoints.map(\.value)), "%",
               status: .sleep(avg(restPoints.map(\.value))), family: .rest)
        effortPillar(avg(days.compactMap(\.strain)))
      }

      LazyVGrid(columns: [GridItem(.flexible(), spacing: 18), GridItem(.flexible(), spacing: 18)], spacing: 20) {
        AuraMiniStat(value: text(avg(days.compactMap(\.avgHrv))), unit: "ms", label: "Avg HRV",
                     level: (avg(days.compactMap(\.avgHrv)) ?? 0) / 140, tint: AuraDesign.Family.charge.glow)
        AuraMiniStat(value: text(avg(days.compactMap { $0.restingHr.map(Double.init) })), unit: "bpm", label: "Avg Resting HR",
                     level: 1 - (avg(days.compactMap { $0.restingHr.map(Double.init) }) ?? 60) / 100,
                     tint: AuraDesign.Family.heart.glow)
        AuraMiniStat(value: hm(avg(days.compactMap(\.totalSleepMin))), label: "Avg Sleep",
                     level: (avg(days.compactMap(\.totalSleepMin)) ?? 0) / 540, tint: AuraDesign.Family.rest.glow)
        AuraMiniStat(value: "\(workoutCount)", label: topSport.map { "Workouts · mostly \($0)" } ?? "Workouts",
                     level: Double(workoutCount) / Double(max(days.count, 1)), tint: AuraDesign.Family.effort.glow)
      }
      .padding(20)
      .frame(maxWidth: .infinity)
      .background(AuraDesign.card, in: AuraDesign.tileShape)
      .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))

      VStack(alignment: .leading, spacing: 14) {
        Text("Charge, day by day").font(AuraDesign.heading(15)).foregroundStyle(AuraDesign.ink)
        AuraGraph(points: days.compactMap { d in d.recovery.map { (d.day, $0) } },
                  tint: AuraDesign.Family.charge.glow, unit: "%", style: .bars, height: 90)
      }
      .auraDarkCard(padding: 18)

      Text("Generated on-device by NOOP · \(Date.now.formatted(date: .abbreviated, time: .shortened)) · Approximate, not medical advice")
        .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.4))
    }
  }

  private func effortPillar(_ stored: Double?) -> some View {
    VStack(alignment: .leading, spacing: 8) {
      Text("Effort").font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.65))
      Text(AuraEffort.text(stored))
        .font(AuraDesign.number(30))
        .foregroundStyle(AuraDesign.Family.effort.glow)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(16)
    .background(AuraDesign.card, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
    .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous)
      .strokeBorder(AuraDesign.hairline, lineWidth: 1))
  }

  private func pillar(_ label: String, _ value: Double?, _ unit: String,
                      status: AuraStatus, family: AuraDesign.Family) -> some View {
    VStack(alignment: .leading, spacing: 8) {
      Text(label).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.65))
      HStack(alignment: .firstTextBaseline, spacing: 2) {
        Text(value.map { String(Int($0.rounded())) } ?? "--")
          .font(AuraDesign.number(30))
          .foregroundStyle(status == .none ? family.glow : status.color)
        if !unit.isEmpty, value != nil {
          Text(unit).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
        }
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(16)
    .background(AuraDesign.card, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
    .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous)
      .strokeBorder(AuraDesign.hairline, lineWidth: 1))
  }

  private func text(_ v: Double?) -> String { v.map { String(Int($0.rounded())) } ?? "--" }
  private func hm(_ m: Double?) -> String {
    guard let m, m > 0 else { return "--" }
    let t = Int(m.rounded()); return "\(t / 60)h \(t % 60)m"
  }
}
