import Foundation

struct MavSnapshot: Equatable, Sendable {
  let coreVersion: String
  let storageSchema: Int
  let revision: UInt64
  let connectionState: String
  let deviceName: String?
  let currentBpm: Int?
  let recoveryUnavailableReason: String?
  let hash: String
}

enum MavSnapshotDecoder {
  static func decode(json: String, hash: String) throws -> MavSnapshot {
    let root = try JSONDecoder().decode(HostSnapshot.self, from: Data(json.utf8))
    guard root.schema == "host-snapshot/v1" else { throw MavPresentationError.unsupportedSchema }
    let recovery = root.analytics?.availability.first(where: { $0.analytic == "recovery" && !$0.available })
    return MavSnapshot(
      coreVersion: root.coreVersion,
      storageSchema: root.storageSchema,
      revision: root.revision,
      connectionState: root.connection.state,
      deviceName: root.connection.displayName,
      currentBpm: root.session?.currentBpm,
      recoveryUnavailableReason: recovery?.reason?.message,
      hash: hash
    )
  }
}

enum MavPresentationError: LocalizedError {
  case unsupportedSchema
  var errorDescription: String? { "This version of Mav cannot read this core snapshot." }
}

private struct HostSnapshot: Decodable {
  let schema: String
  let coreVersion: String
  let storageSchema: Int
  let revision: UInt64
  let connection: Connection
  let session: Session?
  let analytics: Analytics?

  enum CodingKeys: String, CodingKey { case schema; case coreVersion = "core_version"; case storageSchema = "storage_schema"; case revision; case connection; case session; case analytics }
  struct Connection: Decodable { let state: String; let displayName: String?; enum CodingKeys: String, CodingKey { case state; case displayName = "display_name" } }
  struct Session: Decodable { let currentBpm: Int?; enum CodingKeys: String, CodingKey { case currentBpm = "current_bpm" } }
  struct Analytics: Decodable { let availability: [Availability] }
  struct Availability: Decodable { let analytic: String; let available: Bool; let reason: Reason? }
  struct Reason: Decodable {
    let kind: String
    let streams: [String]?
    var message: String {
      switch kind {
      case "algorithm_not_admitted": "Recovery model not admitted"
      case "missing_streams": "Needs \((streams ?? []).joined(separator: ", "))"
      default: "Unavailable"
      }
    }
  }
}
