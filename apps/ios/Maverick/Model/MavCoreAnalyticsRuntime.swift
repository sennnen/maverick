import Foundation

/// The generated core, in the shape `MavAnalyticsEngine` asks for.
///
/// Everything here is translation and nothing is a decision: the plan, the ordering, the reasons
/// and the cache all come from Rust. The one thing this file does is turn the core's strings into
/// the Swift types the reducer switches on, and it throws on an unrecognised one rather than
/// defaulting — a new unavailable reason should surface as a loud failure, not as a card that
/// silently renders as "working".
struct MavCoreAnalyticsRuntime: MavAnalyticsRuntime {
  let runtime: MavRuntime

  func host() -> MavModelBridge.Host { runtime }

  func admitPPGStages(deviceID: UInt64, atMs: Int64) throws {
    _ = try runtime.admitPpgStages(deviceId: deviceID, atMs: atMs)
  }

  func plan(
    deviceID: UInt64,
    atMs: Int64,
    mode: MavAnalyticsEngine.RunMode,
    profileFields: [String]
  ) throws -> [MavPlannedStage] {
    try runtime.analyticsPlan(
      deviceId: deviceID,
      atMs: atMs,
      mode: mode.rawValue,
      profileFields: profileFields
    ).stages.map(Self.stage(from:))
  }

  func profileFields() -> [String] { (try? runtime.wearerProfileFields()) ?? [] }

  func cacheCompletedAt() throws -> [String: Int64] {
    var out: [String: Int64] = [:]
    for entry in try runtime.analyticsCache() { out[entry.modelSlug] = entry.completedAtMs }
    return out
  }

  static func stage(from report: ModelStageReport) throws -> MavPlannedStage {
    guard let state = MavStageState(rawValue: report.state) else {
      throw MavModelError.unknown("the core reported an unknown stage state \(report.state)")
    }
    return MavPlannedStage(
      model: report.modelSlug,
      signal: report.signal,
      state: state,
      displayable: report.displayable,
      unavailable: try unavailable(from: report)
    )
  }

  private static func unavailable(from report: ModelStageReport) throws -> MavUnavailable? {
    switch report.unavailableReason {
    case .none: return nil
    case "missing_streams": return .missingStreams(report.missingStreams)
    case "missing_profile": return .missingProfile(report.missingProfile)
    case "upstream_unavailable": return .upstreamUnavailable(report.blockingModel ?? "")
    case "preprocessing_not_ported":
      return .preprocessingNotPorted(report.missingPreprocessing ?? "")
    case .some(let other):
      throw MavModelError.unknown("the core reported an unknown unavailable reason \(other)")
    }
  }
}

/// A core response this build does not understand.
enum MavModelError: Error {
  case unknown(String)
}
