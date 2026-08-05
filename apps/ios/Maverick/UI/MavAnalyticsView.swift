import SwiftUI

/// What the on-device models are doing, and why anything absent is absent.
///
/// The Android twin is `MavAnalyticsScreen.kt`. This is a product surface, not a diagnostics
/// screen: it is reached from Today's "More" list beside the report, it is written for a wearer,
/// and every line is a fact about their data rather than about the build. It exists because most
/// of the zoo's outputs have no consumer yet — `docs/ml.md` withholds the sleep staging vocabulary
/// and the hypertension risk level, and most models have no ported front-end — and the honest
/// answer to "what is this app doing with all those models" is a screen that says so, per signal,
/// in the wearer's terms.
///
/// It is built from `MavKit` rather than from a plain `List` for the same reason every other
/// pushed screen is: a signal that cannot run renders as `MavUnavailableCard`, the same dashed
/// card an unavailable metric gets everywhere else in the app, so absence looks the same wherever
/// a reader meets it.
///
/// Copy lives in `Localizable.strings`. Nothing here renders a model output as a health reading;
/// a signal whose vocabulary is not admitted says it was computed and stops.
struct MavAnalyticsView: View {
  @EnvironmentObject private var analytics: MavAnalyticsModel

  var body: some View {
    MavDetailScaffold(title: NSLocalizedString("analytics.title", comment: "")) {
      MavTile {
        Text("analytics.subtitle")
          .mavType(.body)
          .foregroundStyle(MavTheme.inkSecondary)
          .fixedSize(horizontal: false, vertical: true)
      }

      if analytics.snapshot.signals.isEmpty {
        MavUnavailableCard(
          name: NSLocalizedString("analytics.title", comment: ""),
          reason: NSLocalizedString(
            analytics.snapshot.working ? "analytics.a11y.working" : "analytics.empty",
            comment: ""
          )
        )
      } else {
        ForEach(analytics.snapshot.signals) { signal in
          SignalCard(signal: signal, onRetry: analytics.retry)
        }
      }
    }
    // The wearer pulling down is the clearest possible "do it again", and it is the gesture they
    // already use on every other scrolling surface in the app.
    .refreshable { await analytics.refreshAndWait() }
  }
}

/// One signal, as a card.
///
/// Two shapes rather than one: a signal nothing can run is an *absence* and gets the dashed
/// unavailable card, which is how absence is drawn everywhere else. Anything else is a live card
/// carrying its state and its coverage.
private struct SignalCard: View {
  let signal: MavSignal
  let onRetry: () -> Void

  var body: some View {
    if case let .unavailable(reasons) = signal.state {
      MavUnavailableCard(name: title, reason: MavSignalCopy.describe(reasons))
    } else {
      MavStatusCard {
        VStack(alignment: .leading, spacing: 6) {
          HStack(alignment: .firstTextBaseline) {
            Text(title).mavType(.label).foregroundStyle(MavTheme.ink)
            Spacer(minLength: 8)
            if case .working = signal.state {
              ProgressView().controlSize(.small)
            }
          }
          Text(summary)
            .mavType(.body)
            .foregroundStyle(MavTheme.inkSecondary)
            .fixedSize(horizontal: false, vertical: true)
          Text(coverage)
            .mavType(.sub)
            .foregroundStyle(MavTheme.inkSecondary)
            .monospacedDigit()

          if case .failed(_, _, true) = signal.state {
            MavQuietButton(title: NSLocalizedString("analytics.retry", comment: ""), action: onRetry)
              .padding(.top, 4)
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
      }
      // One announcement per card, and it carries the coverage too. Combining without it read the
      // title and the state and then dropped the one number that says whether this device can do
      // the work at all. `.combine` keeps the retry button reachable as an action.
      .accessibilityElement(children: .combine)
      .accessibilityLabel(
        Text(
          String(
            format: NSLocalizedString("analytics.a11y.signal", comment: ""),
            title,
            summary,
            coverage
          )
        )
      )
    }
  }

  private var title: String { MavSignalCopy.title(signal.name) }

  private var coverage: String {
    String(
      format: NSLocalizedString("analytics.coverage", comment: ""),
      signal.runnable,
      signal.total
    )
  }

  private var summary: String { MavSignalCopy.describe(signal.state) }
}

/// Every sentence this surface can say, in one place.
///
/// Separated from the view so the copy is testable without a renderer, and so the Android twin has
/// one file to be compared against rather than a switch buried in a layout.
enum MavSignalCopy {
  /// The wearer-facing name of a signal.
  ///
  /// Looked up rather than derived from the slug: `"daytime_hrv".capitalized` is "Daytime Hrv",
  /// and a title case applied to an acronym is how a product surface starts looking generated.
  /// An unknown slug falls back to the derived form so a newly added signal is legible before its
  /// copy lands.
  static func title(_ slug: String) -> String {
    let key = "analytics.signal.\(slug)"
    let localized = NSLocalizedString(key, comment: "")
    if localized != key { return localized }
    return slug.replacingOccurrences(of: "_", with: " ").capitalized
  }

  /// One line of copy for one state.
  static func describe(_ state: MavSignalState) -> String {
    switch state {
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
      return describe(reasons)
    }
  }

  /// Why a signal cannot run. The first reason only: the causes are already collapsed to distinct
  /// ones by the reducer, and a card that lists four is a card nobody reads.
  static func describe(_ reasons: [MavUnavailable]) -> String {
    guard let reason = reasons.first else {
      return NSLocalizedString("analytics.state.idle", comment: "")
    }
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
      return String(
        format: NSLocalizedString("analytics.needsUpstream", comment: ""),
        title(model)
      )
    case let .preprocessingNotPorted(detail):
      return String(format: NSLocalizedString("analytics.notPorted", comment: ""), detail)
    }
  }

  /// The one-line summary Today's entry row carries, so the link says something about the wearer's
  /// data rather than only naming a screen.
  static func rowDetail(_ snapshot: MavAnalyticsSnapshot) -> String {
    if snapshot.working {
      return NSLocalizedString("analytics.a11y.working", comment: "")
    }
    guard !snapshot.signals.isEmpty else {
      return NSLocalizedString("analytics.empty", comment: "")
    }
    let runnable = snapshot.signals.reduce(0) { $0 + $1.runnable }
    let total = snapshot.signals.reduce(0) { $0 + $1.total }
    return String(
      format: NSLocalizedString("analytics.coverage", comment: ""),
      runnable,
      total
    )
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

  /// True once an engine exists. Surfaces hide the entry entirely before then rather than
  /// offering a link to a screen that can only say "nothing has run".
  var isAttached: Bool { engine != nil }

  /// Adopt an engine. Idempotent: a second scene activation must not build a second engine, which
  /// would discard the retry budgets and the published state of the first.
  func attach(_ build: () -> MavAnalyticsEngine) {
    guard engine == nil else { return }
    let engine = build()
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

  /// The same pass, awaited, so `.refreshable` keeps its spinner up for the real duration rather
  /// than snapping back the instant the gesture ends.
  func refreshAndWait() async {
    guard let engine else { return }
    await withCheckedContinuation { continuation in
      engine.runPass(deviceID: MavBackgroundAnalytics.deviceID, mode: .interactive) { _ in
        continuation.resume()
      }
    }
  }

  /// Clear the retry budgets and run again, for the retry affordance on a failed signal.
  func retry() {
    engine?.resetRetries()
    refresh()
  }

  /// Release whatever the runner is holding. Called when the app leaves the foreground: a loaded
  /// Core ML model costs far more resident than its package costs on disk, and a backgrounded app
  /// holding that is a backgrounded app the system kills first.
  func releaseResources() {
    engine?.releaseRunnerCache()
  }
}
