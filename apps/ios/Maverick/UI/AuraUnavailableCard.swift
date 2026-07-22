import SwiftUI

// MARK: - Unavailable analytics
//
// A card backed by an analytic the core cannot serve renders the core's reason, never a locally
// computed substitute and never silence. One view so the wording is written once and every screen
// says the same thing — the Swift twin of Android's AuraUnavailableCard.

/// Turn a core availability entry into a sentence a person can act on.
func auraUnavailableReason(_ entry: AnalyticAvailabilityReport?) -> String {
  guard let entry else { return "This metric is not part of the current core build." }
  if entry.available { return "" }
  switch entry.reason {
  case "algorithm_not_admitted":
    return "Not published yet — the calculation has no reference we can stand behind."
  case "missing_streams" where !entry.missingStreams.isEmpty:
    return "Waiting on \(auraStreamNames(entry.missingStreams)) from your strap."
  default:
    return "No data yet."
  }
}

private func auraStreamNames(_ streams: [String]) -> String {
  let friendly = streams.map { stream -> String in
    switch stream {
    case "rrinterval": "beat intervals"
    case "heartrate": "heart rate"
    case "skintemp": "skin temperature"
    case "respraw": "respiration"
    case "sleepstateraw": "sleep state"
    default: stream.replacingOccurrences(of: "_", with: " ")
    }
  }
  switch friendly.count {
  case 0: return "more data"
  case 1: return friendly[0]
  default: return friendly.dropLast().joined(separator: ", ") + " and " + (friendly.last ?? "")
  }
}

/// The honest empty state for one metric: its name, and why the core has nothing to show.
struct AuraUnavailableCard: View {
  let title: String
  let entry: AnalyticAvailabilityReport?

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      Text(title.uppercased())
        .font(AuraDesign.caption)
        .foregroundStyle(AuraDesign.ink.opacity(0.55))
      Text(auraUnavailableReason(entry))
        .font(AuraDesign.sub)
        .foregroundStyle(AuraDesign.ink.opacity(0.75))
        .fixedSize(horizontal: false, vertical: true)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(AuraDesign.tilePadding)
    .background(AuraDesign.card, in: AuraDesign.tileShape)
    .overlay(AuraDesign.tileShape.stroke(AuraDesign.hairline, lineWidth: 1))
    .accessibilityElement(children: .combine)
    .accessibilityLabel("\(title) unavailable. \(auraUnavailableReason(entry))")
  }
}
