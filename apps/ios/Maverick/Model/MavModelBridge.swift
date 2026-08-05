import Foundation

/// Drives the core's inference queue: pull a request, run it, hand the result back.
///
/// The core decides what to infer and what the numbers mean; the platform decides when. That
/// split is why this is a drain loop the app calls rather than a callback the core invokes —
/// only the app knows whether it is foregrounded and whether the accelerator is free.
///
/// The loop is bounded per pass. A recompute can queue a night of PPG windows, and running all
/// of them in one turn would block whatever called us; `drain` takes as many as it was asked for
/// and leaves the rest for the next pass.
struct MavModelBridge {
  /// Anything that can hand out work and take results. `MavRuntime` satisfies it; tests
  /// substitute a queue they control, so the loop is exercised without a compiled model.
  protocol Host {
    func nextModelInference() throws -> ModelInferenceRequest?
    func submitModelInference(
      requestId: UInt64,
      outputs: [ModelTensor],
      modelSha256: String,
      completedAtMs: Int64
    ) throws -> ModelInferenceResult
    func cancelModelInference(requestId: UInt64) throws -> Bool
  }

  /// Anything that can run one model. `MavModelRunner` satisfies it.
  protocol Runner {
    func run(slug: String, inputs: [String: [Float]]) throws -> [String: [Float]]
    func admittedSHA256(for slug: String) throws -> String
    /// Drop every model held resident. Cheap to call; a runner holding nothing does nothing.
    func releaseCache()
  }

  /// What one drain pass did. Failures are counted, not thrown: one model missing from the
  /// bundle must not stop the others from running.
  struct Outcome: Equatable {
    var completed: Int = 0
    var failed: Int = 0
  }

  let host: Host
  let runner: Runner
  /// The platform's clock. The core reads none of its own — the same rule that keeps day
  /// boundaries reproducible in `recompute` — so the one timestamp worth remembering about an
  /// inference has to travel with it.
  var clock: () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) }

  @discardableResult
  func drain(limit: Int = 8) -> Outcome {
    var outcome = Outcome()
    for _ in 0..<max(0, limit) {
      // `try?` on a throwing call that already returns an optional collapses two layers of
      // optionality; spell the flattening out rather than depend on which way it collapses.
      guard let request = (try? host.nextModelInference()) ?? nil else { break }
      do {
        var inputs: [String: [Float]] = [:]
        for tensor in request.inputs {
          inputs[tensor.name] = tensor.values
        }
        let produced = try runner.run(slug: request.modelSlug, inputs: inputs)
        let outputs = produced.map { ModelTensor(name: $0.key, values: $0.value) }
        _ = try host.submitModelInference(
          requestId: request.requestId,
          outputs: outputs,
          modelSha256: try runner.admittedSHA256(for: request.modelSlug),
          completedAtMs: clock()
        )
        outcome.completed += 1
      } catch {
        // The request stays in flight inside core until it is cancelled, so a transient
        // failure could be retried; a missing or mismatched model will not fix itself, and
        // leaving it queued would stall every later inference behind it.
        _ = try? host.cancelModelInference(requestId: request.requestId)
        outcome.failed += 1
      }
    }
    return outcome
  }
}

extension MavRuntime: MavModelBridge.Host {}
extension MavModelRunner: MavModelBridge.Runner {}
