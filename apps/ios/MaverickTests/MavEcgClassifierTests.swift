import CoreML
import CryptoKit
import Foundation
import PDFKit
import Testing
@testable import Mav

struct MavEcgClassifierTests {
  private struct Corpus: Decodable {
    let schema: String
    let sampleRateHz: Int
    let sampleCount: Int
    let classes: [String]
    let cases: [Case]

    struct Case: Decodable {
      let id: String
      let family: String
      let expected: String
      let coremlFp16: [Float]
    }
  }

  @Test func selectedPackageMembersMatchTheAdmittedHashes() throws {
    let package = repositoryRoot()
      .appendingPathComponent("apps/ios/Maverick/Models/nao_full_ecg_model_fp16.mlpackage")
    let expected = [
      "Manifest.json": "2760ca6f4696a0519091fa43ee9ddbfae1bbda4e61fb85a5438d2cb3317ab288",
      "Data/com.apple.CoreML/model.mlmodel":
        "24230f1c635ab45fba02bb80c86f3b9a9dc2499436bda16a1d97b34ffb63f6d3",
      "Data/com.apple.CoreML/weights/weight.bin":
        "24111a56f73dc262cf600a73f18a647bf8ad623ecaa7336da5463e87325de0d9",
    ]
    for (member, digest) in expected {
      let data = try Data(contentsOf: package.appendingPathComponent(member))
      #expect(SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() == digest)
    }
  }

  @Test func modelContractIsOneFloatTensorAndThreeFiniteProbabilities() throws {
    let classifier = try MavEcgClassifier(bundle: .main)
    #expect(classifier.inputShape == [1, 7_680, 1])
    #expect(classifier.outputShape == [1, 3])

    let probabilities = try classifier.predict(Array(repeating: 0, count: 7_680))
    #expect(probabilities.count == 3)
    #expect(probabilities.allSatisfy { $0.isFinite })
    #expect(abs(probabilities.reduce(0, +) - 1) < 0.001)
  }

  @Test func nineSyntheticCasesKeepTheirExpectedWinningClass() throws {
    let corpus = try loadCorpus()
    #expect(corpus.schema == "mav/ecg-model-corpus/v1")
    #expect(corpus.sampleRateHz == 256)
    #expect(corpus.sampleCount == 7_680)
    #expect(corpus.classes == ["N", "A", "O"])
    #expect(corpus.cases.count == 9)

    let classifier = try MavEcgClassifier(bundle: .main)
    for fixture in corpus.cases {
      let input = try normalizedSignal(id: fixture.id)
      let probabilities = try classifier.predict(input)
      #expect(probabilities.count == 3, "\(fixture.id)")
      #expect(probabilities.allSatisfy { $0.isFinite }, "\(fixture.id)")
      #expect(abs(probabilities.reduce(0, +) - 1) < 0.001, "\(fixture.id)")
      #expect(corpus.classes[probabilities.indices.max(by: {
        probabilities[$0] < probabilities[$1]
      })!] == fixture.expected, "\(fixture.id)")
      for index in probabilities.indices {
        #expect(abs(probabilities[index] - fixture.coremlFp16[index]) < 0.03, "\(fixture.id)")
      }
    }
  }

  @Test func batchPredictionsReturnInRequestOrder() throws {
    let corpus = try loadCorpus()
    let selected = [corpus.cases[0], corpus.cases[3], corpus.cases[6]]
    let classifier = try MavEcgClassifier(bundle: .main)
    let inputs = try selected.map { try normalizedSignal(id: $0.id) }
    let outputs = try classifier.predictBatch(inputs)
    #expect(outputs.count == selected.count)
    for (output, fixture) in zip(outputs, selected) {
      let winner = output.indices.max(by: { output[$0] < output[$1] })!
      #expect(corpus.classes[winner] == fixture.expected)
    }
  }

  @Test func everyCoreMLFixtureProducesAReadableOnePageNativePDF() throws {
    let corpus = try loadCorpus()
    let classifier = try MavEcgClassifier(bundle: .main)
    let directory = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
      .appendingPathComponent("MaverickECGReports/CoreML", isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)

    for (index, fixture) in corpus.cases.enumerated() {
      let modelInput = try normalizedSignal(id: fixture.id)
      let waveform = try millivoltSignal(id: fixture.id)
      let probabilities = try classifier.predict(modelInput)
      let winner = probabilities.indices.max(by: { probabilities[$0] < probabilities[$1] })!
      let report = MavEcgReportContent(
        captureID: UInt64(index + 1),
        recordedAt: Date(timeIntervalSince1970: 1_752_600_000 + Double(index * 60)),
        rhythm: rhythm(corpus.classes[winner]),
        probabilities: probabilities,
        confidence: confidence(probabilities),
        quality: 0.94,
        sampleRateHz: corpus.sampleRateHz,
        sampleCount: waveform.count,
        sourceUnit: "millivolts",
        waveform: waveform,
        explanation: explanation(modelInput),
        modelSHA256: MavEcgClassifier.admittedModelSHA256,
        preprocessingSHA256:
          "793dddb8f59e71d8a9b24cbd03e02efe0b361879027cf525a2a3dd6435edff24",
        algorithmVersion: "2.0.0",
        provisional: true
      )
      let data = MavEcgPDFRenderer.render(report)
      #expect(data.starts(with: Data("%PDF".utf8)), "\(fixture.id)")
      let document = PDFDocument(data: data)
      #expect(document?.pageCount == 1, "\(fixture.id)")
      let text = document?.page(at: 0)?.string ?? ""
      #expect(
        text.replacingOccurrences(of: " ", with: "").contains("MAVERICK"),
        "\(fixture.id)"
      )
      #expect(text.contains(rhythmTitle(corpus.classes[winner])), "\(fixture.id)")
      try data.write(
        to: directory.appendingPathComponent("\(fixture.id)_coreml.pdf"),
        options: .atomic
      )
    }

    #expect(
      try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
        .filter { $0.pathExtension == "pdf" }.count == 9
    )
  }

  private func loadCorpus() throws -> Corpus {
    let url = repositoryRoot().appendingPathComponent("fixtures/ecg/corpus/manifest.json")
    let decoder = JSONDecoder()
    decoder.keyDecodingStrategy = .convertFromSnakeCase
    return try decoder.decode(Corpus.self, from: Data(contentsOf: url))
  }

  private func normalizedSignal(id: String) throws -> [Float] {
    try signalColumn(id: id, column: 2)
  }

  private func millivoltSignal(id: String) throws -> [Float] {
    try signalColumn(id: id, column: 1)
  }

  private func signalColumn(id: String, column: Int) throws -> [Float] {
    let url = repositoryRoot().appendingPathComponent("fixtures/ecg/corpus/\(id).csv")
    let lines = try String(contentsOf: url, encoding: .utf8)
      .split(whereSeparator: \.isNewline)
    guard lines.count == 7_681 else {
      throw CocoaError(.fileReadCorruptFile)
    }
    return try lines.dropFirst().map { line in
      let columns = line.split(separator: ",", omittingEmptySubsequences: false)
      guard columns.count == 3, let value = Float(columns[column]) else {
        throw CocoaError(.fileReadCorruptFile)
      }
      return value
    }
  }

  private func rhythm(_ code: String) -> String {
    switch code {
    case "N": "sinus_rhythm"
    case "A": "atrial_fibrillation"
    default: "other_abnormal_rhythm"
    }
  }

  private func rhythmTitle(_ code: String) -> String {
    switch code {
    case "N": "Sinus rhythm"
    case "A": "Atrial fibrillation"
    default: "Other rhythm"
    }
  }

  private func confidence(_ values: [Float]) -> Float {
    let ordered = values.sorted(by: >)
    guard ordered.count >= 2 else { return 0 }
    return min(max((ordered[0] - ordered[1]) / 0.2, 0), 1)
  }

  private func explanation(_ waveform: [Float]) -> [MavEcgReportContent.Segment] {
    let segmentSize = waveform.count / 6
    let energies = (0..<6).map { segment -> Float in
      let values = waveform[(segment * segmentSize)..<((segment + 1) * segmentSize)]
      return values.reduce(0) { $0 + abs($1) } / Float(values.count)
    }
    let maximum = energies.max() ?? 1
    return energies.enumerated().map { index, energy in
      MavEcgReportContent.Segment(
        startSecond: index * 5,
        endSecond: (index + 1) * 5,
        importance: maximum > 0 ? energy / maximum : 0
      )
    }
  }

  private func repositoryRoot() -> URL {
    URL(fileURLWithPath: #filePath)
      .deletingLastPathComponent()
      .deletingLastPathComponent()
      .deletingLastPathComponent()
      .deletingLastPathComponent()
  }
}
