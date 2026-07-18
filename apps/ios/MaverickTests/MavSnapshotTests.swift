import XCTest
@testable import Mav

final class MavSnapshotTests: XCTestCase {
  /// Decodes the shared canonical fixture the core pins and Kotlin decodes too (PL-P7 parity).
  func testDecodesThePlatformFixtureExactly() throws {
    let url = try XCTUnwrap(
      Bundle(for: Self.self).url(forResource: "host_snapshot_v1.expected", withExtension: "json"),
      "fixtures/platform/host_snapshot_v1.expected.json missing from the test bundle"
    )
    let fixture = try XCTUnwrap(
      try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
    )
    let json = try XCTUnwrap(fixture["json"] as? String)
    let hash = try XCTUnwrap(fixture["hash"] as? String)

    let snapshot = try MavSnapshotDecoder.decode(json: json, hash: hash)

    XCTAssertEqual(snapshot.coreVersion, "0.1.0")
    XCTAssertEqual(snapshot.storageSchema, 1)
    XCTAssertEqual(snapshot.revision, 1)
    XCTAssertEqual(snapshot.asOfUnixMs, 1_752_600_500_000)
    XCTAssertEqual(snapshot.connectionState, "streaming")
    XCTAssertEqual(snapshot.deviceName, "MG")
    XCTAssertNil(snapshot.batteryPercent)
    XCTAssertNil(snapshot.charging)
    XCTAssertNil(snapshot.onWrist)
    XCTAssertEqual(snapshot.lastSampleUnixMs, 1_752_600_500_000)
    XCTAssertEqual(snapshot.currentBpm, 72)
    XCTAssertEqual(snapshot.meanMilliBpm, 72_000)
    XCTAssertEqual(snapshot.inRangeSamples, 1)
    XCTAssertEqual(snapshot.excludedSamples, 0)

    let prv = try XCTUnwrap(snapshot.prv)
    XCTAssertEqual(prv.label, "pulse_rate_variability")
    XCTAssertEqual(prv.intervalSource, "ppg")
    XCTAssertEqual(prv.meanIntervalMicros, 828_000)
    XCTAssertEqual(prv.rmssdMicros, 67_454)
    XCTAssertEqual(prv.sdnnMicros, 46_583)
    XCTAssertEqual(prv.nn50Count, 2)
    XCTAssertEqual(prv.pnn50MilliPercent, 50_000)
    XCTAssertEqual(prv.intervalCount, 5)
    XCTAssertEqual(prv.excludedIntervalCount, 1)
    XCTAssertEqual(prv.algorithm, "time_domain_interval_variability")
    XCTAssertEqual(prv.algorithmVersion, "1.0.0")
    XCTAssertEqual(prv.provenanceId, 3)

    XCTAssertNil(snapshot.prvUnavailableReason)
    XCTAssertEqual(snapshot.recoveryUnavailableReason, "Recovery model not admitted")
    XCTAssertEqual(snapshot.hash, hash)
  }

  func testDecodesStructuredRecoveryUnavailability() throws {
    let snapshot = try MavSnapshotDecoder.decode(json: """
    {
      "schema":"host-snapshot/v1",
      "core_version":"0.1.0",
      "storage_schema":1,
      "revision":4,
      "as_of_unix_ms":9,
      "connection":{"state":"connected","display_name":"Test strap"},
      "session":{"current_bpm":61,"mean_milli_bpm":null,"in_range_samples":1,"excluded_samples":0},
      "analytics":{"availability":[{"analytic":"recovery","available":false,"reason":{"kind":"missing_streams","streams":["rr_interval"]}}]}
    }
    """, hash: "fixture-hash")

    XCTAssertEqual(snapshot.currentBpm, 61)
    XCTAssertEqual(snapshot.deviceName, "Test strap")
    XCTAssertEqual(snapshot.recoveryUnavailableReason, "Needs rr_interval")
    XCTAssertEqual(snapshot.hash, "fixture-hash")
  }

  func testDecodesBatteryAndWristStateWhenPresent() throws {
    let snapshot = try MavSnapshotDecoder.decode(json: """
    {
      "schema":"host-snapshot/v1",
      "core_version":"0.1.0",
      "storage_schema":1,
      "revision":3,
      "as_of_unix_ms":7,
      "connection":{"state":"streaming","display_name":"MG","battery_percent":81,"charging":null,"on_wrist":true},
      "session":null,
      "analytics":null
    }
    """, hash: "batt")

    XCTAssertEqual(snapshot.batteryPercent, 81)
    XCTAssertNil(snapshot.charging)
    XCTAssertEqual(snapshot.onWrist, true)
  }

  func testMissingRrStreamsMakePrvUnavailableWithTheExactReason() throws {
    let snapshot = try MavSnapshotDecoder.decode(json: """
    {
      "schema":"host-snapshot/v1",
      "core_version":"0.1.0",
      "storage_schema":1,
      "revision":2,
      "as_of_unix_ms":5,
      "connection":{"state":"streaming","display_name":"MG"},
      "session":null,
      "analytics":{
        "variability_label":null,
        "availability":[
          {"analytic":"time_domain_hrv","available":false,
           "reason":{"kind":"missing_streams","streams":["rr_interval"]}}
        ]
      }
    }
    """, hash: "abc")

    XCTAssertNil(snapshot.prv)
    XCTAssertEqual(snapshot.prvUnavailableReason, "Needs rr_interval")
  }

  func testRejectsUnknownSnapshotSchema() {
    XCTAssertThrowsError(try MavSnapshotDecoder.decode(json: """
    {"schema":"host-snapshot/v2","core_version":"0.1.0","storage_schema":1,"revision":0,"as_of_unix_ms":0,"connection":{"state":"idle"}}
    """, hash: "hash"))
  }
}
