import XCTest
@testable import Maverick

final class MavSnapshotTests: XCTestCase {
  func testDecodesStructuredRecoveryUnavailability() throws {
    let snapshot = try MavSnapshotDecoder.decode(json: """
    {
      "schema":"host-snapshot/v1",
      "core_version":"0.1.0",
      "storage_schema":1,
      "revision":4,
      "connection":{"state":"connected","display_name":"Test strap"},
      "session":{"current_bpm":61},
      "analytics":{"availability":[{"analytic":"recovery","available":false,"reason":{"kind":"missing_streams","streams":["rr_interval"]}}]}
    }
    """, hash: "fixture-hash")

    XCTAssertEqual(snapshot.currentBpm, 61)
    XCTAssertEqual(snapshot.deviceName, "Test strap")
    XCTAssertEqual(snapshot.recoveryUnavailableReason, "Needs rr_interval")
    XCTAssertEqual(snapshot.hash, "fixture-hash")
  }

  func testRejectsUnknownSnapshotSchema() {
    XCTAssertThrowsError(try MavSnapshotDecoder.decode(json: """
    {"schema":"host-snapshot/v2","core_version":"0.1.0","storage_schema":1,"revision":0,"connection":{"state":"idle"}}
    """, hash: "hash"))
  }
}
