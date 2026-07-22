import Foundation

@MainActor
final class MavStore: ObservableObject {
  enum State: Equatable { case opening, ready, failed(String) }

  @Published private(set) var state: State = .opening
  /// The core's `historical-status/v1` progress as a display line; nil when idle or unknown.
  @Published private(set) var syncProgress: String?
  private let worker = MavRuntimeWorker()
  private var inFlight = false

  init() { refresh() }

  func refresh() {
    guard !inFlight else { return }
    inFlight = true
    // Keep the last good snapshot on screen during a re-read; opening only before the first.
    if case .ready = state {} else { state = .opening }
    worker.refresh(config: Self.runtimeConfig()) { [weak self] result in
      Task { @MainActor in
        guard let self else { return }
        self.inFlight = false
        switch result {
        case .success:
          self.syncProgress = nil
          self.state = .ready
        case let .failure(message): self.state = .failed(message)
        }
      }
    }
  }

  func retry() { refresh() }

  /// The core store's on-disk location — shared with the diagnostics size readout.
  nonisolated static func databaseURL() -> URL {
    let root = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
      .appendingPathComponent("Maverick", isDirectory: true)
    try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    return root.appendingPathComponent("mav.sqlite")
  }

  nonisolated static func runtimeConfig() -> RuntimeConfig {
    return RuntimeConfig(
      databasePath: databaseURL().path,
      timezoneId: TimeZone.current.identifier,
      appVersion: Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0.1.0"
    )
  }
}

private final class MavRuntimeWorker: @unchecked Sendable {
  private let queue = DispatchQueue(label: "com.sennnen.mav.runtime", qos: .userInitiated)
  private var runtime: MavRuntime?

  enum Output: Sendable { case success; case failure(String) }

  func refresh(config: RuntimeConfig, completion: @escaping @Sendable (Output) -> Void) {
    queue.async { [weak self] in
      guard let self else { return }
      do {
        let runtime = try self.runtime ?? MavRuntime(config: config)
        self.runtime = runtime
        completion(.success)
      } catch {
        completion(.failure((error as? LocalizedError)?.errorDescription ?? error.localizedDescription))
      }
    }
  }
}
