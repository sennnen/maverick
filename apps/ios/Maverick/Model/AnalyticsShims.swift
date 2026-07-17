import Foundation

// Value-type twins of the StrandAnalytics results the Aura UI can render. In Maverick these are
// computed nightly on-device; in Mav the matching analytics live in the Rust core and publish
// through the host snapshot once admitted (docs/analytics.md). Only the shapes the copied
// views read are carried — no scoring runs in the app.

/// A contiguous sleep-stage segment (wall-clock unix seconds; stage ∈ wake|light|deep|rem).
struct StageSegment: Equatable, Sendable, Codable {
  var start: Int
  var end: Int
  var stage: String
}

/// Overnight illness-ward heads-up (multi-vital anomaly screen; never a diagnosis).
enum IllnessSignalEngine {
  enum Level: String, Equatable, Sendable, Codable {
    case quiet
    case mild
    case raised
    case suppressed
    case alreadyUnwell
  }

  struct Result: Equatable, Sendable {
    let score: Double
    let level: Level
    let firedSignals: [String]
    let suppressedBy: [String]
    let signalCount: Int
    let copy: String
  }

  static let disclaimerTail = "On-device estimate - not a diagnosis."
}

/// Cycle-awareness phase estimate (temperature-shift based; awareness only, never contraception).
enum CyclePhaseEngine {
  enum Phase: String, Equatable, Sendable, Codable {
    case follicular
    case periOvulatory
    case luteal
    case unknown
    case learning
  }

  enum Confidence: String, Equatable, Sendable, Codable {
    case learning
    case building
    case solid
  }

  struct ShiftMarker: Equatable, Sendable {
    let day: String
  }

  struct NextPeriodWindow: Equatable, Sendable {
    let earliestDay: String
    let latestDay: String
  }

  struct Result: Equatable, Sendable {
    let phase: Phase
    let confidence: Confidence
    let cycleDayLow: Int?
    let cycleDayHigh: Int?
    let cycleLengthDays: Int?
    let nextPeriodWindow: NextPeriodWindow?
    let shiftMarkers: [ShiftMarker]
    let note: String
  }
}
