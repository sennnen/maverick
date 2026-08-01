import XCTest

final class ConnectorParityTests: XCTestCase {
  func testFrozenConnectorParityReportsMeetMobileBudgets() throws {
    let expected = [
      ("whoop4", "dev.maverick.whoop4", 16,
       "e5f625b8cd4645cb0b09e69ae9ef5ce496293bab5e944d102284ab4af2a45989"),
      ("whoop5", "dev.maverick.whoop5", 16,
       "3062689f5278ae2c2d0c6a744a854badae7f91d172da518670394aa8fee83632"),
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
