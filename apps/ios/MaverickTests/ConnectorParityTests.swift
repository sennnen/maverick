import XCTest

final class ConnectorParityTests: XCTestCase {
  func testFrozenConnectorParityReportsMeetMobileBudgets() throws {
    let expected = [
      ("whoop4", "dev.maverick.whoop4", 14,
       "3158072c210ff18a510e044192a28b781669a276cab6279ed0ae58dfef23c72d"),
      ("whoop5", "dev.maverick.whoop5", 12,
       "3c4c013f6c593c411fb822e65b8c363a6524dbf759390c10781a8bae695cfd47"),
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
      for fixture in fixtures {
        XCTAssertLessThanOrEqual(try XCTUnwrap(fixture["max_fuel_consumed"] as? Int), 5_000_000)
        XCTAssertLessThanOrEqual(try XCTUnwrap(fixture["peak_memory_bytes"] as? Int), 4 * 1024 * 1024)
      }
    }
  }
}
