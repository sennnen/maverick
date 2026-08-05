import CoreML
import Foundation

enum MavModelRunnerError: Error, Equatable, LocalizedError {
  case unknownModel(String)
  case modelMissing(String)
  case contractMismatch(model: String, detail: String)
  case missingInput(model: String, tensor: String)
  case nonFiniteOutput(model: String, tensor: String)

  var errorDescription: String? {
    switch self {
    case let .unknownModel(slug):
      "This build ships no model named \(slug)."
    case let .modelMissing(slug):
      "The model \(slug) is missing from this build."
    case let .contractMismatch(model, detail):
      "The model \(model) does not match Maverick's admitted tensor contract: \(detail)"
    case let .missingInput(model, tensor):
      "The inference for \(model) is missing its \(tensor) tensor."
    case let .nonFiniteOutput(model, tensor):
      "The model \(model) returned an invalid value in \(tensor)."
    }
  }
}

/// Runs one Core ML model from the zoo, and nothing else.
///
/// Every judgement — which model, what the tensor means, whether a prediction may be believed —
/// stays in the shared core. This type binds named `Float` buffers to an `MLModel`, asserts the
/// shapes the generated catalogue declares, and hands the numbers straight back.
///
/// Models are compiled by Xcode from the `.mlpackage` sources under `Maverick/Models`, so at
/// runtime each is an `.mlmodelc` in the bundle. The compiled form has no stable hash of the
/// source package, which is why `MavModelCatalog` carries the admitted SHA-256 as a generated
/// constant and `tools/check_model_assets.py` proves the shipped package matches it. The runner
/// reports that constant with each result; core refuses anything it does not admit.
final class MavModelRunner {
  /// How many models may stay loaded at once.
  ///
  /// A count rather than a byte budget, which is what the Android twin uses: `Debug`'s native
  /// heap figure gives that runner a measured per-model cost, and Core ML offers no equivalent —
  /// a compiled model's residency is inside the framework and the package size on disk
  /// understates it badly. So the bound here is the one honest number available, and it is small
  /// because the models that chain together in a pass are few: an encoder and the heads that
  /// read it, which is three.
  ///
  /// It is a bound on *idle* models. The one being run is never evicted, so a single model
  /// larger than the whole budget still runs.
  static let maxResident = 4

  private let bundle: Bundle
  private var loaded: [String: MLModel] = [:]
  /// Least-recently-used first. Kept beside `loaded` rather than as a timestamp on each entry so
  /// eviction is a `removeFirst` instead of a scan.
  private var recency: [String] = []
  private let lock = NSLock()

  init(bundle: Bundle = .main) {
    self.bundle = bundle
  }

  /// Load and contract-check one model. Cached: an `MLModel` is expensive to build and the same
  /// model is asked for once per window during a recompute.
  func model(for slug: String) throws -> MLModel {
    guard let entry = MavModelCatalog.entries[slug] else {
      throw MavModelRunnerError.unknownModel(slug)
    }
    lock.lock()
    defer { lock.unlock() }
    if let cached = loaded[slug] {
      touch(slug)
      return cached
    }
    guard let url = bundle.url(forResource: slug, withExtension: "mlmodelc") else {
      throw MavModelRunnerError.modelMissing(slug)
    }
    let configuration = MLModelConfiguration()
    // The configuration this model's parity was measured under, not a blanket `.all`.
    // Core ML's backends disagree on some graphs — the sleep models are exact on the CPU and
    // the Neural Engine and wrong by more than a whole relative unit on the GPU — so the
    // catalogue records which units each model was admitted for and this honours it.
    configuration.computeUnits = entry.computeUnits.mlComputeUnits
    let model = try MLModel(contentsOf: url, configuration: configuration)
    try assertContract(model: model, entry: entry)
    loaded[slug] = model
    touch(slug)
    evictIdle(keeping: slug)
    return model
  }

  /// How many models are resident. Read by the tests that prove the bound holds.
  var residentCount: Int {
    lock.lock()
    defer { lock.unlock() }
    return loaded.count
  }

  /// Move one model to the most-recently-used end. Caller holds `lock`.
  private func touch(_ slug: String) {
    recency.removeAll { $0 == slug }
    recency.append(slug)
  }

  /// Drop least-recently-used models until the resident count is back within budget. Caller
  /// holds `lock`.
  private func evictIdle(keeping: String) {
    while loaded.count > Self.maxResident {
      guard let oldest = recency.first(where: { $0 != keeping }) else { return }
      recency.removeAll { $0 == oldest }
      loaded.removeValue(forKey: oldest)
    }
  }

  /// Run one inference. Inputs are keyed by the contract's tensor names; outputs come back the
  /// same way, flattened row-major in the declared shape.
  func run(slug: String, inputs: [String: [Float]]) throws -> [String: [Float]] {
    guard let entry = MavModelCatalog.entries[slug] else {
      throw MavModelRunnerError.unknownModel(slug)
    }
    let model = try self.model(for: slug)

    var features: [String: MLFeatureValue] = [:]
    for spec in entry.inputs {
      guard let values = inputs[spec.name] else {
        throw MavModelRunnerError.missingInput(model: slug, tensor: spec.name)
      }
      guard values.count == spec.elementCount else {
        throw MavModelRunnerError.contractMismatch(
          model: slug,
          detail: "\(spec.name) has \(values.count) values, expected \(spec.elementCount)"
        )
      }
      // The model's own declared element type, not the contract's — see `array`.
      let declared = model.modelDescription
        .inputDescriptionsByName[spec.name]?
        .multiArrayConstraint?
        .dataType ?? .float32
      features[spec.name] = MLFeatureValue(
        multiArray: try Self.array(values, shape: spec.shape, dataType: declared)
      )
    }

    let prediction = try model.prediction(from: MLDictionaryFeatureProvider(dictionary: features))

    var outputs: [String: [Float]] = [:]
    for spec in entry.outputs {
      guard let array = prediction.featureValue(for: spec.name)?.multiArrayValue else {
        throw MavModelRunnerError.contractMismatch(
          model: slug,
          detail: "no output named \(spec.name)"
        )
      }
      guard array.count == spec.elementCount else {
        throw MavModelRunnerError.contractMismatch(
          model: slug,
          detail: "\(spec.name) returned \(array.count) values, expected \(spec.elementCount)"
        )
      }
      let values = Self.floats(array)
      guard values.allSatisfy(\.isFinite) else {
        throw MavModelRunnerError.nonFiniteOutput(model: slug, tensor: spec.name)
      }
      outputs[spec.name] = values
    }
    return outputs
  }

  /// The hash core admits for this model on this platform.
  func admittedSHA256(for slug: String) throws -> String {
    guard let entry = MavModelCatalog.entries[slug] else {
      throw MavModelRunnerError.unknownModel(slug)
    }
    return entry.admittedSHA256
  }

  /// Release every cached model.
  ///
  /// Called when the app leaves the foreground and when the system warns about memory —
  /// `MavAnalyticsModel.releaseResources` is the one caller, behind the engine's queue so this
  /// never lands on a model mid-inference. A compiled Core ML model holds far more resident than
  /// its package costs on disk, and Pulse-PPG alone is 55 MB of that disk.
  func releaseCache() {
    lock.lock()
    loaded.removeAll()
    recency.removeAll()
    lock.unlock()
  }

  private func assertContract(model: MLModel, entry: MavModelCatalog.Entry) throws {
    let description = model.modelDescription
    for spec in entry.inputs {
      guard let constraint = description.inputDescriptionsByName[spec.name]?.multiArrayConstraint
      else {
        throw MavModelRunnerError.contractMismatch(
          model: entry.slug,
          detail: "no input named \(spec.name)"
        )
      }
      let shape = constraint.shape.map(\.intValue)
      guard shape.reduce(1, *) == spec.elementCount else {
        throw MavModelRunnerError.contractMismatch(
          model: entry.slug,
          detail: "input \(spec.name) is \(shape), expected \(spec.shape)"
        )
      }
    }
    for spec in entry.outputs {
      guard description.outputDescriptionsByName[spec.name] != nil else {
        throw MavModelRunnerError.contractMismatch(
          model: entry.slug,
          detail: "no output named \(spec.name)"
        )
      }
    }
  }

  /// Build the input array in the element type the model declares.
  ///
  /// Values cross the FFI as whole-numbered floats regardless of the contract's dtype, but
  /// Core ML is strict about the array it is handed: feeding a float32 array to a model whose
  /// input is INT32 is rejected outright. The behaviour-id lookup is that case, so the
  /// declared type decides, not the wire type.
  private static func array(
    _ values: [Float],
    shape: [Int],
    dataType: MLMultiArrayDataType
  ) throws -> MLMultiArray {
    let array = try MLMultiArray(shape: shape.map(NSNumber.init(value:)), dataType: dataType)
    switch dataType {
    case .float32:
      let pointer = array.dataPointer.bindMemory(to: Float32.self, capacity: values.count)
      values.withUnsafeBufferPointer { source in
        guard let base = source.baseAddress else { return }
        pointer.update(from: base, count: values.count)
      }
    case .int32:
      let pointer = array.dataPointer.bindMemory(to: Int32.self, capacity: values.count)
      for (index, value) in values.enumerated() {
        pointer[index] = Int32(value.rounded())
      }
    default:
      for (index, value) in values.enumerated() {
        array[index] = NSNumber(value: value)
      }
    }
    return array
  }

  private static func floats(_ array: MLMultiArray) -> [Float] {
    if array.dataType == .float32 {
      let pointer = array.dataPointer.bindMemory(to: Float32.self, capacity: array.count)
      return Array(UnsafeBufferPointer(start: pointer, count: array.count))
    }
    return (0..<array.count).map { array[$0].floatValue }
  }
}
