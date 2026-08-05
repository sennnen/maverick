import SwiftUI

/// What the on-device models are doing, and why anything absent is absent.
///
/// The Android twin is `MavAnalyticsScreen.kt`. This is a product surface, not a diagnostics
/// screen: it is reachable, it is written for a wearer, and every line is a fact about their data
/// rather than about the build. It exists because most of the zoo's outputs have no consumer yet
/// — `docs/ml.md` withholds the sleep staging vocabulary and the hypertension risk level, and
/// most models have no ported front-end — and the honest answer to "what is this app doing with
/// forty-one models" is a screen that says so, per signal, in the wearer's terms.
///
/// Copy is draft and lives in `Localizable.strings`. Nothing here renders a model output as a
/// health reading; a signal whose vocabulary is not admitted says it was computed and stops.
struct MavAnalyticsView: View {
  @EnvironmentObject private var analytics: MavAnalyticsModel

  var body: some View {
    List {
      Section {
        if analytics.snapshot.signals.isEmpty {
          Text(
            analytics.snapshot.working
              ? "analytics.a11y.working"
              : "analytics.empty"
          )
          .font(.subheadline)
          .foregroundStyle(.secondary)
        }
        ForEach(analytics.snapshot.signals) { signal in
          SignalRow(signal: signal, onRetry: analytics.retry)
        }
      } header: {
        Text("analytics.title")
      } footer: {
        Text("analytics.subtitle")
      }
    }
    .navigationTitle(Text("analytics.title"))
  }
}

private struct SignalRow: View {
  let signal: MavSignal
  let onRetry: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 2) {
      HStack {
        Text(title).font(.body)
        Spacer()
        if case .working = signal.state { ProgressView() }
      }
      Text(summary).font(.caption).foregroundStyle(.secondary)
      Text(
        String(
          format: NSLocalizedString("analytics.coverage", comment: ""),
          signal.runnable,
          signal.total
        )
      )
      .font(.caption2)
      .foregroundStyle(.secondary)

      if case .failed(_, _, true) = signal.state {
        Button("analytics.retry", action: onRetry)
          .accessibilityLabel(
            Text(
              String(
                format: NSLocalizedString("analytics.a11y.retry", comment: ""),
                title
              )
            )
          )
      }
    }
    // One announcement per row. Without this VoiceOver reads the title, the state and the
    // coverage as three unrelated fragments and the wearer has to assemble them.
    .accessibilityElement(children: .combine)
    .accessibilityLabel(
      Text(
        String(
          format: NSLocalizedString("analytics.a11y.signal", comment: ""),
          title,
          summary
        )
      )
    )
  }

  private var title: String {
    signal.name.replacingOccurrences(of: "_", with: " ").capitalized
  }

  /// One line of copy for one state.
  private var summary: String {
    switch signal.state {
    case .idle:
      return NSLocalizedString("analytics.state.idle", comment: "")
    case let .working(done, total):
      return String(
        format: NSLocalizedString("analytics.state.working", comment: ""),
        done,
        total
      )
    case let .ready(_, displayable):
      // The model ran and its vocabulary is not admitted. Saying "up to date" here would imply
      // a reading exists to be up to date.
      return NSLocalizedString(
        displayable ? "analytics.state.ready" : "analytics.state.computedNotShown",
        comment: ""
      )
    case .stale:
      return NSLocalizedString("analytics.state.stale", comment: "")
    case .deferred:
      return NSLocalizedString("analytics.state.deferred", comment: "")
    case .failed:
      return NSLocalizedString("analytics.state.failed", comment: "")
    case let .permissionRequired(permission):
      return String(
        format: NSLocalizedString("analytics.permissionRequired", comment: ""),
        permission
      )
    case let .unavailable(reasons):
      guard let reason = reasons.first else {
        return NSLocalizedString("analytics.state.idle", comment: "")
      }
      return describe(reason)
    }
  }

  private func describe(_ reason: MavUnavailable) -> String {
    switch reason {
    case let .missingStreams(streams):
      return String(
        format: NSLocalizedString("analytics.needsSensor", comment: ""),
        streams.joined(separator: ", ")
      )
    case let .missingProfile(fields):
      return String(
        format: NSLocalizedString("analytics.needsProfile", comment: ""),
        fields.joined(separator: ", ")
      )
    case let .upstreamUnavailable(model):
      return String(format: NSLocalizedString("analytics.needsUpstream", comment: ""), model)
    case let .preprocessingNotPorted(detail):
      return String(format: NSLocalizedString("analytics.notPorted", comment: ""), detail)
    }
  }
}

/// The observable wrapper the views bind to.
///
/// The engine itself is deliberately not an `ObservableObject`: it runs off the main actor and
/// publishing from it directly would put `@Published` writes on a background queue. This class is
/// the main-actor edge, and the only thing it can do is start a pass — a view cannot reach in and
/// change what runs.
@MainActor
final class MavAnalyticsModel: ObservableObject {
  @Published private(set) var snapshot = MavAnalyticsSnapshot()

  private var engine: MavAnalyticsEngine?

  func attach(_ engine: MavAnalyticsEngine) {
    self.engine = engine
    engine.onChange { [weak self] next in
      Task { @MainActor in self?.snapshot = next }
    }
    MavBackgroundAnalytics.provider = { [weak engine] in engine }
  }

  /// Run an interactive pass. The wearer is looking at the screen, so this one may be expensive.
  func refresh() {
    engine?.runPass(deviceID: MavBackgroundAnalytics.deviceID, mode: .interactive) { _ in }
  }

  /// Clear the retry budgets and run again, for the retry affordance on a failed signal.
  func retry() {
    engine?.resetRetries()
    refresh()
  }
}
