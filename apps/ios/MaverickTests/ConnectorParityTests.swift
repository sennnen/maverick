import XCTest

final class ConnectorParityTests: XCTestCase {
  func testFrozenConnectorParityReportsMeetMobileBudgets() throws {
    let expected = [
      ("whoop4", "dev.maverick.whoop4", 14,
       "ea7e360add1365a2ca8e1f06bb5631cda25fda93c601bd90b6b6f000a22e4df0"),
      ("whoop5", "dev.maverick.whoop5", 12,
       "7829241ae70b256eb84ab70a9b8a5eac44512009fcf15aba5967cb35df94221d"),
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
