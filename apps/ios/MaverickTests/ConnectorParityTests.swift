import CryptoKit
import XCTest

final class ConnectorParityTests: XCTestCase {
  private struct Expected {
    let family: String
    let connectorID: String
    let fixtureCount: Int
    let artifactHash: String
    /// Fixtures this connector must carry by name. Different per family, because "the connector
    /// still covers the case it was written for" is the thing worth asserting and the cases are
    /// not the same — generic-hr has no history cursor to retry.
    let requiredFixtures: Set<String>
  }

  /// Frozen against the signed registry at the maverick-connectors commit `CONNECTORS_REF` names.
  ///
  /// The Kotlin twin in `ConnectorParityTest.kt` carries the same three constants. They move
  /// together with `CONNECTORS_REF` and `fixtures/connectors/README.md`, never on their own.
  private let expected = [
    Expected(
      family: "generic_hr",
      connectorID: "dev.maverick.generic-hr",
      fixtureCount: 3,
      artifactHash: "9ac7a6648d2a508998a05797d3c38acd8bb1d28d1322d6352fce989553862d98",
      requiredFixtures: ["chest-strap-reports-electrical-intervals"]
    ),
    Expected(
      family: "whoop4",
      connectorID: "dev.maverick.whoop4",
      fixtureCount: 18,
      artifactHash: "c7539ff1fdae3a0cdc07aef88bae1ae220345878391e7367973fa0502ecac551",
      requiredFixtures: ["history-cursor-retry", "state-restart", "malformed-frame"]
    ),
    Expected(
      family: "whoop5",
      connectorID: "dev.maverick.whoop5",
      fixtureCount: 16,
      artifactHash: "6137a0a2e1708f681a4f85d4109f186720ef74114fc2b7f08d3ee30fc19cd427",
      requiredFixtures: [
        "history-cursor-retry", "state-restart", "malformed-frame",
        "mg-ecg-capture", "non-mg-ecg-fails-closed",
      ]
    ),
  ]

  func testFrozenConnectorParityReportsMeetMobileBudgets() throws {
    for value in expected {
      let report = try parityReport(value.family)
      XCTAssertEqual(report["schema"] as? String, "mavconn-parity/v1")
      XCTAssertEqual(report["connector_id"] as? String, value.connectorID)
      XCTAssertEqual(report["artifact_sha256"] as? String, value.artifactHash, value.family)
      XCTAssertEqual(report["fixture_count"] as? Int, value.fixtureCount, value.family)

      let fixtures = try XCTUnwrap(report["fixtures"] as? [[String: Any]])
      let names = Set(fixtures.compactMap { $0["name"] as? String })
      XCTAssertTrue(
        names.isSuperset(of: value.requiredFixtures),
        "\(value.family) lost \(value.requiredFixtures.subtracting(names))"
      )
      for fixture in fixtures {
        XCTAssertLessThanOrEqual(try XCTUnwrap(fixture["max_fuel_consumed"] as? Int), 5_000_000)
        XCTAssertLessThanOrEqual(
          try XCTUnwrap(fixture["peak_memory_bytes"] as? Int), 4 * 1024 * 1024
        )
      }
    }
  }

  /// Every report names the SHA-256 of the artifact sitting beside it.
  ///
  /// The frozen hashes above cannot tell a legitimate connector release from a report that has
  /// drifted away from its own bytes — both look like one changed constant. Computing the hash
  /// separates the two failures, and it is the Kotlin twin's rule; iOS was asserting the frozen
  /// list alone, so a report and its artifact could disagree here and only Android would say so.
  func testEachReportNamesTheArtifactBesideIt() throws {
    for value in expected {
      let artifact = try XCTUnwrap(
        Bundle(for: Self.self).url(forResource: "\(value.family)_v1", withExtension: "mavconn"),
        "\(value.family) artifact is not in the test bundle"
      )
      let digest = SHA256.hash(data: try Data(contentsOf: artifact))
      let hex = digest.map { String(format: "%02x", $0) }.joined()
      let report = try parityReport(value.family)
      XCTAssertEqual(
        report["artifact_sha256"] as? String, hex,
        "\(value.family)'s parity report and its artifact disagree"
      )
    }
  }

  private func parityReport(_ family: String) throws -> [String: Any] {
    let url = try XCTUnwrap(
      Bundle(for: Self.self).url(
        forResource: "\(family)_parity_v1.expected", withExtension: "json"
      ),
      "\(family) parity report is not in the test bundle"
    )
    return try XCTUnwrap(
      try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
    )
  }
}
