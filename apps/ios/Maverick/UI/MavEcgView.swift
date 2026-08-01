import SwiftUI

struct MavEcgView: View {
  @EnvironmentObject private var connectors: ConnectorManager
  @State private var latestWaveform: [Float]?

  var body: some View {
    MavDetailScaffold(title: "ECG") {
      if let capture = connectors.ecgCapture,
        !["result", "failed", "cancelled"].contains(capture.phase)
      {
        captureCard(capture)
      } else if connectors.ecgCapabilities.contains(where: { $0.stream == "ecg" }) {
        MavTile {
          VStack(alignment: .leading, spacing: 14) {
            // A capture that ended without a result says why, here, before the retry. Dropping
            // straight back to the start card made a strap that never streamed and a wearer who
            // never made contact look identical: a button that appeared to do nothing.
            let ended = connectors.ecgCapture.flatMap {
              ["failed", "cancelled"].contains($0.phase) ? $0 : nil
            }
            Text(ended.map(Self.endedTitle) ?? "A clean 30-second recording")
              .mavType(.title)
              .foregroundStyle(MavTheme.ink)
            Text(
              ended.map(Self.endedDetail)
                ?? ("Hold still while Maverick checks signal contact. Recording starts only "
                  + "after quality stays good, then stops at exactly 30 seconds.")
            )
            .mavType(.body)
            .foregroundStyle(MavTheme.inkSecondary)
            .fixedSize(horizontal: false, vertical: true)
            MavPrimaryButton(
              title: ended == nil ? "Start ECG" : "Try again",
              detail: "Rest a finger on the metal electrode and keep still",
              systemImage: "waveform.path.ecg",
              action: connectors.startEcgCapture
            )
          }
        }
      } else {
        MavUnavailableCard(
          name: "ECG capture",
          reason: "Connect an ECG-capable device. Capture appears only after the hardware "
            + "positively declares that capability."
        )
      }

      if let error = connectors.ecgError {
        MavTile {
          Text(error)
            .mavType(.body)
            .foregroundStyle(MavTheme.destructiveInk())
            .fixedSize(horizontal: false, vertical: true)
        }
      }

      if let latest = connectors.ecgResults.first {
        NavigationLink {
          MavEcgResultView(result: latest)
        } label: {
          MavTile {
            VStack(alignment: .leading, spacing: 12) {
              HStack {
                VStack(alignment: .leading, spacing: 4) {
                  Text(rhythmTitle(latest.rhythm))
                    .mavType(.title)
                    .foregroundStyle(MavTheme.ink)
                  Text(relativeTime(latest.startedNs))
                    .mavType(.sub)
                    .foregroundStyle(MavTheme.inkSecondary)
                }
                Spacer()
                Image(systemName: "chevron.right")
                  .font(.system(size: 12, weight: .semibold))
                  .foregroundStyle(MavTheme.inkSecondary)
              }
              if let waveform = latestWaveform {
                MavEcgTrace(
                  waveform: waveform, sampleRateHz: Int(latest.sourceRateHz), height: 96
                )
              }
              MavEcgChecklist(checks: latest.checks, compact: true)
            }
          }
        }
        .buttonStyle(.plain)
        .task(id: latest.captureId) {
          latestWaveform = try? await connectors.ecgReportPayload(
            captureID: latest.captureId
          ).waveform
        }
      }

      if connectors.ecgResults.count > 1 {
        MavSectionHeader(title: "Earlier readings")
        VStack(spacing: 0) {
          ForEach(Array(connectors.ecgResults.dropFirst().enumerated()), id: \.element.captureId) {
            index, result in
            if index > 0 { MavDivider() }
            NavigationLink {
              MavEcgResultView(result: result)
            } label: {
              MavRow(
                title: rhythmTitle(result.rhythm),
                detail: resultDate(result.startedNs)
              ) {
                Image(systemName: "chevron.right")
                  .font(.system(size: 12, weight: .semibold))
                  .foregroundStyle(MavTheme.inkSecondary)
              }
            }
            .buttonStyle(.plain)
          }
        }
        .mavSurface(MavTheme.tileShape)
      }
    }
    .task { connectors.refreshEcgHistory() }
  }

  private func captureCard(_ capture: EcgCaptureReport) -> some View {
    MavTile {
      VStack(alignment: .leading, spacing: 15) {
        HStack {
          VStack(alignment: .leading, spacing: 4) {
            Text(captureTitle(capture.phase))
              .mavType(.title)
              .foregroundStyle(MavTheme.ink)
            Text(captureDetail(capture))
              .mavType(.body)
              .foregroundStyle(MavTheme.inkSecondary)
          }
          Spacer()
          ProgressView(value: Double(capture.progressMilli), total: 1_000)
            .progressViewStyle(.circular)
            .tint(MavTheme.accent)
        }
        if capture.phase != "analysing" {
          ProgressView(value: Double(capture.progressMilli), total: 1_000)
            .tint(MavTheme.accent)
            .accessibilityLabel("ECG capture progress")
            .accessibilityValue("\(capture.progressMilli / 10) percent")
          MavQuietButton(title: "Cancel", action: connectors.stopEcgCapture)
        }
      }
    }
  }

  private func captureTitle(_ phase: String) -> String {
    switch phase {
    case "calibrating": "Checking signal"
    case "recording": "Recording ECG"
    case "analysing": "Analysing on device"
    default: "ECG"
    }
  }

  /// What ended a capture, in the wearer's terms. The reason vocabulary is the core's.
  static func endedTitle(_ capture: EcgCaptureReport) -> String {
    if capture.phase == "cancelled" { return "Recording cancelled" }
    return capture.qualityReason == "no_signal"
      ? "No ECG signal arrived" : "Recording did not start"
  }

  static func endedDetail(_ capture: EcgCaptureReport) -> String {
    switch capture.qualityReason {
    case "no_signal":
      return "The strap accepted the request but sent no ECG. It stops the stream when it "
        + "decides it is off-wrist — check the fit and that it is still connected."
    case "calibration_timeout":
      return "Signal never stayed clean long enough to start. Rest a finger on the metal "
        + "electrode, keep your arm supported, and stay still."
    case "cancelled":
      return "You stopped this recording before it finished."
    default:
      return qualityAdvice(capture.qualityReason)
    }
  }

  /// The core's quality vocabulary, said out loud. Unknown reasons are named, never swallowed.
  static func qualityAdvice(_ reason: String?) -> String {
    switch reason {
    case .none: return "Signal looks good. Keep still."
    case "contact": return "Rest a finger on the metal electrode so the lead closes."
    case "motion": return "Too much movement. Support your arm and keep still."
    case "saturation": return "The signal is hitting its limits. Loosen contact slightly."
    case "flatline": return "No variation in the signal. Check the electrode contact."
    case "dropout": return "Samples are going missing. Keep the strap close to the phone."
    case .some(let reason): return "Signal is not usable yet (\(reason))."
    }
  }

  private func captureDetail(_ capture: EcgCaptureReport) -> String {
    switch capture.phase {
    case "calibrating":
      return Self.qualityAdvice(capture.qualityReason)
    case "recording":
      let seconds = capture.targetSamples > 0
        ? Int(capture.recordedSamples) * 30 / Int(capture.targetSamples) : 0
      return "\(seconds) of 30 seconds"
    case "analysing": return "The admitted model is running locally. No recording leaves the phone."
    default: return ""
    }
  }
}

/// The reading itself: trace first, then the verdict, the rate, and the report.
struct MavEcgResultView: View {
  let result: EcgResultReport
  @EnvironmentObject private var connectors: ConnectorManager
  @Environment(\.dismiss) private var dismiss
  @State private var waveform: [Float]?
  @State private var pdfURL: URL?
  @State private var pdfError: String?

  var body: some View {
    MavDetailScaffold(title: "ECG details") {
      Text(recordedRange(result.startedNs, result.endedNs))
        .mavType(.sub)
        .foregroundStyle(MavTheme.inkSecondary)

      if let waveform {
        MavEcgTrace(waveform: waveform, sampleRateHz: Int(result.sourceRateHz), height: 190)
        Text("Scroll to read the whole 30 seconds. One large square is 0.2 seconds.")
          .mavType(.sub)
          .foregroundStyle(MavTheme.inkSecondary)
      }

      Text(rhythmTitle(result.rhythm))
        .mavType(.numeralMedium)
        .foregroundStyle(MavTheme.ink)
      MavEcgChecklist(checks: result.checks, compact: false)

      MavTile {
        Text(rhythmExplanation(result.rhythm))
          .mavType(.body)
          .foregroundStyle(MavTheme.ink)
          .fixedSize(horizontal: false, vertical: true)
      }

      if let bpm = result.meanHeartRateBpm {
        MavTile {
          VStack(alignment: .leading, spacing: 4) {
            Text("\(bpm)")
              .mavType(.numeralMedium)
              .foregroundStyle(MavTheme.ink)
            Text("AVG. HEART RATE")
              .mavType(.label)
              .foregroundStyle(MavTheme.inkSecondary)
          }
        }
      }

      if let pdfURL {
        ShareLink(item: pdfURL) {
          Label("Share your ECG report", systemImage: "square.and.arrow.up")
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.glassProminent)
        .tint(MavTheme.accent)
      } else if pdfError == nil {
        ProgressView("Preparing report")
          .frame(maxWidth: .infinity)
      }

      MavTile {
        VStack(alignment: .leading, spacing: 10) {
          Text(
            "Taking regular readings makes a change easier to notice. A doctor can use an ECG "
              + "to look at your rhythm properly."
          )
          .mavType(.body)
          .foregroundStyle(MavTheme.inkSecondary)
          .fixedSize(horizontal: false, vertical: true)
          Text(
            "This on-device result is provisional and is not a diagnosis. If you think you are "
              + "having a medical emergency, call emergency services."
          )
          .mavType(.body)
          .foregroundStyle(MavTheme.inkSecondary)
          .fixedSize(horizontal: false, vertical: true)
        }
      }

      MavQuietButton(title: "Remove this ECG result") {
        connectors.removeEcgResult(captureID: result.captureId)
        dismiss()
      }

      if let pdfError {
        Text(pdfError)
          .mavType(.sub)
          .foregroundStyle(MavTheme.destructiveInk())
      }
    }
    .task(id: result.captureId) { await prepare() }
  }

  private func prepare() async {
    do {
      let payload = try await connectors.ecgReportPayload(captureID: result.captureId)
      waveform = payload.waveform
      let data = MavEcgPDFRenderer.render(MavEcgReportContent(payload: payload))
      let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("Maverick-ECG-\(result.captureId).pdf")
      try data.write(to: url, options: .atomic)
      pdfURL = url
    } catch {
      pdfError = error.localizedDescription
    }
  }
}

/// The four checks, exactly as the core derived them. A check the reading cannot support says so
/// rather than showing a tick it has not earned.
struct MavEcgChecklist: View {
  let checks: [EcgCheck]
  let compact: Bool

  /// The high- and low-rate checks say the same thing when there is no rate to judge, so a
  /// reading without one showed "Heart rate not measured" twice. Collapse repeats rather than
  /// dropping a check: the pair is still two findings, it just has one thing to say.
  private var visible: [EcgCheck] {
    checks.enumerated().filter { index, check in
      index == 0 || label(check) != label(checks[index - 1])
    }.map(\.element)
  }

  var body: some View {
    VStack(alignment: .leading, spacing: compact ? 6 : 10) {
      ForEach(visible, id: \.id) { check in
        HStack(spacing: 10) {
          Image(systemName: symbol(check))
            .font(.system(size: compact ? 14 : 17, weight: .semibold))
            .foregroundStyle(tint(check))
          Text(label(check))
            .mavType(compact ? .sub : .body)
            .foregroundStyle(MavTheme.ink)
            .fixedSize(horizontal: false, vertical: true)
          Spacer(minLength: 0)
        }
      }
    }
  }

  private func symbol(_ check: EcgCheck) -> String {
    if !check.known { return "questionmark.circle.fill" }
    return check.passed ? "checkmark.circle.fill" : "exclamationmark.triangle.fill"
  }

  private func tint(_ check: EcgCheck) -> Color {
    if !check.known { return MavTheme.inkSecondary }
    return check.passed ? MavTheme.accent : MavTheme.destructiveInk()
  }

  private func label(_ check: EcgCheck) -> String {
    switch check.id {
    case "afib":
      if !check.known { return "Atrial fibrillation not assessed" }
      return check.passed ? "AFib not detected" : "AFib detected"
    case "high_heart_rate":
      if !check.known { return "Heart rate not measured" }
      return check.passed ? "High heart rate not detected" : "High heart rate detected"
    case "low_heart_rate":
      if !check.known { return "Heart rate not measured" }
      return check.passed ? "Low heart rate not detected" : "Low heart rate detected"
    case "sinus_rhythm":
      return check.passed ? "Normal sinus rhythm detected" : "Normal sinus rhythm not detected"
    default: return check.id
    }
  }
}

private func rhythmExplanation(_ rhythm: String) -> String {
  switch rhythm {
  case "sinus_rhythm":
    "A normal sinus rhythm means the heart is beating in a uniform pattern, with the upper and "
      + "lower chambers in sync. It does not guarantee that you are well. If you feel unwell or "
      + "have symptoms, speak to a doctor."
  case "atrial_fibrillation":
    "This reading looks like atrial fibrillation, where the upper chambers beat irregularly. It "
      + "is a provisional software result on a single-lead recording, not a diagnosis. Take it "
      + "to a doctor, and seek urgent care if you feel unwell."
  default:
    "This reading did not match a normal sinus rhythm or atrial fibrillation. That covers a wide "
      + "range, including ordinary variation and recordings the model cannot place. Share the "
      + "report with a doctor if you are concerned."
  }
}

private func recordedRange(_ startNs: Int64, _ endNs: Int64) -> String {
  let start = Date(timeIntervalSince1970: TimeInterval(startNs) / 1_000_000_000)
  let end = Date(timeIntervalSince1970: TimeInterval(endNs) / 1_000_000_000)
  return start.formatted(date: .abbreviated, time: .standard) + " – "
    + end.formatted(date: .omitted, time: .standard)
}

private func relativeTime(_ nanoseconds: Int64) -> String {
  let date = Date(timeIntervalSince1970: TimeInterval(nanoseconds) / 1_000_000_000)
  return date.formatted(.relative(presentation: .named))
}

func rhythmTitle(_ rhythm: String) -> String {
  switch rhythm {
  case "sinus_rhythm": "Normal sinus rhythm"
  case "atrial_fibrillation": "Atrial fibrillation"
  default: "Other rhythm"
  }
}

private func resultDate(_ nanoseconds: Int64) -> String {
  let date = Date(timeIntervalSince1970: TimeInterval(nanoseconds) / 1_000_000_000)
  return date.formatted(date: .abbreviated, time: .shortened)
}

private func explanationText(_ result: EcgResultReport) -> String {
  guard let segment = result.explanation.max(by: {
    $0.importanceMilli < $1.importanceMilli
  }) else {
    return "No single five-second interval changed the winning score more than the others."
  }
  return "The \(segment.startSecond)–\(segment.endSecond) second interval influenced this "
    + "result most."
}
