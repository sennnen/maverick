import CoreML
import Foundation

enum MavEcgClassifierError: Error, Equatable, LocalizedError {
  case modelMissing
  case invalidInputCount(Int)
  case invalidModelContract
  case nonFiniteOutput

  var errorDescription: String? {
    switch self {
    case .modelMissing: "The ECG model is missing from this build."
    case let .invalidInputCount(count): "The ECG tensor has \(count) values; expected 7,680."
    case .invalidModelContract: "The ECG model does not match Maverick's admitted tensor contract."
    case .nonFiniteOutput: "The ECG model returned an invalid value."
    }
  }
}

/// Thin native inference boundary. Filtering, resampling, labels, confidence and XAI stay in core.
final class MavEcgClassifier {
  static let modelName = "nao_full_ecg_model_fp16"
  static let admittedModelSHA256 =
    "24230f1c635ab45fba02bb80c86f3b9a9dc2499436bda16a1d97b34ffb63f6d3"
  static let expectedInputShape = [1, 7_680, 1]
  static let expectedOutputShape = [1, 3]

  let inputShape: [Int]
  let outputShape: [Int]
  private let model: MLModel

  init(bundle: Bundle = .main) throws {
    guard let modelURL = bundle.url(forResource: Self.modelName, withExtension: "mlmodelc") else {
      throw MavEcgClassifierError.modelMissing
    }
    let configuration = MLModelConfiguration()
    configuration.computeUnits = .all
    model = try MLModel(contentsOf: modelURL, configuration: configuration)

    inputShape = Self.shape(
      model.modelDescription.inputDescriptionsByName["ecg"]?.multiArrayConstraint
    )
    outputShape = Self.shape(
      model.modelDescription.outputDescriptionsByName["probabilities"]?.multiArrayConstraint
    )
    guard inputShape == Self.expectedInputShape, outputShape == Self.expectedOutputShape else {
      throw MavEcgClassifierError.invalidModelContract
    }
  }

  func predict(_ tensor: [Float]) throws -> [Float] {
    guard tensor.count == Self.expectedInputShape[1] else {
      throw MavEcgClassifierError.invalidInputCount(tensor.count)
    }
    let input = try MLMultiArray(
      shape: Self.expectedInputShape.map(NSNumber.init(value:)),
      dataType: .float32
    )
    let pointer = input.dataPointer.bindMemory(to: Float32.self, capacity: tensor.count)
    tensor.withUnsafeBufferPointer { source in
      guard let baseAddress = source.baseAddress else { return }
      pointer.update(from: baseAddress, count: tensor.count)
    }

    let provider = try MLDictionaryFeatureProvider(
      dictionary: ["ecg": MLFeatureValue(multiArray: input)]
    )
    let prediction = try model.prediction(from: provider)
    guard
      let output = prediction.featureValue(for: "probabilities")?.multiArrayValue,
      output.count == Self.expectedOutputShape[1]
    else {
      throw MavEcgClassifierError.invalidModelContract
    }
    let values = (0..<output.count).map { output[$0].floatValue }
    guard values.allSatisfy(\.isFinite) else {
      throw MavEcgClassifierError.nonFiniteOutput
    }
    return values
  }

  /// Occlusion-XAI asks for one baseline plus bounded masked tensors. Sequential execution keeps
  /// the returned values in core-request order and avoids concurrent access to one MLModel.
  func predictBatch(_ tensors: [[Float]]) throws -> [[Float]] {
    try tensors.map(predict)
  }

  private static func shape(_ constraint: MLMultiArrayConstraint?) -> [Int] {
    constraint?.shape.map(\.intValue) ?? []
  }
}
