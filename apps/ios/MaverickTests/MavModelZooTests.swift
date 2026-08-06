import CoreML
import CryptoKit
import Foundation
import Testing
@testable import Mav

/// The iOS half of the model-zoo proof.
///
/// Three separate claims, kept separate because they fail for different reasons: the packages in
/// the repository are the packages the manifest admitted, the generated catalogue agrees with
/// that manifest, and every model in the catalogue actually loads and runs at its contracted
/// shape. The drain loop is checked against a substituted host, so a broken model is exercised
/// without needing one.
struct MavModelZooTests {
  private struct Manifest: Decodable {
    struct TensorSpec: Decodable {
      let name: String
      let shape: [Int]
      let dtype: String
    }

    struct CoreML: Decodable {
      let sha256: String
      let members: [String: String]
      let bytes: Int
    }

    struct Model: Decodable {
      let model: String
      let algorithmId: String
      let algorithmVersion: String
      let inputs: [TensorSpec]
      let outputs: [TensorSpec]
      let coreml: CoreML

      private enum CodingKeys: String, CodingKey {
        case model
        case algorithmId = "algorithm_id"
        case algorithmVersion = "algorithm_version"
        case inputs, outputs, coreml
      }
    }

    let models: [Model]
  }

  /// The checkout this test file lives in.
  ///
  /// Four levels up from `MaverickTests/` is the repository root. The same helper exists in
  /// `MavEcgClassifierTests`; both are private because a shared one would be a third file to
  /// keep in step for four lines of path arithmetic.
  private func repositoryRoot() -> URL {
    URL(fileURLWithPath: #filePath)
      .deletingLastPathComponent()
      .deletingLastPathComponent()
      .deletingLastPathComponent()
      .deletingLastPathComponent()
  }

  private func manifest() throws -> Manifest {
    let url = repositoryRoot().appendingPathComponent("artifacts/models/manifest.json")
    return try JSONDecoder().decode(Manifest.self, from: Data(contentsOf: url))
  }

  @Test func everyAdmittedPackageMemberMatchesItsManifestHash() throws {
    for model in try manifest().models {
      let package = repositoryRoot()
        .appendingPathComponent("apps/ios/Maverick/Models/\(model.model).mlpackage")
      #expect(FileManager.default.fileExists(atPath: package.path), "\(model.model) is not bundled")
      for (member, digest) in model.coreml.members {
        let data = try Data(contentsOf: package.appendingPathComponent(member))
        let actual = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        #expect(actual == digest, "\(model.model)/\(member) has an unadmitted hash")
      }
    }
  }

  @Test func theGeneratedCatalogueMatchesTheManifest() throws {
    let models = try manifest().models
    #expect(models.count == MavModelCatalog.entries.count)
    for model in models {
      let entry = try #require(MavModelCatalog.entries[model.model])
      #expect(entry.admittedSHA256 == model.coreml.sha256)
      #expect(entry.inputs.map(\.name) == model.inputs.map(\.name))
      #expect(entry.inputs.map(\.shape) == model.inputs.map(\.shape))
      #expect(entry.outputs.map(\.name) == model.outputs.map(\.name))
      #expect(entry.outputs.map(\.shape) == model.outputs.map(\.shape))
    }
  }

  @Test func everyBundledModelLoadsAndRunsAtItsContractedShape() throws {
    let runner = MavModelRunner(bundle: .main)
    for slug in MavModelCatalog.slugs {
      let entry = try #require(MavModelCatalog.entries[slug])
      var inputs: [String: [Float]] = [:]
      for spec in entry.inputs {
        // Zero is a legitimate tensor for every contract here, and it makes the assertion about
        // shape and finiteness rather than about any particular signal.
        inputs[spec.name] = Array(repeating: 0, count: spec.elementCount)
      }
      let outputs = try runner.run(slug: slug, inputs: inputs)
      #expect(outputs.count == entry.outputs.count, "\(slug) returned the wrong tensor count")
      for spec in entry.outputs {
        let values = try #require(outputs[spec.name], "\(slug) omitted \(spec.name)")
        #expect(values.count == spec.elementCount)
        // Hoisted out of the macro: `allSatisfy` is `rethrows`, and the expansion calls it from
        // a context that cannot handle a throw even when the closure demonstrably cannot.
        let allFinite = values.allSatisfy(\.isFinite)
        #expect(allFinite, "\(slug) returned a non-finite \(spec.name)")
      }
    }
  }

  @Test func aShortInputTensorIsRefusedBeforeTheModelRuns() throws {
    let runner = MavModelRunner(bundle: .main)
    let slug = try #require(MavModelCatalog.slugs.first)
    let entry = try #require(MavModelCatalog.entries[slug])
    var inputs: [String: [Float]] = [:]
    for spec in entry.inputs {
      inputs[spec.name] = Array(repeating: 0, count: spec.elementCount - 1)
    }
    // do/catch rather than `#expect(throws:)`: the macro's expansion calls the closure from a
    // non-throwing context under this toolchain, and spelling the catch out says exactly which
    // error was wanted anyway.
    do {
      _ = try runner.run(slug: slug, inputs: inputs)
      Issue.record("\(slug) ran with a short input tensor")
    } catch let error as MavModelRunnerError {
      guard case .contractMismatch = error else {
        Issue.record("expected a contract mismatch, got \(error)")
        return
      }
    }
  }

  /// The cache is bounded.
  ///
  /// It was not: a pass over the zoo left every model it touched resident, and a compiled Core ML
  /// model costs far more than its package does on disk. Runs more models than the budget so the
  /// assertion is about eviction happening, not about the budget being generous.
  @Test func loadingMoreModelsThanTheBudgetEvictsTheIdleOnes() throws {
    let runner = MavModelRunner(bundle: .main)
    let slugs = Array(MavModelCatalog.slugs.prefix(MavModelRunner.maxResident + 2))
    try #require(slugs.count > MavModelRunner.maxResident)
    for slug in slugs {
      _ = try runner.model(for: slug)
    }
    #expect(runner.residentCount <= MavModelRunner.maxResident)
    // The one asked for last is the one still there and the one asked for first is gone; an LRU
    // that evicted the newest would make the cache a treadmill. Asked of the cache rather than
    // through `model(for:)`, which would reload an evicted model and report it as present.
    #expect(runner.isResident(try #require(slugs.last)))
    #expect(!runner.isResident(try #require(slugs.first)))

    runner.releaseCache()
    #expect(runner.residentCount == 0)
  }

  @Test func anUnknownSlugIsRefused() throws {
    let runner = MavModelRunner(bundle: .main)
    do {
      _ = try runner.run(slug: "not_a_model", inputs: [:])
      Issue.record("an unknown slug was accepted")
    } catch let error as MavModelRunnerError {
      #expect(error == .unknownModel("not_a_model"))
    }
  }

  // MARK: - The drain loop

  private final class FakeHost: MavModelBridge.Host {
    var queue: [ModelInferenceRequest]
    var submitted: [UInt64] = []
    var cancelled: [UInt64] = []

    init(queue: [ModelInferenceRequest]) {
      self.queue = queue
    }

    func nextModelInference() throws -> ModelInferenceRequest? {
      queue.isEmpty ? nil : queue.removeFirst()
    }

    func submitModelInference(
      requestId: UInt64,
      outputs: [ModelTensor],
      modelSha256: String,
      completedAtMs: Int64
    ) throws -> ModelInferenceResult {
      submitted.append(requestId)
      return ModelInferenceResult(
        requestId: requestId,
        modelSlug: "good_model",
        outputs: outputs,
        modelSha256: modelSha256
      )
    }

    func cancelModelInference(requestId: UInt64) throws -> Bool {
      cancelled.append(requestId)
      return true
    }
  }

  private struct FakeRunner: MavModelBridge.Runner {
    func run(slug: String, inputs: [String: [Float]]) throws -> [String: [Float]] {
      if slug == "bad_model" { throw MavModelRunnerError.modelMissing(slug) }
      return ["embeddings": [0.5]]
    }

    func admittedSHA256(for slug: String) throws -> String { String(repeating: "a", count: 64) }

    func releaseCache() {}
  }

  @Test func theDrainLoopCompletesWhatItCanAndCancelsWhatItCannot() {
    let host = FakeHost(queue: [
      ModelInferenceRequest(
        requestId: 1,
        modelSlug: "good_model",
        inputs: [ModelTensor(name: "ppg", values: [0, 1])]
      ),
      ModelInferenceRequest(
        requestId: 2,
        modelSlug: "bad_model",
        inputs: [ModelTensor(name: "ppg", values: [0, 1])]
      ),
    ])
    let outcome = MavModelBridge(host: host, runner: FakeRunner()).drain()
    #expect(outcome.completed == 1)
    #expect(outcome.failed == 1)
    #expect(host.submitted == [1])
    #expect(host.cancelled == [2])
  }

  @Test func theDrainLoopStopsAtItsLimit() {
    let host = FakeHost(queue: (1...10).map { index in
      ModelInferenceRequest(
        requestId: UInt64(index),
        modelSlug: "good_model",
        inputs: [ModelTensor(name: "ppg", values: [0])]
      )
    })
    let outcome = MavModelBridge(host: host, runner: FakeRunner()).drain(limit: 3)
    #expect(outcome.completed == 3)
    #expect(host.queue.count == 7)
  }
}
