import Foundation
import XCTest
@testable import Mav

final class ConnectorManagementTests: XCTestCase {
  func testEveryAcquisitionSourcePreservesExactBytesAndSanitizesLocator() throws {
    let bytes = Data([0, 1, 2, 0xff])
    let file = try ConnectorAcquisition.make(
      bytes: bytes, origin: .file, displayName: "sensor.mavconn", locator: "/private/a/sensor.mavconn")
    let share = try ConnectorAcquisition.make(
      bytes: bytes, origin: .share, displayName: "sensor.mavconn", locator: "/private/b/sensor.mavconn")
    let remote = try ConnectorAcquisition.make(
      bytes: bytes, origin: .remote, displayName: "sensor.mavconn", locator: "https://example.test/sensor.mavconn")

    XCTAssertEqual(file.bytes, bytes)
    XCTAssertEqual(share.bytes, bytes)
    XCTAssertEqual(remote.bytes, bytes)
    XCTAssertEqual(file.source.displayName, "sensor.mavconn")
    XCTAssertEqual(file.source.locatorDigest.count, 32)
    XCTAssertFalse(String(decoding: file.source.locatorDigest, as: UTF8.self).contains("private"))
  }

  func testOversizedArtifactIsRejectedBeforeInspection() {
    XCTAssertThrowsError(try ConnectorAcquisition.make(
      bytes: Data(repeating: 0, count: ConnectorAcquisition.maximumBytes + 1),
      origin: .file,
      displayName: "large.mavconn",
      locator: "large.mavconn"
    )) { error in
      XCTAssertEqual(error as? ConnectorAcquisitionError, .tooLarge)
    }
  }

  func testRegistryCachePreservesExactSignedBytesAndRevocations() throws {
    let checkpoint = ConnectorRegistryCheckpoint(
      registryId: "org.example.registry",
      revision: 3,
      digest: Data(repeating: 7, count: 32),
      revocationRevision: 2,
      revocations: [ConnectorRevocationRecord(
        publisherKeyId: "publisher-v1", revokedAtMs: 42, reason: "compromised")])
    let cache = CachedConnectorRegistry(bytes: Data([0, 1, 255]), checkpoint: checkpoint)
    let restored = try JSONDecoder().decode(
      CachedConnectorRegistry.self, from: JSONEncoder().encode(cache))
    XCTAssertEqual(restored.bytes, Data([0, 1, 255]))
    XCTAssertEqual(restored.checkpoint.digest, checkpoint.digest)
    XCTAssertEqual(restored.checkpoint.revocations.first?.reason, "compromised")
  }

  func testApprovalCannotRunBeforeInspectionAndCancelClearsPendingBytes() {
    var machine = ConnectorApprovalMachine()
    XCTAssertThrowsError(try machine.beginApproval())

    machine.beginInspection()
    machine.inspectionSucceeded(
      ConnectorApprovalSummary(
        connectorID: "org.example.sensor", version: "1.0.0", displayName: "Example Sensor",
        publisherKeyID: "publisher.example", fixtureCount: 4),
      artifactBytes: Data([1, 2, 3])
    )
    XCTAssertNoThrow(try machine.beginApproval())
    machine.cancel()
    XCTAssertEqual(machine.phase, .idle)
    XCTAssertNil(machine.pendingBytes)
  }

  func testFailureRollbackAndRevocationHaveExplicitStates() {
    var machine = ConnectorApprovalMachine()
    machine.fail("Signature rejected")
    XCTAssertEqual(machine.phase, .failed("Signature rejected"))
    machine.rolledBack(connectorID: "org.example.sensor")
    XCTAssertEqual(machine.phase, .rolledBack("org.example.sensor"))
    machine.revoked(connectorID: "org.example.sensor")
    XCTAssertEqual(machine.phase, .revoked("org.example.sensor"))
  }

  func testGenericTransportRequestsMapWithoutDeviceKnowledge() {
    XCTAssertEqual(
      ConnectorNativeOperation.map(.startScan(serviceUuids: ["180D"], manufacturerIds: [])),
      .scan(serviceUUIDs: ["180D"], manufacturerIDs: []))
    XCTAssertEqual(ConnectorNativeOperation.map(.connect(address: "A")), .connect("A"))
    XCTAssertEqual(
      ConnectorNativeOperation.map(.subscribe(
        characteristicId: "measurement", serviceUuid: "180D", characteristicUuid: "2A37")),
      .subscribe(id: "measurement", service: "180D", characteristic: "2A37")
    )
    XCTAssertEqual(
      ConnectorNativeOperation.map(.write(
        characteristicId: "control", serviceUuid: "180D", characteristicUuid: "2A39",
        bytes: Data([1]), confirmed: true)),
      .write(id: "control", service: "180D", characteristic: "2A39", bytes: Data([1]), confirmed: true)
    )
  }

  func testRestorationCheckpointContainsOpaqueRuntimeIdentityOnly() throws {
    let checkpoint = ConnectorRestorationCheckpoint(
      connectorID: "org.example.sensor", sessionID: 42, cancellationGeneration: 3)
    let restored = try JSONDecoder().decode(
      ConnectorRestorationCheckpoint.self, from: JSONEncoder().encode(checkpoint))
    XCTAssertEqual(restored, checkpoint)
    XCTAssertFalse(String(data: try JSONEncoder().encode(checkpoint), encoding: .utf8)!.contains("mavconn"))
  }
}
