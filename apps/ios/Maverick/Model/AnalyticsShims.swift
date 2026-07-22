import Foundation

// Presentation value types the Aura sleep views draw with. Not analytics: they carry no
// calculation, and the numbers that fill them come from the core.
//
// The illness and cycle result twins that used to live here are gone. Those are analytics, they are
// declared in `mav-analytic::capability`, and the core reports whether it can serve them — the views
// render that reason rather than a shape the app filled in for itself (ADR-024).

/// A contiguous sleep-stage segment (wall-clock unix seconds; stage ∈ wake|light|deep|rem).
struct StageSegment: Equatable, Sendable, Codable {
  var start: Int
  var end: Int
  var stage: String
}
