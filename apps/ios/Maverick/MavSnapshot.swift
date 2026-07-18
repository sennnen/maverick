import Foundation

struct MavSnapshot: Equatable, Sendable {
  let coreVersion: String
  let storageSchema: Int
  let revision: UInt64
  let asOfUnixMs: Int64
  let connectionState: String
  let deviceName: String?
  let batteryPercent: Int?
  let charging: Bool?
  let onWrist: Bool?
  let lastSampleUnixMs: Int64?
  let currentBpm: Int?
  let meanMilliBpm: Int?
  let inRangeSamples: Int?
  let excludedSamples: Int?
  let prv: MavPrv?
  let prvUnavailableReason: String?
  let recoveryUnavailableReason: String?
  let hash: String
}

/// The admitted time-domain variability read model. WHOOP intervals are PPG-derived, so the core
/// labels the result `pulse_rate_variability`; it is presented as PRV, never as ECG HRV
/// (docs/analytics.md).
struct MavPrv: Equatable, Sendable {
  let label: String
  let intervalSource: String
  let meanIntervalMicros: Int64
  let rmssdMicros: Int64
  let sdnnMicros: Int64
  let nn50Count: Int
  let pnn50MilliPercent: Int64
  let intervalCount: Int
  let excludedIntervalCount: Int
  let algorithm: String
  let algorithmVersion: String
  let provenanceId: Int64
}

enum MavSnapshotDecoder {
  static func decode(json: String, hash: String) throws -> MavSnapshot {
    let root = try JSONDecoder().decode(HostSnapshot.self, from: Data(json.utf8))
    guard root.schema == "host-snapshot/v1" else { throw MavPresentationError.unsupportedSchema }
    return MavSnapshot(
      coreVersion: root.coreVersion,
      storageSchema: root.storageSchema,
      revision: root.revision,
      asOfUnixMs: root.asOfUnixMs,
      connectionState: root.connection.state,
      deviceName: root.connection.displayName,
      batteryPercent: root.connection.batteryPercent,
      charging: root.connection.charging,
      onWrist: root.connection.onWrist,
      lastSampleUnixMs: root.connection.lastSampleUnixMs,
      currentBpm: root.session?.currentBpm,
      meanMilliBpm: root.session?.meanMilliBpm,
      inRangeSamples: root.session?.inRangeSamples,
      excludedSamples: root.session?.excludedSamples,
      prv: try root.analytics.flatMap(prv(from:)),
      prvUnavailableReason: root.analytics?.unavailableReason(analytic: "time_domain_hrv", name: "PRV"),
      recoveryUnavailableReason: root.analytics?.unavailableReason(analytic: "recovery", name: "Recovery"),
      hash: hash
    )
  }

  private static func prv(from analytics: HostSnapshot.Analytics) throws -> MavPrv? {
    guard let label = analytics.variabilityLabel else { return nil }
    guard let intervalSource = analytics.intervalSource,
          let meanIntervalMicros = analytics.meanIntervalMicros,
          let rmssdMicros = analytics.rmssdMicros,
          let sdnnMicros = analytics.sdnnMicros,
          let nn50Count = analytics.nn50Count,
          let pnn50MilliPercent = analytics.pnn50MilliPercent,
          let intervalCount = analytics.intervalCount,
          let excludedIntervalCount = analytics.excludedIntervalCount,
          let algorithm = analytics.algorithm,
          let algorithmVersion = analytics.algorithmVersion,
          let provenanceId = analytics.provenanceId
    else { throw MavPresentationError.incompleteAnalytics }
    return MavPrv(
      label: label,
      intervalSource: intervalSource,
      meanIntervalMicros: meanIntervalMicros,
      rmssdMicros: rmssdMicros,
      sdnnMicros: sdnnMicros,
      nn50Count: nn50Count,
      pnn50MilliPercent: pnn50MilliPercent,
      intervalCount: intervalCount,
      excludedIntervalCount: excludedIntervalCount,
      algorithm: algorithm,
      algorithmVersion: algorithmVersion,
      provenanceId: provenanceId
    )
  }
}

enum MavPresentationError: LocalizedError {
  case unsupportedSchema
  case incompleteAnalytics
  var errorDescription: String? {
    switch self {
    case .unsupportedSchema: "This version of Mav cannot read this core snapshot."
    case .incompleteAnalytics: "The core analytics snapshot is missing required fields."
    }
  }
}

private struct HostSnapshot: Decodable {
  let schema: String
  let coreVersion: String
  let storageSchema: Int
  let revision: UInt64
  let asOfUnixMs: Int64
  let connection: Connection
  let session: Session?
  let analytics: Analytics?

  enum CodingKeys: String, CodingKey {
    case schema
    case coreVersion = "core_version"
    case storageSchema = "storage_schema"
    case revision
    case asOfUnixMs = "as_of_unix_ms"
    case connection
    case session
    case analytics
  }

  struct Connection: Decodable {
    let state: String
    let displayName: String?
    let batteryPercent: Int?
    let charging: Bool?
    let onWrist: Bool?
    let lastSampleUnixMs: Int64?

    enum CodingKeys: String, CodingKey {
      case state
      case displayName = "display_name"
      case batteryPercent = "battery_percent"
      case charging
      case onWrist = "on_wrist"
      case lastSampleUnixMs = "last_sample_unix_ms"
    }
  }

  struct Session: Decodable {
    let currentBpm: Int?
    let meanMilliBpm: Int?
    let inRangeSamples: Int
    let excludedSamples: Int

    enum CodingKeys: String, CodingKey {
      case currentBpm = "current_bpm"
      case meanMilliBpm = "mean_milli_bpm"
      case inRangeSamples = "in_range_samples"
      case excludedSamples = "excluded_samples"
    }
  }

  struct Analytics: Decodable {
    let intervalSource: String?
    let variabilityLabel: String?
    let meanIntervalMicros: Int64?
    let rmssdMicros: Int64?
    let sdnnMicros: Int64?
    let nn50Count: Int?
    let pnn50MilliPercent: Int64?
    let intervalCount: Int?
    let excludedIntervalCount: Int?
    let algorithm: String?
    let algorithmVersion: String?
    let provenanceId: Int64?
    let availability: [Availability]?

    enum CodingKeys: String, CodingKey {
      case intervalSource = "interval_source"
      case variabilityLabel = "variability_label"
      case meanIntervalMicros = "mean_interval_micros"
      case rmssdMicros = "rmssd_micros"
      case sdnnMicros = "sdnn_micros"
      case nn50Count = "nn50_count"
      case pnn50MilliPercent = "pnn50_milli_percent"
      case intervalCount = "interval_count"
      case excludedIntervalCount = "excluded_interval_count"
      case algorithm
      case algorithmVersion = "algorithm_version"
      case provenanceId = "provenance_id"
      case availability
    }

    func unavailableReason(analytic: String, name: String) -> String? {
      guard let entry = availability?.first(where: { $0.analytic == analytic && !$0.available })
      else { return nil }
      return entry.reason?.message(name: name) ?? "Unavailable"
    }
  }

  struct Availability: Decodable { let analytic: String; let available: Bool; let reason: Reason? }

  struct Reason: Decodable {
    let kind: String
    let streams: [String]?

    func message(name: String) -> String {
      switch kind {
      case "algorithm_not_admitted": "\(name) model not admitted"
      case "missing_streams": "Needs \((streams ?? []).joined(separator: ", "))"
      default: "Unavailable"
      }
    }
  }
}
