import Foundation
import UIKit

struct MavEcgReportContent {
  struct Segment {
    let startSecond: Int
    let endSecond: Int
    let importance: Float
  }

  let captureID: UInt64
  let recordedAt: Date
  let rhythm: String
  let probabilities: [Float]
  let confidence: Float
  let quality: Float
  let sampleRateHz: Int
  let sampleCount: Int
  let sourceUnit: String
  let waveform: [Float]
  let explanation: [Segment]
  let modelSHA256: String
  let preprocessingSHA256: String
  let algorithmVersion: String
  let provisional: Bool

  init(payload: EcgReportPayload) {
    let result = payload.result
    captureID = result.captureId
    recordedAt = Date(timeIntervalSince1970: TimeInterval(result.startedNs) / 1_000_000_000)
    rhythm = result.rhythm
    probabilities = [
      result.sinusProbability,
      result.atrialFibrillationProbability,
      result.otherAbnormalProbability,
    ]
    confidence = Float(result.confidenceMilli) / 1_000
    quality = Float(result.qualityMilli) / 1_000
    sampleRateHz = Int(result.sourceRateHz)
    sampleCount = Int(result.sampleCount)
    sourceUnit = payload.sourceUnit
    waveform = payload.waveform
    explanation = result.explanation.map {
      Segment(
        startSecond: Int($0.startSecond),
        endSecond: Int($0.endSecond),
        importance: Float($0.importanceMilli) / 1_000
      )
    }
    modelSHA256 = result.modelSha256
    preprocessingSHA256 = result.preprocessingSha256
    algorithmVersion = result.algorithmVersion
    provisional = result.provisional
  }

  init(
    captureID: UInt64,
    recordedAt: Date,
    rhythm: String,
    probabilities: [Float],
    confidence: Float,
    quality: Float,
    sampleRateHz: Int,
    sampleCount: Int,
    sourceUnit: String,
    waveform: [Float],
    explanation: [Segment],
    modelSHA256: String,
    preprocessingSHA256: String,
    algorithmVersion: String,
    provisional: Bool
  ) {
    self.captureID = captureID
    self.recordedAt = recordedAt
    self.rhythm = rhythm
    self.probabilities = probabilities
    self.confidence = confidence
    self.quality = quality
    self.sampleRateHz = sampleRateHz
    self.sampleCount = sampleCount
    self.sourceUnit = sourceUnit
    self.waveform = waveform
    self.explanation = explanation
    self.modelSHA256 = modelSHA256
    self.preprocessingSHA256 = preprocessingSHA256
    self.algorithmVersion = algorithmVersion
    self.provisional = provisional
  }
}

enum MavEcgPDFRenderer {
  private struct TraceScale {
    let center: Float
    let pointsPerUnit: CGFloat
    let caption: String
    /// True when `pointsPerUnit` is a real 10 mm/mV gain rather than a fitted relative one.
    let calibrated: Bool
  }

  private static let page = CGRect(x: 0, y: 0, width: 595, height: 842)
  private static let paper = UIColor(red: 0.965, green: 0.957, blue: 0.925, alpha: 1)
  private static let ink = UIColor(red: 0.075, green: 0.105, blue: 0.100, alpha: 1)
  private static let secondary = UIColor(red: 0.28, green: 0.34, blue: 0.32, alpha: 1)
  private static let teal = UIColor(red: 0.18, green: 0.36, blue: 0.34, alpha: 1)
  private static let paleTeal = UIColor(red: 0.82, green: 0.88, blue: 0.84, alpha: 1)
  private static let rule = UIColor(red: 0.76, green: 0.76, blue: 0.69, alpha: 1)
  /// Conventional ECG paper red. The trace grid uses this rather than the document rule colour.
  private static let gridInk = UIColor(red: 0.78, green: 0.42, blue: 0.42, alpha: 1)
  private static let pointsPerMillimetre: CGFloat = 72 / 25.4
  private static let traceWidth = 125 * pointsPerMillimetre
  private static let traceHeight = 22 * pointsPerMillimetre

  static func render(_ report: MavEcgReportContent) -> Data {
    let format = UIGraphicsPDFRendererFormat()
    format.documentInfo = [
      kCGPDFContextTitle as String: "Maverick ECG report",
      kCGPDFContextAuthor as String: "Maverick",
      kCGPDFContextCreator as String: "Maverick native iOS report renderer",
    ]
    return UIGraphicsPDFRenderer(bounds: page, format: format).pdfData { renderer in
      renderer.beginPage()
      drawReport(report, in: renderer.cgContext)
    }
  }

  private static func drawReport(_ report: MavEcgReportContent, in context: CGContext) {
    context.setFillColor(paper.cgColor)
    context.fill(page)

    drawText(
      "MAVERICK", at: CGPoint(x: 42, y: 34), font: .systemFont(ofSize: 12, weight: .bold),
      color: ink, tracking: 3.4
    )
    drawText(
      "ECG REPORT", at: CGPoint(x: 42, y: 62), font: .systemFont(ofSize: 9, weight: .semibold),
      color: teal, tracking: 2
    )
    drawText(
      rhythmTitle(report.rhythm), at: CGPoint(x: 42, y: 80),
      font: .systemFont(ofSize: 31, weight: .light), color: ink
    )
    drawText(
      report.provisional ? "Provisional on-device interpretation" : "On-device interpretation",
      at: CGPoint(x: 44, y: 119), font: .systemFont(ofSize: 11.5, weight: .medium),
      color: secondary
    )

    let date = DateFormatter()
    date.dateStyle = .medium
    date.timeStyle = .short
    metric("RECORDED", date.string(from: report.recordedAt), x: 42, y: 153)
    metric("DURATION", "30 seconds", x: 226, y: 153)
    metric("QUALITY", percent(report.quality), x: 348, y: 153)
    metric("CONFIDENCE", percent(report.confidence), x: 454, y: 153)

    drawText(
      "MODEL VIEW", at: CGPoint(x: 42, y: 199), font: .systemFont(ofSize: 8.5, weight: .semibold),
      color: teal, tracking: 1.6
    )
    let labels = ["Sinus", "Atrial fibrillation", "Other"]
    for index in 0..<3 {
      let value = report.probabilities.indices.contains(index) ? report.probabilities[index] : 0
      let x = 42 + CGFloat(index) * 174
      probability(label: labels[index], value: value, x: x, y: 219, width: 158)
    }

    let scale = traceScale(for: report)
    drawText(
      "30-SECOND RHYTHM STRIP", at: CGPoint(x: 42, y: 258),
      font: .systemFont(ofSize: 8.5, weight: .semibold), color: teal, tracking: 1.6
    )
    drawText(
      scale.caption, at: CGPoint(x: 552, y: 258),
      font: .monospacedSystemFont(ofSize: 7.5, weight: .regular), color: secondary,
      alignment: .right
    )

    let graphX: CGFloat = 42
    let graphY: CGFloat = 274
    let stride: CGFloat = 68
    for strip in 0..<6 {
      let rect = CGRect(
        x: graphX, y: graphY + CGFloat(strip) * stride, width: traceWidth, height: traceHeight
      )
      drawGrid(in: rect, context: context)
      drawTrace(
        report.waveform, sampleRate: report.sampleRateHz, strip: strip, scale: scale,
        in: rect, context: context
      )
      if strip == 0 { drawCalibrationPulse(scale: scale, in: rect, context: context) }
      traceAnnotation(report, strip: strip, graph: rect)
    }

    drawText(
      "HOW TO READ", at: CGPoint(x: 42, y: 690),
      font: .systemFont(ofSize: 8.5, weight: .semibold), color: teal, tracking: 1.6
    )
    drawText(
      "Each row is five seconds at a true 25 mm/s time base. The same vertical gain is used "
        + "throughout. Model influence shows which masked interval most changed the winning score.",
      in: CGRect(x: 42, y: 704, width: 510, height: 34),
      font: .systemFont(ofSize: 8.8), color: secondary
    )
    drawText(
      "READ WITH CARE", at: CGPoint(x: 42, y: 745),
      font: .systemFont(ofSize: 8.5, weight: .semibold), color: teal, tracking: 1.6
    )
    drawText(
      "This research-only software result is not a diagnosis. Seek urgent care for chest pain, "
        + "fainting, severe breathlessness, or other concerning symptoms. A clinician should "
        + "interpret this single-lead recording in context.",
      in: CGRect(x: 42, y: 759, width: 510, height: 38),
      font: .systemFont(ofSize: 8.8), color: secondary
    )
    footer(report, context: context)
  }

  private static func metric(_ label: String, _ value: String, x: CGFloat, y: CGFloat) {
    drawText(
      label, at: CGPoint(x: x, y: y), font: .systemFont(ofSize: 7, weight: .semibold),
      color: secondary, tracking: 1
    )
    drawText(
      value, at: CGPoint(x: x, y: y + 14), font: .systemFont(ofSize: 10.5, weight: .semibold),
      color: ink
    )
  }

  private static func probability(
    label: String, value: Float, x: CGFloat, y: CGFloat, width: CGFloat
  ) {
    drawText(
      label, in: CGRect(x: x, y: y, width: width - 38, height: 14),
      font: .systemFont(ofSize: 9, weight: .medium), color: ink
    )
    drawText(
      percent(value), in: CGRect(x: x, y: y, width: width, height: 14),
      font: .monospacedSystemFont(ofSize: 8.5, weight: .semibold), color: ink,
      alignment: .right
    )
    let track = CGRect(x: x, y: y + 17, width: width, height: 4)
    paleTeal.setFill()
    UIBezierPath(roundedRect: track, cornerRadius: 2).fill()
    teal.setFill()
    UIBezierPath(
      roundedRect: CGRect(
        x: track.minX, y: track.minY, width: track.width * CGFloat(value.clamped),
        height: track.height
      ),
      cornerRadius: 2
    ).fill()
  }

  private static func traceAnnotation(
    _ report: MavEcgReportContent, strip: Int, graph: CGRect
  ) {
    let start = strip * 5
    let importance = report.explanation.indices.contains(strip)
      ? report.explanation[strip].importance.clamped : 0
    let x = graph.maxX + 18
    drawText(
      "\(start)-\(start + 5) s", at: CGPoint(x: x, y: graph.minY + 3),
      font: .monospacedSystemFont(ofSize: 8, weight: .semibold), color: ink
    )
    drawText(
      "MODEL INFLUENCE", at: CGPoint(x: x, y: graph.minY + 19),
      font: .systemFont(ofSize: 6.5, weight: .semibold), color: secondary, tracking: 0.7
    )
    let track = CGRect(x: x, y: graph.minY + 34, width: 119, height: 5)
    paleTeal.setFill()
    UIBezierPath(roundedRect: track, cornerRadius: 2.5).fill()
    teal.setFill()
    UIBezierPath(
      roundedRect: CGRect(
        x: track.minX, y: track.minY, width: track.width * CGFloat(importance),
        height: track.height
      ),
      cornerRadius: 2.5
    ).fill()
    drawText(
      percent(importance), at: CGPoint(x: 552, y: graph.minY + 42),
      font: .monospacedSystemFont(ofSize: 7, weight: .medium), color: secondary,
      alignment: .right
    )
  }

  private static func drawGrid(in rect: CGRect, context: CGContext) {
    context.saveGState()
    context.clip(to: rect)
    context.setLineWidth(0.22)
    context.setStrokeColor(gridInk.withAlphaComponent(0.47).cgColor)
    let horizontal = Int((rect.width / pointsPerMillimetre).rounded())
    let vertical = Int((rect.height / pointsPerMillimetre).rounded())
    for index in 0...horizontal {
      let x = rect.minX + CGFloat(index) * pointsPerMillimetre
      context.move(to: CGPoint(x: x, y: rect.minY))
      context.addLine(to: CGPoint(x: x, y: rect.maxY))
    }
    for index in 0...vertical {
      let y = rect.minY + CGFloat(index) * pointsPerMillimetre
      context.move(to: CGPoint(x: rect.minX, y: y))
      context.addLine(to: CGPoint(x: rect.maxX, y: y))
    }
    context.strokePath()
    context.setLineWidth(0.5)
    context.setStrokeColor(gridInk.withAlphaComponent(0.80).cgColor)
    for index in stride(from: 0, through: horizontal, by: 5) {
      let x = rect.minX + CGFloat(index) * pointsPerMillimetre
      context.move(to: CGPoint(x: x, y: rect.minY))
      context.addLine(to: CGPoint(x: x, y: rect.maxY))
    }
    for index in stride(from: 0, through: vertical, by: 5) {
      let y = rect.minY + CGFloat(index) * pointsPerMillimetre
      context.move(to: CGPoint(x: rect.minX, y: y))
      context.addLine(to: CGPoint(x: rect.maxX, y: y))
    }
    context.strokePath()
    context.restoreGState()
  }

  private static func drawTrace(
    _ waveform: [Float],
    sampleRate: Int,
    strip: Int,
    scale: TraceScale,
    in rect: CGRect,
    context: CGContext
  ) {
    guard sampleRate > 0, !waveform.isEmpty else { return }
    let start = strip * sampleRate * 5
    let end = min(waveform.count, start + sampleRate * 5)
    guard end - start > 1 else { return }
    let path = UIBezierPath()
    for offset in 0..<(end - start) {
      let sample = waveform[start + offset]
      guard sample.isFinite else { continue }
      let seconds = CGFloat(offset) / CGFloat(sampleRate)
      let x = rect.minX + seconds * 25 * pointsPerMillimetre
      let displacement = CGFloat(sample - scale.center) * scale.pointsPerUnit
      let y = rect.midY - displacement
      if path.isEmpty {
        path.move(to: CGPoint(x: x, y: y))
      } else {
        path.addLine(to: CGPoint(x: x, y: y))
      }
    }
    context.saveGState()
    context.clip(to: rect)
    teal.setStroke()
    path.lineWidth = 0.9
    path.lineJoinStyle = .round
    path.lineCapStyle = .round
    path.stroke()
    context.restoreGState()
  }

  /// The standard 1 mV calibration mark: a 5 mm step at the head of the recording, drawn in the
  /// same gain as the trace. Only honest where the gain means millivolts, so a source with no
  /// established scale gets no pulse rather than a decorative one.
  private static func drawCalibrationPulse(
    scale: TraceScale, in rect: CGRect, context: CGContext
  ) {
    guard scale.calibrated else { return }
    let millivolt = scale.pointsPerUnit
    let baseline = rect.midY
    let width = 5 * pointsPerMillimetre
    let path = UIBezierPath()
    path.move(to: CGPoint(x: rect.minX, y: baseline))
    path.addLine(to: CGPoint(x: rect.minX + width * 0.35, y: baseline))
    path.addLine(to: CGPoint(x: rect.minX + width * 0.35, y: baseline - millivolt))
    path.addLine(to: CGPoint(x: rect.minX + width, y: baseline - millivolt))
    path.addLine(to: CGPoint(x: rect.minX + width, y: baseline))
    path.addLine(to: CGPoint(x: rect.minX + width * 1.35, y: baseline))
    context.setStrokeColor(ink.cgColor)
    context.setLineWidth(0.9)
    context.setLineJoin(.miter)
    context.addPath(path.cgPath)
    context.strokePath()
  }

  private static func traceScale(for report: MavEcgReportContent) -> TraceScale {
    let finite = report.waveform.filter(\.isFinite).sorted()
    let center = finite.isEmpty ? 0 : finite[finite.count / 2]
    switch report.sourceUnit {
    case "millivolts":
      return TraceScale(
        center: center,
        pointsPerUnit: 10 * pointsPerMillimetre,
        caption: "25 mm/s / 10 mm/mV / \(report.sampleRateHz) Hz", calibrated: true
      )
    case "microvolts":
      return TraceScale(
        center: center,
        pointsPerUnit: 0.01 * pointsPerMillimetre,
        caption: "25 mm/s / 10 mm/mV / \(report.sampleRateHz) Hz", calibrated: true
      )
    case "volts":
      return TraceScale(
        center: center,
        pointsPerUnit: 10_000 * pointsPerMillimetre,
        caption: "25 mm/s / 10 mm/mV / \(report.sampleRateHz) Hz", calibrated: true
      )
    default:
      let deviations = finite.map { abs($0 - center) }.sorted()
      let percentile = deviations.isEmpty
        ? 1 : deviations[min(deviations.count - 1, Int(Float(deviations.count) * 0.98))]
      return TraceScale(
        center: center,
        pointsPerUnit: 7 * pointsPerMillimetre / CGFloat(max(percentile, 0.000_001)),
        caption: "25 mm/s / shared relative gain / \(report.sampleRateHz) Hz", calibrated: false
      )
    }
  }

  private static func footer(_ report: MavEcgReportContent, context: CGContext) {
    context.setStrokeColor(rule.cgColor)
    context.setLineWidth(0.5)
    context.move(to: CGPoint(x: 42, y: 788))
    context.addLine(to: CGPoint(x: 552, y: 788))
    context.strokePath()
    let fingerprint = String(report.modelSHA256.prefix(12))
    drawText(
      "Capture \(report.captureID) / Model \(fingerprint) / v\(report.algorithmVersion) / "
        + "\(report.sampleCount) samples",
      at: CGPoint(x: 42, y: 798), font: .monospacedSystemFont(ofSize: 7, weight: .regular),
      color: secondary
    )
    drawText(
      "1 / 1", at: CGPoint(x: 552, y: 798),
      font: .monospacedSystemFont(ofSize: 7, weight: .regular), color: secondary,
      alignment: .right
    )
  }

  private static func drawText(
    _ value: String,
    at point: CGPoint,
    font: UIFont,
    color: UIColor,
    tracking: CGFloat = 0,
    alignment: NSTextAlignment = .left
  ) {
    let originX = switch alignment {
    case .right: point.x - 510
    case .center: point.x - 255
    default: point.x
    }
    drawText(
      value, in: CGRect(x: originX, y: point.y, width: 510, height: font.lineHeight + 4),
      font: font, color: color, tracking: tracking, alignment: alignment
    )
  }

  private static func drawText(
    _ value: String,
    in rect: CGRect,
    font: UIFont,
    color: UIColor,
    tracking: CGFloat = 0,
    alignment: NSTextAlignment = .left
  ) {
    let paragraph = NSMutableParagraphStyle()
    paragraph.alignment = alignment
    paragraph.lineBreakMode = .byWordWrapping
    paragraph.lineSpacing = 1.5
    NSAttributedString(
      string: value,
      attributes: [
        .font: font,
        .foregroundColor: color,
        .kern: tracking,
        .paragraphStyle: paragraph,
      ]
    ).draw(in: rect)
  }

  private static func rhythmTitle(_ rhythm: String) -> String {
    switch rhythm {
    case "sinus_rhythm": "Sinus rhythm"
    case "atrial_fibrillation": "Atrial fibrillation"
    default: "Other rhythm"
    }
  }

  private static func percent(_ value: Float) -> String {
    "\(Int((value.clamped * 100).rounded()))%"
  }
}

private extension Float {
  var clamped: Float { min(max(self, 0), 1) }
}
