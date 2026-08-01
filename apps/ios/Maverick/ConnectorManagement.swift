import CryptoKit
import Foundation

enum ConnectorImportOrigin: Equatable {
  case file
  case share
  case remote
  /// Shipped inside the app. The only connector that is: see `BundledConnector`.
  case bundled

  var sourceKind: ConnectorSourceKind {
    switch self {
    case .file, .share: .imported
    case .remote: .remote
    case .bundled: .bundled
    }
  }
}

/// The one connector Maverick ships with.
///
/// Everything else installs from a file, a share or the registry — that is the product invariant,
/// and bundling drivers is what the connector architecture exists to avoid. This one is the
/// exception because it is not a driver for a device: it speaks the Bluetooth SIG heart-rate
/// profile, which is a published standard rather than anyone's protocol, so a brand-new install has
/// something to pair with before the wearer has found any connector at all. It also happens to be
/// the only source of genuine heart-rate variability, because a chest strap times its beats
/// electrically.
enum BundledConnector {
  static let resource = "generic-hr"
  static let displayName = "Generic HR Monitor"
  static let connectorID = "dev.maverick.generic-hr"

  static func bytes() -> Data? {
    Bundle.main.url(forResource: resource, withExtension: "mavconn")
      .flatMap { try? Data(contentsOf: $0) }
  }
}

enum ConnectorAcquisitionError: Error, Equatable, LocalizedError {
  case empty
  case tooLarge
  case unsupportedURL
  case remoteImportDisabled
  case invalidResponse
  case transport(String)

  var errorDescription: String? {
    switch self {
    case .empty: "The connector file is empty."
    case .tooLarge: "The connector is larger than the 4 MB safety limit."
    case .unsupportedURL: "Use a local file or an HTTPS URL."
    case .remoteImportDisabled: "Remote connector import is disabled in this release."
    case .invalidResponse: "The connector download returned an invalid response."
    case let .transport(message): message
    }
  }
}

struct ConnectorAcquisition: Equatable {
  static let maximumBytes = 4 * 1_024 * 1_024

  let bytes: Data
  let source: ConnectorSourceMetadata

  static func make(
    bytes: Data,
    origin: ConnectorImportOrigin,
    displayName: String,
    locator: String
  ) throws -> ConnectorAcquisition {
    guard !bytes.isEmpty else { throw ConnectorAcquisitionError.empty }
    guard bytes.count <= maximumBytes else { throw ConnectorAcquisitionError.tooLarge }
    let safeName = URL(fileURLWithPath: displayName).lastPathComponent
    let digest = Data(SHA256.hash(data: Data(locator.utf8)))
    return ConnectorAcquisition(
      bytes: bytes,
      source: ConnectorSourceMetadata(
        kind: origin.sourceKind,
        displayName: safeName.isEmpty ? "Connector" : safeName,
        locatorDigest: digest
      )
    )
  }
}

struct ConnectorApprovalSummary: Equatable {
  let connectorID: String
  let version: String
  let displayName: String
  let publisherKeyID: String
  let fixtureCount: UInt32
  var detail: String = ""
  var sourceName: String = ""
  var capabilities: [String] = []
  var permissions: [String] = []
}

struct ConnectorConnectionState: Equatable {
  var connectorID: String?
  var lifecycle: ConnectorLifecycleState?
  var label: String
  var connected: Bool
  var heartRateBPM: Int?
  var batteryPercent: Int?
  var onWrist: Bool?
  var lastSampleWallTimeMs: Int64?
  var errorMessage: String?
  /// The device the open session is bound to, so history can be read for it.
  var deviceID: UInt64?

  static let disconnected = ConnectorConnectionState(
    connectorID: nil, lifecycle: nil, label: "Disconnected", connected: false,
    heartRateBPM: nil, batteryPercent: nil, onWrist: nil,
    lastSampleWallTimeMs: nil, errorMessage: nil)

  init(
    connectorID: String?, lifecycle: ConnectorLifecycleState?, label: String, connected: Bool,
    heartRateBPM: Int?, batteryPercent: Int?, onWrist: Bool?,
    lastSampleWallTimeMs: Int64?, errorMessage: String?, deviceID: UInt64? = nil
  ) {
    self.connectorID = connectorID
    self.lifecycle = lifecycle
    self.label = label
    self.connected = connected
    self.heartRateBPM = heartRateBPM
    self.batteryPercent = batteryPercent
    self.onWrist = onWrist
    self.lastSampleWallTimeMs = lastSampleWallTimeMs
    self.errorMessage = errorMessage
    self.deviceID = deviceID
  }

  init(telemetry: ConnectorTelemetrySnapshot) {
    connectorID = telemetry.connectorId
    lifecycle = telemetry.lifecycle
    label = Self.label(for: telemetry.lifecycle)
    connected = telemetry.lifecycle == .streaming || telemetry.lifecycle == .historical
    heartRateBPM = telemetry.heartRateBpm.map(Int.init)
    batteryPercent = telemetry.batteryPercent.map(Int.init)
    onWrist = telemetry.onWrist
    lastSampleWallTimeMs = telemetry.lastSampleWallTimeMs
    errorMessage = nil
    deviceID = telemetry.deviceId
  }

  private static func label(for lifecycle: ConnectorLifecycleState) -> String {
    switch lifecycle {
    case .installed: "Installed"
    case .selected: "Starting"
    case .scanning: "Scanning"
    case .connecting: "Connecting"
    case .discovering: "Discovering services"
    case .pairing: "Pairing"
    case .configuring: "Configuring"
    case .streaming: "Streaming"
    case .historical: "Syncing history"
    case .suspending: "Suspending"
    case .disconnected: "Disconnected"
    case .failed: "Failed"
    }
  }
}

struct ConnectorScanDevice: Equatable, Identifiable {
  let id: String
  let name: String
  let rssi: Int
}

struct ConnectorApprovalMachine {
  enum Phase: Equatable {
    case idle
    case inspecting
    case awaitingApproval(ConnectorApprovalSummary)
    case installing(ConnectorApprovalSummary)
    case installed(String)
    case failed(String)
    case rolledBack(String)
    case revoked(String)
  }

  enum TransitionError: Error, Equatable { case inspectionRequired }

  private(set) var phase: Phase = .idle
  private(set) var pendingBytes: Data?

  mutating func beginInspection() {
    pendingBytes = nil
    phase = .inspecting
  }

  mutating func inspectionSucceeded(_ summary: ConnectorApprovalSummary, artifactBytes: Data) {
    pendingBytes = artifactBytes
    phase = .awaitingApproval(summary)
  }

  mutating func beginApproval() throws {
    guard case let .awaitingApproval(summary) = phase, pendingBytes != nil else {
      throw TransitionError.inspectionRequired
    }
    phase = .installing(summary)
  }

  mutating func installed(connectorID: String) {
    pendingBytes = nil
    phase = .installed(connectorID)
  }

  mutating func cancel() {
    pendingBytes = nil
    phase = .idle
  }

  mutating func fail(_ message: String) {
    pendingBytes = nil
    phase = .failed(message)
  }

  mutating func rolledBack(connectorID: String) {
    pendingBytes = nil
    phase = .rolledBack(connectorID)
  }

  mutating func revoked(connectorID: String) {
    pendingBytes = nil
    phase = .revoked(connectorID)
  }
}

enum ConnectorNativeOperation: Equatable {
  case scan(serviceUUIDs: [String], manufacturerIDs: [UInt16])
  case stopScan
  case connect(String)
  case ensurePaired
  case discoverServices
  case subscribe(id: String, service: String, characteristic: String)
  case unsubscribe(id: String, service: String, characteristic: String)
  case read(id: String, service: String, characteristic: String)
  case write(id: String, service: String, characteristic: String, bytes: Data, confirmed: Bool)
  case disconnect
  case setTimer(token: UInt64, delayMs: UInt64)
  case cancelTimer(token: UInt64)

  static func map(_ request: ConnectorTransportRequest) -> ConnectorNativeOperation {
    switch request {
    case let .startScan(serviceUuids, manufacturerIds):
      .scan(serviceUUIDs: serviceUuids, manufacturerIDs: manufacturerIds)
    case .stopScan: .stopScan
    case let .connect(address): .connect(address)
    case .ensurePaired: .ensurePaired
    case .discoverServices: .discoverServices
    case let .subscribe(characteristicId, serviceUuid, characteristicUuid):
      .subscribe(id: characteristicId, service: serviceUuid, characteristic: characteristicUuid)
    case let .unsubscribe(characteristicId, serviceUuid, characteristicUuid):
      .unsubscribe(id: characteristicId, service: serviceUuid, characteristic: characteristicUuid)
    case let .read(characteristicId, serviceUuid, characteristicUuid):
      .read(id: characteristicId, service: serviceUuid, characteristic: characteristicUuid)
    case let .write(characteristicId, serviceUuid, characteristicUuid, bytes, confirmed):
      .write(
        id: characteristicId,
        service: serviceUuid,
        characteristic: characteristicUuid,
        bytes: bytes,
        confirmed: confirmed)
    case .disconnect: .disconnect
    case let .setTimer(token, delayMs): .setTimer(token: token, delayMs: delayMs)
    case let .cancelTimer(token): .cancelTimer(token: token)
    }
  }
}

struct ConnectorRestorationCheckpoint: Codable, Equatable {
  let connectorID: String
  let sessionID: UInt64
  let cancellationGeneration: UInt64
}

struct ConnectorReleasePolicy {
  let managerEnabled: Bool
  let remoteImportEnabled: Bool
  let trust: ConnectorTrustPolicy
  let revocations: ConnectorTrustRevocations
  let registry: ConnectorRegistryConfiguration?

  static func current(bundle: Bundle = .main, nowMs: Int64) -> ConnectorReleasePolicy {
    let dictionaries = bundle.object(forInfoDictionaryKey: "MAVOfficialPublisherKeys")
      as? [[String: Any]] ?? []
    let keys = dictionaries.compactMap { value -> ConnectorPublisherKey? in
      guard
        let id = value["id"] as? String,
        let encoded = value["publicKeyBase64"] as? String,
        let bytes = Data(base64Encoded: encoded),
        bytes.count == 32
      else { return nil }
      return ConnectorPublisherKey(
        id: id,
        publicKey: bytes,
        scope: .official,
        validFromMs: value["validFromMs"] as? Int64 ?? 0,
        validUntilMs: value["validUntilMs"] as? Int64,
        status: .active,
        statusAtMs: nil,
        statusDetail: nil
      )
    }
    let registry = (bundle.object(forInfoDictionaryKey: "MAVConnectorRegistry") as? [String: Any])
      .flatMap { value -> ConnectorRegistryConfiguration? in
        guard
          let registryID = value["id"] as? String,
          let keyID = value["rootKeyId"] as? String,
          let encoded = value["rootPublicKeyBase64"] as? String,
          let publicKey = Data(base64Encoded: encoded), publicKey.count == 32,
          let rawURL = value["url"] as? String,
          let url = URL(string: rawURL), url.scheme?.lowercased() == "https"
        else { return nil }
        return ConnectorRegistryConfiguration(
          url: url,
          root: ConnectorRegistryRoot(
            registryId: registryID, keyId: keyID, publicKey: publicKey))
      }
    let configured = ConnectorReleasePolicy(
      managerEnabled: bundle.object(forInfoDictionaryKey: "MAVConnectorManagerEnabled") as? Bool ?? false,
      remoteImportEnabled: bundle.object(forInfoDictionaryKey: "MAVAllowRemoteConnectorImport") as? Bool ?? false,
      trust: ConnectorTrustPolicy(
        revision: 1,
        allowThirdParty: false,
        allowDevelopment: false,
        keys: keys
      ),
      revocations: ConnectorTrustRevocations(
        revision: 0,
        generatedAtMs: registry == nil ? 0 : 1,
        validUntilMs: registry == nil ? nowMs + 31_536_000_000 : 0,
        entries: []
      ),
      registry: registry
    )
#if DEBUG
    if configured.trust.keys.isEmpty && configured.registry == nil {
      return development(nowMs: nowMs)
    }
#endif
    return configured
  }

  static func development(nowMs: Int64) -> ConnectorReleasePolicy {
    let livePublisher = Data(base64Encoded: "4bcav9MjKAQmHkI/NlVva0GFvtQf39ANdpzhWjlPQ84=") ?? Data()
    let registryRoot = Data(base64Encoded: "e+KbmLoJN+rfofwbR5MfBd/OSfru6JbA9Awkp41kn+Y=") ?? Data()
    return ConnectorReleasePolicy(
      managerEnabled: true,
      remoteImportEnabled: true,
      trust: ConnectorTrustPolicy(
        revision: 1,
        allowThirdParty: false,
        allowDevelopment: true,
        keys: [
          ConnectorPublisherKey(
            id: "maverick-whoop-live-test",
            publicKey: livePublisher,
            scope: .development,
            validFromMs: 0,
            validUntilMs: nil,
            status: .active,
            statusAtMs: nil,
            statusDetail: nil),
        ]),
      revocations: ConnectorTrustRevocations(
        revision: 0, generatedAtMs: 0, validUntilMs: nowMs + 31_536_000_000, entries: []),
      registry: ConnectorRegistryConfiguration(
        url: URL(string: "https://raw.githubusercontent.com/sennnen/maverick-connectors/main/registry/index-v1.json")!,
        root: ConnectorRegistryRoot(
          registryId: "dev.maverick.connectors",
          keyId: "registry-root-v1",
          publicKey: registryRoot)))
  }
}

struct ConnectorRegistryConfiguration {
  let url: URL
  let root: ConnectorRegistryRoot
}

struct CachedRegistryRevocation: Codable {
  let publisherKeyID: String
  let revokedAtMs: Int64
  let reason: String
}

struct CachedConnectorRegistry: Codable {
  let bytes: Data
  let registryID: String
  let revision: UInt64
  let digest: Data
  let revocationRevision: UInt64
  let revocations: [CachedRegistryRevocation]

  init(bytes: Data, checkpoint: ConnectorRegistryCheckpoint) {
    self.bytes = bytes
    registryID = checkpoint.registryId
    revision = checkpoint.revision
    digest = checkpoint.digest
    revocationRevision = checkpoint.revocationRevision
    revocations = checkpoint.revocations.map {
      CachedRegistryRevocation(
        publisherKeyID: $0.publisherKeyId, revokedAtMs: $0.revokedAtMs, reason: $0.reason)
    }
  }

  var checkpoint: ConnectorRegistryCheckpoint {
    ConnectorRegistryCheckpoint(
      registryId: registryID,
      revision: revision,
      digest: digest,
      revocationRevision: revocationRevision,
      revocations: revocations.map {
        ConnectorRevocationRecord(
          publisherKeyId: $0.publisherKeyID, revokedAtMs: $0.revokedAtMs, reason: $0.reason)
      })
  }
}

@MainActor
final class ConnectorManager: ObservableObject {
  @Published private(set) var machine = ConnectorApprovalMachine()
  @Published private(set) var installed: [InstalledConnectorRecord] = []
  @Published private(set) var registryEntries: [ConnectorRegistryEntry] = []
  @Published private(set) var registryError: String?
  @Published private(set) var connection = ConnectorConnectionState.disconnected
  @Published private(set) var discoveredDevices: [ConnectorScanDevice] = []
  /// The trailing day history the trend and vitals surfaces read, straight from the core.
  @Published private(set) var days: [DailySnapshotReport] = []
  @Published private(set) var ecgCapabilities: [ConnectorCaptureCapability] = []
  @Published private(set) var ecgCapture: EcgCaptureReport?
  @Published private(set) var ecgResults: [EcgResultReport] = []
  @Published private(set) var ecgError: String?

  private let worker: ConnectorRuntimeWorker
  private var ecgInferenceInFlight: UInt64?

  /// Battery saver (ADR-030). Routed to the core, which applies it to the live session and to any
  /// session started afterwards.
  func setLowPower(_ on: Bool) { worker.setLowPower(on) }
  private var inspection: ConnectorInspection?
  private var acquisition: ConnectorAcquisition?
  private lazy var bluetooth = MavBluetoothExecutor(
    eventSink: { [weak self] event in self?.applyTransportEvent(event) },
    discoverySink: { [weak self] devices in self?.discoveredDevices = devices },
    errorSink: { [weak self] message in
      self?.failConnection(ConnectorAcquisitionError.transport(message))
    }
  )
  private(set) var releasePolicy: ConnectorReleasePolicy
  private var registryCheckpoint: ConnectorRegistryCheckpoint?
  private let registryCacheKey = "mav.connector-registry.cache.v1"

  init() {
    let now = Self.nowMs
    releasePolicy = .current(nowMs: now)
    worker = ConnectorRuntimeWorker(config: MavStore.runtimeConfig())
    worker.publishTimezoneSpans { _ in }
    if !restoreRegistryIfAvailable() { refreshRegistry() }
    installBundledConnectorIfMissing()
    refreshInstalled()
    refreshEcgHistory()
    DispatchQueue.main.async { [weak self] in self?.resumeIfNeeded() }
  }

  /// Install the shipped Generic HR Monitor the first time the app runs, so a fresh install can
  /// pair with a chest strap before the wearer has found any connector at all.
  ///
  /// It goes through the same public path every other connector uses — inspect, then install
  /// against the approval token that inspection issued — because a bundled artifact that skipped
  /// verification would be a second trust path, and the whole point is that there is only one.
  /// Already installed is the normal case and is silent.
  private func installBundledConnectorIfMissing() {
    // Read the policy on the actor that owns it and carry the value into the callback. Reaching
    // for `self.releasePolicy` from inside a Sendable completion is a data race the compiler now
    // rejects, and it was always one.
    let policy = releasePolicy
    worker.list { [weak self] result in
      guard let self,
        case let .success(records) = result,
        !records.contains(where: { $0.connectorId == BundledConnector.connectorID }),
        let bytes = BundledConnector.bytes(),
        let acquisition = try? ConnectorAcquisition.make(
          bytes: bytes, origin: .bundled, displayName: BundledConnector.displayName,
          locator: BundledConnector.resource)
      else { return }

      self.worker.inspect(acquisition: acquisition, policy: policy) { inspected in
        guard case let .success(inspection) = inspected else { return }
        self.worker.install(acquisition: acquisition, inspection: inspection, policy: policy) {
          _ in
          DispatchQueue.main.async { self.refreshInstalled() }
        }
      }
    }
  }

  func refreshRegistry() {
    guard let configuration = releasePolicy.registry else { return }
    Task {
      do {
        let bytes = try await Self.download(
          configuration.url, maximumBytes: 1_024 * 1_024)
        worker.ingestRegistry(
          bytes: bytes,
          root: configuration.root,
          previous: registryCheckpoint,
          policy: releasePolicy.trust
        ) { [weak self] result in
          Task { @MainActor in
            guard let self else { return }
            switch result {
            case let .success(snapshot):
              self.registryError = nil
              self.applyRegistry(snapshot, bytes: bytes)
            case let .failure(error): self.registryError = Self.message(error)
            }
          }
        }
      } catch { registryError = Self.message(error) }
    }
  }

  func importRegistryEntry(_ entry: ConnectorRegistryEntry) {
    guard releasePolicy.remoteImportEnabled, !entry.revoked,
      let url = URL(string: entry.artifactUrl), url.scheme?.lowercased() == "https"
    else {
      machine.fail("This registry connector is not available for remote import.")
      return
    }
    Task {
      do {
        let bytes = try await Self.download(url, maximumBytes: ConnectorAcquisition.maximumBytes)
        worker.verifyRegistryArtifact(entry: entry, bytes: bytes) { [weak self] result in
          Task { @MainActor in
            guard let self else { return }
            switch result {
            case .success:
              do {
                self.inspect(try ConnectorAcquisition.make(
                  bytes: bytes,
                  origin: .remote,
                  displayName: "\(entry.connectorId)-\(entry.version).mavconn",
                  locator: entry.artifactUrl))
              } catch { self.machine.fail(Self.message(error)) }
            case let .failure(error): self.machine.fail(Self.message(error))
            }
          }
        }
      } catch { machine.fail(Self.message(error)) }
    }
  }

  var phase: ConnectorApprovalMachine.Phase { machine.phase }

  func reportAcquisitionFailure(_ error: Error) {
    machine.fail(Self.message(error))
  }

  func importFile(_ url: URL, origin: ConnectorImportOrigin = .file) {
    guard releasePolicy.managerEnabled else {
      machine.fail("Connector management is disabled in this release.")
      return
    }
    Task {
      do {
        let payload = try await Task.detached {
          let scoped = url.startAccessingSecurityScopedResource()
          defer { if scoped { url.stopAccessingSecurityScopedResource() } }
          let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize
          if let size, size > ConnectorAcquisition.maximumBytes {
            throw ConnectorAcquisitionError.tooLarge
          }
          return try ConnectorAcquisition.make(
            bytes: Data(contentsOf: url, options: [.mappedIfSafe, .uncached]),
            origin: origin,
            displayName: url.lastPathComponent,
            locator: url.absoluteString
          )
        }.value
        inspect(payload)
      } catch { machine.fail(Self.message(error)) }
    }
  }

  func importRemote(_ url: URL) {
    guard url.scheme?.lowercased() == "https" else {
      machine.fail(ConnectorAcquisitionError.unsupportedURL.localizedDescription)
      return
    }
    guard releasePolicy.remoteImportEnabled else {
      machine.fail(ConnectorAcquisitionError.remoteImportDisabled.localizedDescription)
      return
    }
    Task {
      do {
        var request = URLRequest(url: url)
        request.cachePolicy = .reloadIgnoringLocalAndRemoteCacheData
        request.timeoutInterval = 30
        let (stream, response) = try await URLSession.shared.bytes(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
          throw ConnectorAcquisitionError.invalidResponse
        }
        let expected = response.expectedContentLength
        if expected > Int64(ConnectorAcquisition.maximumBytes) {
          throw ConnectorAcquisitionError.tooLarge
        }
        var bytes = Data()
        bytes.reserveCapacity(min(max(0, Int(expected)), ConnectorAcquisition.maximumBytes))
        for try await byte in stream {
          guard bytes.count < ConnectorAcquisition.maximumBytes else {
            throw ConnectorAcquisitionError.tooLarge
          }
          bytes.append(byte)
        }
        let payload = try ConnectorAcquisition.make(
          bytes: bytes,
          origin: .remote,
          displayName: response.suggestedFilename ?? url.lastPathComponent,
          locator: url.absoluteString
        )
        inspect(payload)
      } catch { machine.fail(Self.message(error)) }
    }
  }

  func approve() {
    do { try machine.beginApproval() } catch {
      machine.fail("Inspect this connector before approving it.")
      return
    }
    guard let inspection, let acquisition else {
      machine.fail("Inspection expired. Import the connector again.")
      return
    }
    worker.install(
      acquisition: acquisition,
      inspection: inspection,
      policy: releasePolicy
    ) { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case let .success(record):
          self.inspection = nil
          self.acquisition = nil
          self.machine.installed(connectorID: record.connectorId)
          self.refreshInstalled()
        case let .failure(error): self.machine.fail(Self.message(error))
        }
      }
    }
  }

  func cancel() {
    inspection = nil
    acquisition = nil
    machine.cancel()
  }

  func rollback(_ connectorID: String) {
    worker.rollback(connectorID: connectorID, policy: releasePolicy) { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case .success:
          self.machine.rolledBack(connectorID: connectorID)
          self.refreshInstalled()
        case let .failure(error): self.machine.fail(Self.message(error))
        }
      }
    }
  }

  func remove(_ record: InstalledConnectorRecord) {
    worker.remove(record: record, policy: releasePolicy) { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case .success: self.cancel(); self.refreshInstalled()
        case let .failure(error): self.machine.fail(Self.message(error))
        }
      }
    }
  }

  func enforceRevocations() {
    worker.enforce(policy: releasePolicy) { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case let .success(ids):
          if let first = ids.first { self.machine.revoked(connectorID: first) }
          self.refreshInstalled()
        case let .failure(error): self.machine.fail(Self.message(error))
        }
      }
    }
  }

  func connect(_ record: InstalledConnectorRecord) {
    let sessionID = UInt64(Date().timeIntervalSince1970 * 1_000)
    let checkpoint = ConnectorRestorationCheckpoint(
      connectorID: record.connectorId,
      sessionID: sessionID,
      cancellationGeneration: 0
    )
    bluetooth.checkpoint = checkpoint
    connection = ConnectorConnectionState(
      connectorID: record.connectorId, lifecycle: .selected, label: "Starting", connected: false,
      heartRateBPM: nil, batteryPercent: nil, onWrist: nil,
      lastSampleWallTimeMs: nil, errorMessage: nil)
    worker.openSession(
      checkpoint: checkpoint,
      policy: releasePolicy
    ) { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case .success: self.drainTransportActions()
        case let .failure(error): self.failConnection(error)
        }
      }
    }
  }

  func disconnect() {
    worker.cancelSession { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case let .success(telemetry):
          self.publishTelemetry(telemetry)
          self.drainTransportActions()
        case let .failure(error): self.failConnection(error)
        }
      }
    }
  }

  func selectDevice(_ id: String) {
    bluetooth.selectDevice(id)
  }

  func startEcgCapture() {
    guard ecgCapabilities.contains(where: { $0.stream == "ecg" }) else {
      ecgError = "This connected device has not positively declared ECG capture."
      return
    }
    ecgError = nil
    worker.startCapture(stream: "ecg") { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case .success: self.refreshEcgState()
        case let .failure(error): self.ecgError = Self.message(error)
        }
      }
    }
  }

  func stopEcgCapture() {
    worker.stopCapture(stream: "ecg") { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case .success: self.refreshEcgState()
        case let .failure(error): self.ecgError = Self.message(error)
        }
      }
    }
  }

  func refreshEcgHistory() {
    worker.ecgResults(deviceID: 1, limit: 50) { [weak self] result in
      Task { @MainActor in
        if case let .success(results) = result { self?.ecgResults = results }
      }
    }
  }

  /// Forget one reading. The history reloads from the store rather than being patched.
  func removeEcgResult(captureID: UInt64) {
    worker.deleteEcgCapture(captureID: captureID) { [weak self] result in
      Task { @MainActor in
        switch result {
        case .success: self?.refreshEcgHistory()
        case let .failure(error): self?.ecgError = error.localizedDescription
        }
      }
    }
  }

  func ecgReportPayload(captureID: UInt64) async throws -> EcgReportPayload {
    try await withCheckedThrowingContinuation { continuation in
      worker.ecgReportPayload(captureID: captureID) { result in
        continuation.resume(with: result.flatMap { payload in
          payload.map(Result.success)
            ?? .failure(ConnectorAcquisitionError.transport("ECG report evidence is unavailable."))
        })
      }
    }
  }

  private func inspect(_ payload: ConnectorAcquisition) {
    machine.beginInspection()
    worker.inspect(acquisition: payload, policy: releasePolicy) { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case let .success(report):
          self.inspection = report
          self.acquisition = payload
          self.machine.inspectionSucceeded(
            ConnectorApprovalSummary(
              connectorID: report.connectorId,
              version: report.version,
              displayName: report.displayName,
              publisherKeyID: report.publisherKeyId,
              fixtureCount: report.fixtureCount,
              detail: report.description,
              sourceName: report.source.displayName,
              capabilities: report.capabilities,
              permissions: report.permissions
            ),
            artifactBytes: payload.bytes
          )
        case let .failure(error): self.machine.fail(Self.message(error))
        }
      }
    }
  }

  private func refreshInstalled() {
    worker.list { [weak self] result in
      Task { @MainActor in
        if case let .success(records) = result { self?.installed = records }
      }
    }
  }

  /// Reload the trailing history for the device a session is open on. Sixty days is what the
  /// longitudinal readout looks back over, so it is what the surfaces can honestly chart.
  func refreshDays(deviceID: UInt64, days trailing: Int = 60) {
    worker.dailySnapshots(deviceID: deviceID, days: trailing) { [weak self] result in
      Task { @MainActor in
        if case let .success(history) = result { self?.days = history }
      }
    }
  }

  @discardableResult
  private func restoreRegistryIfAvailable() -> Bool {
    guard let configuration = releasePolicy.registry,
      let encoded = UserDefaults.standard.data(forKey: registryCacheKey),
      let cached = try? JSONDecoder().decode(CachedConnectorRegistry.self, from: encoded)
    else { return false }
    worker.restoreRegistry(
      bytes: cached.bytes,
      root: configuration.root,
      checkpoint: cached.checkpoint,
      policy: releasePolicy.trust
    ) { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case let .success(snapshot):
          self.registryError = nil
          self.applyRegistry(snapshot, bytes: cached.bytes)
          self.refreshRegistry()
        case let .failure(error):
          self.registryError = Self.message(error)
          self.refreshRegistry()
        }
      }
    }
    return true
  }

  private func applyRegistry(_ snapshot: ConnectorRegistrySnapshot, bytes: Data) {
    registryCheckpoint = snapshot.checkpoint
    registryEntries = snapshot.entries
    releasePolicy = ConnectorReleasePolicy(
      managerEnabled: releasePolicy.managerEnabled,
      remoteImportEnabled: releasePolicy.remoteImportEnabled,
      trust: snapshot.trust,
      revocations: snapshot.revocations,
      registry: releasePolicy.registry)
    if let encoded = try? JSONEncoder().encode(
      CachedConnectorRegistry(bytes: bytes, checkpoint: snapshot.checkpoint))
    {
      UserDefaults.standard.set(encoded, forKey: registryCacheKey)
    }
  }

  private static func download(_ url: URL, maximumBytes: Int) async throws -> Data {
    var request = URLRequest(url: url)
    request.cachePolicy = .reloadIgnoringLocalAndRemoteCacheData
    request.timeoutInterval = 30
    let (stream, response) = try await URLSession.shared.bytes(for: request)
    guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
      throw ConnectorAcquisitionError.invalidResponse
    }
    if response.expectedContentLength > Int64(maximumBytes) {
      throw ConnectorAcquisitionError.tooLarge
    }
    var bytes = Data()
    for try await byte in stream {
      guard bytes.count < maximumBytes else { throw ConnectorAcquisitionError.tooLarge }
      bytes.append(byte)
    }
    guard !bytes.isEmpty else { throw ConnectorAcquisitionError.empty }
    return bytes
  }

  private func resumeIfNeeded() {
    guard let checkpoint = bluetooth.checkpoint else { return }
    worker.openSession(checkpoint: checkpoint, policy: releasePolicy) { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case .success: self.drainTransportActions()
        case let .failure(error): self.failConnection(error)
        }
      }
    }
  }

  private func applyTransportEvent(_ event: ConnectorTransportEvent) {
    worker.apply(event: event) { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case .success: self.drainTransportActions()
        case let .failure(error): self.failConnection(error)
        }
      }
    }
  }

  private func drainTransportActions() {
    worker.drain { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case let .success(actions):
          actions.forEach(self.bluetooth.execute)
          self.refreshTelemetry()
          self.refreshEcgState()
        case let .failure(error): self.failConnection(error)
        }
      }
    }
  }

  private func refreshTelemetry() {
    worker.telemetry { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case let .success(telemetry):
          self.publishTelemetry(telemetry)
          if telemetry.lifecycle == .disconnected || telemetry.lifecycle == .failed {
            self.bluetooth.checkpoint = nil
          }
        case let .failure(error): self.failConnection(error)
        }
      }
    }
  }

  private func refreshEcgState() {
    worker.captureCapabilities { [weak self] result in
      Task { @MainActor in
        if case let .success(capabilities) = result {
          self?.ecgCapabilities = capabilities
        }
      }
    }
    worker.ecgState { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        switch result {
        case let .success(state):
          self.ecgCapture = state
          if state?.phase == "analysing" { self.requestEcgInference() }
        case let .failure(error):
          self.ecgError = Self.message(error)
        }
      }
    }
  }

  private func requestEcgInference() {
    worker.ecgInferenceRequest { [weak self] result in
      Task { @MainActor in
        guard let self, case let .success(request?) = result,
          self.ecgInferenceInFlight != request.captureId
        else { return }
        self.ecgInferenceInFlight = request.captureId
        do {
          let predictions = try await Task.detached(priority: .userInitiated) {
            let classifier = try MavEcgClassifier(bundle: .main)
            return try classifier.predictBatch(request.tensors.map(\.values)).map { values in
              EcgPrediction(
                sinusRhythm: values[0],
                atrialFibrillation: values[1],
                otherAbnormalRhythm: values[2]
              )
            }
          }.value
          self.worker.submitEcgInference(
            captureID: request.captureId,
            predictions: predictions,
            modelSHA256: MavEcgClassifier.admittedModelSHA256
          ) { [weak self] submitted in
            Task { @MainActor in
              guard let self else { return }
              self.ecgInferenceInFlight = nil
              switch submitted {
              case let .success(result):
                self.ecgCapture = EcgCaptureReport(
                  captureId: result.captureId,
                  phase: "result",
                  progressMilli: 1_000,
                  qualityMilli: result.qualityMilli,
                  qualityReason: nil,
                  recordedSamples: result.sampleCount,
                  targetSamples: result.sampleCount
                )
                self.refreshEcgHistory()
              case let .failure(error): self.ecgError = Self.message(error)
              }
            }
          }
        } catch {
          self.ecgInferenceInFlight = nil
          self.ecgError = Self.message(error)
        }
      }
    }
  }

  private func publishTelemetry(_ telemetry: ConnectorTelemetrySnapshot) {
    bluetooth.checkpoint = ConnectorRestorationCheckpoint(
      connectorID: telemetry.connectorId,
      sessionID: telemetry.sessionId,
      cancellationGeneration: telemetry.cancellationGeneration)
    connection = ConnectorConnectionState(telemetry: telemetry)
  }

  private func failConnection(_ error: Error) {
    let message = Self.message(error)
    connection = ConnectorConnectionState(
      connectorID: connection.connectorID,
      lifecycle: .failed,
      label: "Failed",
      connected: false,
      heartRateBPM: connection.heartRateBPM,
      batteryPercent: connection.batteryPercent,
      onWrist: connection.onWrist,
      lastSampleWallTimeMs: connection.lastSampleWallTimeMs,
      errorMessage: message)
  }

  nonisolated private static var nowMs: Int64 { Int64(Date().timeIntervalSince1970 * 1_000) }

  nonisolated private static func message(_ error: Error) -> String {
    (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
  }
}

private final class ConnectorRuntimeWorker: @unchecked Sendable {
  private let queue = DispatchQueue(label: "com.sennnen.mav.connector-runtime", qos: .userInitiated)
  private let config: RuntimeConfig
  private var runtime: MavRuntime?

  init(config: RuntimeConfig) { self.config = config }

  func inspect(
    acquisition: ConnectorAcquisition,
    policy: ConnectorReleasePolicy,
    completion: @escaping @Sendable (Result<ConnectorInspection, Error>) -> Void
  ) {
    perform(completion) { runtime in
      try runtime.inspectConnectorBytes(
        bytes: acquisition.bytes,
        source: acquisition.source,
        policy: policy.trust,
        revocations: policy.revocations,
        nowMs: Self.nowMs,
        approvalTtlMs: 300_000
      )
    }
  }

  func ingestRegistry(
    bytes: Data,
    root: ConnectorRegistryRoot,
    previous: ConnectorRegistryCheckpoint?,
    policy: ConnectorTrustPolicy,
    completion: @escaping @Sendable (Result<ConnectorRegistrySnapshot, Error>) -> Void
  ) {
    perform(completion) {
      try $0.ingestConnectorRegistry(
        bytes: bytes, root: root, previous: previous, policy: policy, nowMs: Self.nowMs)
    }
  }

  func restoreRegistry(
    bytes: Data,
    root: ConnectorRegistryRoot,
    checkpoint: ConnectorRegistryCheckpoint,
    policy: ConnectorTrustPolicy,
    completion: @escaping @Sendable (Result<ConnectorRegistrySnapshot, Error>) -> Void
  ) {
    perform(completion) {
      try $0.restoreConnectorRegistry(
        bytes: bytes, root: root, checkpoint: checkpoint, policy: policy, nowMs: Self.nowMs)
    }
  }

  func verifyRegistryArtifact(
    entry: ConnectorRegistryEntry,
    bytes: Data,
    completion: @escaping @Sendable (Result<Void, Error>) -> Void
  ) {
    perform(completion) { try $0.verifyConnectorRegistryArtifact(entry: entry, bytes: bytes) }
  }

  func install(
    acquisition: ConnectorAcquisition,
    inspection: ConnectorInspection,
    policy: ConnectorReleasePolicy,
    completion: @escaping @Sendable (Result<InstalledConnectorRecord, Error>) -> Void
  ) {
    perform(completion) { runtime in
      try runtime.installConnectorBytes(
        request: ConnectorInstallRequest(
          bytes: acquisition.bytes,
          source: acquisition.source,
          approvalToken: inspection.approvalToken,
          activate: true,
          nowMs: Self.nowMs
        ),
        policy: policy.trust,
        revocations: policy.revocations
      )
    }
  }

  func list(completion: @escaping @Sendable (Result<[InstalledConnectorRecord], Error>) -> Void) {
    perform(completion) { try $0.listInstalledConnectors() }
  }

  func rollback(
    connectorID: String,
    policy: ConnectorReleasePolicy,
    completion: @escaping @Sendable (Result<Void, Error>) -> Void
  ) {
    perform(completion) { runtime in
      try runtime.rollbackInstalledConnector(
        connectorId: connectorID,
        policy: policy.trust,
        revocations: policy.revocations,
        nowMs: Self.nowMs
      )
    }
  }

  func remove(
    record: InstalledConnectorRecord,
    policy: ConnectorReleasePolicy,
    completion: @escaping @Sendable (Result<Void, Error>) -> Void
  ) {
    perform(completion) { runtime in
      try runtime.removeInstalledConnector(
        connectorId: record.connectorId,
        version: record.version,
        mode: .quarantineState,
        policy: policy.trust,
        revocations: policy.revocations,
        nowMs: Self.nowMs
      )
    }
  }

  func enforce(
    policy: ConnectorReleasePolicy,
    completion: @escaping @Sendable (Result<[String], Error>) -> Void
  ) {
    perform(completion) {
      try $0.enforceConnectorTrust(
        policy: policy.trust, revocations: policy.revocations, nowMs: Self.nowMs)
    }
  }

  func openSession(
    checkpoint: ConnectorRestorationCheckpoint,
    policy: ConnectorReleasePolicy,
    completion: @escaping @Sendable (Result<Void, Error>) -> Void
  ) {
    perform(completion) { runtime in
      _ = try runtime.openConnectorSession(
        config: ConnectorSessionConfig(
          connectorId: checkpoint.connectorID,
          sessionId: checkpoint.sessionID,
          deviceId: 1,
          transportCapacity: 256,
          nowMs: Self.nowMs
        ),
        policy: policy.trust,
        revocations: policy.revocations
      )
    }
  }

  func apply(
    event: ConnectorTransportEvent,
    completion: @escaping @Sendable (Result<Void, Error>) -> Void
  ) {
    perform(completion) { runtime in
      _ = try runtime.applyConnectorEvent(event: event, wallTimeMs: Self.nowMs)
    }
  }

  func drain(
    completion: @escaping @Sendable (Result<[ConnectorTransportAction], Error>) -> Void
  ) {
    perform(completion) { try $0.drainConnectorActions(limit: 64) }
  }

  func captureCapabilities(
    completion: @escaping @Sendable (Result<[ConnectorCaptureCapability], Error>) -> Void
  ) {
    perform(completion) { try $0.connectorCaptureCapabilities() }
  }

  func startCapture(
    stream: String,
    completion: @escaping @Sendable (Result<Void, Error>) -> Void
  ) {
    perform(completion) { try $0.startConnectorCapture(stream: stream, nowMs: Self.nowMs) }
  }

  func stopCapture(
    stream: String,
    completion: @escaping @Sendable (Result<Void, Error>) -> Void
  ) {
    perform(completion) { try $0.stopConnectorCapture(stream: stream, nowMs: Self.nowMs) }
  }

  func ecgState(
    completion: @escaping @Sendable (Result<EcgCaptureReport?, Error>) -> Void
  ) {
    perform(completion) { try $0.ecgCaptureState(nowMs: Self.nowMs) }
  }

  func ecgInferenceRequest(
    completion: @escaping @Sendable (Result<EcgInferenceRequest?, Error>) -> Void
  ) {
    perform(completion) { try $0.ecgInferenceRequest() }
  }

  func submitEcgInference(
    captureID: UInt64,
    predictions: [EcgPrediction],
    modelSHA256: String,
    completion: @escaping @Sendable (Result<EcgResultReport, Error>) -> Void
  ) {
    perform(completion) {
      try $0.submitEcgInference(
        captureId: captureID,
        predictions: predictions,
        modelSha256: modelSHA256,
        nowMs: Self.nowMs
      )
    }
  }

  func ecgResults(
    deviceID: UInt64,
    limit: UInt32,
    completion: @escaping @Sendable (Result<[EcgResultReport], Error>) -> Void
  ) {
    perform(completion) { try $0.ecgResults(deviceId: deviceID, limit: limit) }
  }

  func ecgReportPayload(
    captureID: UInt64,
    completion: @escaping @Sendable (Result<EcgReportPayload?, Error>) -> Void
  ) {
    perform(completion) { try $0.ecgReportPayload(captureId: captureID) }
  }

  func deleteEcgCapture(
    captureID: UInt64,
    completion: @escaping @Sendable (Result<Bool, Error>) -> Void
  ) {
    perform(completion) { try $0.deleteEcgCapture(captureId: captureID) }
  }

  /// Hand the core the platform's own zone table. Rust holds no tzdata (ADR-024): iOS has a correct
  /// and updated one, and it is the only place the user's zone is genuinely known. Two years back
  /// and one forward covers every day the app can show plus the next transition.
  func publishTimezoneSpans(completion: @escaping @Sendable (Result<Void, Error>) -> Void) {
    let zone = TimeZone.current
    let day = 86_400.0
    let now = Date().timeIntervalSince1970
    var spans: [TimezoneSpan] = []
    var last: Int?
    var cursor = now - 730 * day
    while cursor <= now + 365 * day {
      let offset = zone.secondsFromGMT(for: Date(timeIntervalSince1970: cursor))
      if offset != last {
        spans.append(TimezoneSpan(startUnixSeconds: Int64(cursor), offsetSeconds: Int32(offset)))
        last = offset
      }
      cursor += day
    }
    if spans.isEmpty {
      spans = [TimezoneSpan(startUnixSeconds: 0, offsetSeconds: Int32(zone.secondsFromGMT()))]
    }
    let identifier = zone.identifier
    perform(completion) { try $0.setTimezoneSpans(timezoneId: identifier, spans: spans) }
  }

  /// One local day's analytics from the core.
  func dailySnapshot(
    deviceID: UInt64,
    completion: @escaping @Sendable (Result<DailySnapshotReport, Error>) -> Void
  ) {
    perform(completion) { try $0.dailySnapshot(deviceId: deviceID, wallTimeMs: Self.nowMs) }
  }

  /// A window of local days, oldest first. The trend and vitals surfaces read a range, and
  /// asking day by day would recompute the longitudinal look-back once per day rendered.
  func dailySnapshots(
    deviceID: UInt64,
    days: Int,
    completion: @escaping @Sendable (Result<[DailySnapshotReport], Error>) -> Void
  ) {
    let now = Self.nowMs
    let from = now - Int64(days) * 86_400_000
    perform(completion) {
      try $0.dailySnapshots(deviceId: deviceID, fromMs: from, toMs: now)
    }
  }

  func telemetry(
    completion: @escaping @Sendable (Result<ConnectorTelemetrySnapshot, Error>) -> Void
  ) {
    perform(completion) { try $0.connectorTelemetry(nowMs: Self.nowMs) }
  }

  func cancelSession(
    completion: @escaping @Sendable (Result<ConnectorTelemetrySnapshot, Error>) -> Void
  ) {
    perform(completion) { runtime in
      _ = try runtime.cancelConnectorSession(reason: .user, wallTimeMs: Self.nowMs)
      return try runtime.connectorTelemetry(nowMs: Self.nowMs)
    }
  }

  /// Trade data density for battery on both phone and strap (ADR-030). Fire-and-forget: the core
  /// keeps the setting for later sessions, so a failure here cannot desync the user's choice.
  func setLowPower(_ on: Bool) {
    queue.async { [weak self] in
      guard let self else { return }
      guard let runtime = try? (self.runtime ?? MavRuntime(config: self.config)) else { return }
      self.runtime = runtime
      _ = try? runtime.setLowPower(lowPower: on, nowMs: Self.nowMs)
    }
  }

  private func perform<T>(
    _ completion: @escaping @Sendable (Result<T, Error>) -> Void,
    operation: @escaping (MavRuntime) throws -> T
  ) {
    queue.async { [weak self] in
      guard let self else { return }
      do {
        let runtime = try self.runtime ?? MavRuntime(config: self.config)
        self.runtime = runtime
        completion(.success(try operation(runtime)))
      } catch { completion(.failure(error)) }
    }
  }

  private static var nowMs: Int64 { Int64(Date().timeIntervalSince1970 * 1_000) }
}
