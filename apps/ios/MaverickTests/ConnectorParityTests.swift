import XCTest

final class ConnectorParityTests: XCTestCase {
  func testFrozenConnectorParityReportsMeetMobileBudgets() throws {
    let expected = [
      // Frozen against the signed registry at maverick-connectors@6f21fcb — whoop4 1.0.3 and
      // whoop5 1.0.7, the release that added ECG capture. The Kotlin twin in
      // ConnectorParityTest.kt carries the same two constants; both move together with
      // CONNECTORS_REF, never on their own.
      ("whoop4", "dev.maverick.whoop4", 18,
       "d3dae33eb0849f6eec489473d5ddd38ff39506e74ec40c6ca57a2b513491a145"),
      ("whoop5", "dev.maverick.whoop5", 16,
       "a37e0acdaf161ad1a94fd81d65be9c0572285124a3ee17e262b1bf492b86a7b5"),
    ]
    for (family, connectorID, fixtureCount, artifactHash) in expected {
      let url = try XCTUnwrap(
        Bundle(for: Self.self).url(
          forResource: "\(family)_parity_v1.expected", withExtension: "json"
        )
      )
      let report = try XCTUnwrap(
        try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
      )
      XCTAssertEqual(report["schema"] as? String, "mavconn-parity/v1")
      XCTAssertEqual(report["connector_id"] as? String, connectorID)
      XCTAssertEqual(report["artifact_sha256"] as? String, artifactHash)
      XCTAssertEqual(report["fixture_count"] as? Int, fixtureCount)

      let fixtures = try XCTUnwrap(report["fixtures"] as? [[String: Any]])
      let names = Set(fixtures.compactMap { $0["name"] as? String })
      XCTAssertTrue(names.isSuperset(of: [
        "history-cursor-retry", "state-restart", "malformed-frame",
      ]))
      if family == "whoop5" {
        XCTAssertTrue(names.isSuperset(of: [
          "mg-ecg-capture", "non-mg-ecg-fails-closed",
        ]))
      }
      for fixture in fixtures {
        XCTAssertLessThanOrEqual(try XCTUnwrap(fixture["max_fuel_consumed"] as? Int), 5_000_000)
        XCTAssertLessThanOrEqual(try XCTUnwrap(fixture["peak_memory_bytes"] as? Int), 4 * 1024 * 1024)
      }
    }
  }
}
