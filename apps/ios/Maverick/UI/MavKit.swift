import SwiftUI

// The Terrain component vocabulary. Every screen is assembled from these, and no screen writes a
// colour, a font size, or a corner radius of its own.
//
// Two rules are enforced structurally rather than by review:
//
//  1. A component takes a *family*, never a colour. It resolves its own wash, so "the surface
//     names the metric" cannot be broken by a caller passing the wrong thing.
//  2. A component that draws data takes an `accessibilitySummary` as a non-optional parameter. A
//     chart nobody can hear is a defect, and the type system is a better reviewer than a checklist.

// MARK: - Glass

extension View {
  /// **Every** surface in this app is Liquid Glass. That is the identity, and it is one function so
  /// it cannot drift into "some cards are glass and some are a flat fill".
  ///
  /// A status tint goes *into* the material via `Glass.tint` rather than being painted on top of
  /// it. Layering a translucent fill and a hairline over `glassEffect` fights the material and
  /// produces a flat blur that merely resembles glass — which is exactly what the first pass did.
  ///
  /// Chrome is the one exception, and only because the OS already gets there first: toolbars and
  /// the tab bar are glass without being asked, so nothing in those places calls this.
  func mavSurface(_ shape: some Shape = MavTheme.tileShape, tint: Color? = nil) -> some View {
    self.glassEffect(tint.map { Glass.regular.tint($0) } ?? .regular, in: shape)
  }

  /// A glass surface that responds to touch. Used for anything tappable, so a card that pushes
  /// somewhere feels different under the finger from one that does not.
  func mavInteractiveSurface(_ shape: some Shape = MavTheme.tileShape, tint: Color? = nil)
    -> some View
  {
    self.glassEffect(
      (tint.map { Glass.regular.tint($0) } ?? .regular).interactive(), in: shape)
  }

  /// Minimum target size, applied to every control in the kit so it is never a per-screen decision.
  func mavTarget() -> some View {
    frame(minWidth: 44, minHeight: 44)
  }
}

// MARK: - Photography

/// A landscape behind a wash.
///
/// The wash is not decoration: it is what makes text contrast a constant instead of a property of
/// whichever photograph happened to load. When the asset is missing the scene draws the wash over
/// the canvas and still looks deliberate — a missing image is never a blank rectangle.
///
/// Photography is deliberately scarce. One landscape on a screen reads as considered; a landscape
/// behind every card reads as a screensaver, and the copy stops being the thing you look at. The
/// rule is **at most one `.story` scene per screen**, and `.veiled` for anything repeating.
struct MavScene: View {
  /// Which band of the photograph survives the crop. Two scenes cut from one asset at different
  /// crops do not read as the same rectangle twice, which is what makes a single placeholder
  /// bearable in more than one place.
  enum Crop {
    case high, middle, low

    /// `aspectRatio(.fill)` oversizes the image; the frame's alignment decides which part of that
    /// overflow is kept.
    var alignment: Alignment {
      switch self {
      case .high: .top
      case .middle: .center
      case .low: .bottom
      }
    }
  }

  /// How hard the photograph is pushed back, which follows from what sits on top of it.
  enum Treatment {
    /// A hero. White copy sits directly on the landscape, so the scrim is dark and
    /// scheme-independent — white on a light photograph is the failure this prevents.
    case story
    /// A repeating surface. Ordinary ink sits on it, so the photograph is veiled almost to the
    /// canvas and survives only as texture. This is what lets a metric row carry a landscape
    /// without the row turning into a poster.
    case veiled

    var wash: Color {
      switch self {
      case .story: MavTheme.photoScrim
      case .veiled: MavTheme.photoVeil
      }
    }
  }

  var crop: Crop = .middle
  var treatment: Treatment = .story
  /// Placeholder art until licensed landscapes land, each of which needs a light and a dark
  /// variant so the wash is not fighting the photograph.
  var asset = "TerrainPlaceholder"

  var body: some View {
    ZStack {
      MavTheme.canvas
      if let image = UIImage(named: asset) {
        GeometryReader { proxy in
          Image(uiImage: image)
            .resizable()
            .aspectRatio(contentMode: .fill)
            .frame(width: proxy.size.width, height: proxy.size.height, alignment: crop.alignment)
            .clipped()
        }
        .allowsHitTesting(false)
      }
      treatment.wash
    }
    .accessibilityHidden(true)
  }
}

/// The atmosphere behind a tab root.
///
/// Three tab roots on a flat near-black read as a void: the cards float on nothing and the screen
/// has no depth to scroll against. This is the cheapest honest fix — two soft blooms of the one hue,
/// off-centre so the screen has a light source rather than a symmetrical glow, falling off to the
/// canvas well before the safe area ends.
///
/// It is deliberately *not* a photograph. The landscape already appears on the hero cards, on every
/// Vitals row and behind every detail screen; putting it behind the tab roots as well would make it
/// wallpaper rather than an accent. `tools/check_a11y.py` checks ink against the canvas at full
/// bloom strength, so no part of the gradient can put a label under its ratio.
struct MavAtmosphere: View {
  var body: some View {
    ZStack {
      MavTheme.canvas

      GeometryReader { proxy in
        let width = proxy.size.width
        // Upper bloom, drawn wide and soft so no edge of the gradient is ever visible.
        Circle()
          .fill(
            RadialGradient(
              // Fading to a fully transparent *hue* rather than to `.clear`. `.clear` is
              // (0,0,0,0), so interpolating to it walks the RGB toward black on the way out and
              // leaves a visible dark ring around the bloom.
              colors: [MavTheme.bloomTop, MavTheme.bloomTop.opacity(0)],
              center: .center,
              startRadius: 0,
              endRadius: width * 0.85))
          .frame(width: width * 1.7, height: width * 1.7)
          .position(x: width * 0.16, y: proxy.size.height * 0.06)

        // Lower bloom, weaker, on the other side. The asymmetry is the point — two matched glows
        // read as a gradient, one off-centre pair reads as light in a room.
        Circle()
          .fill(
            RadialGradient(
              colors: [MavTheme.bloomBottom, MavTheme.bloomBottom.opacity(0)],
              center: .center,
              startRadius: 0,
              endRadius: width * 0.8))
          .frame(width: width * 1.5, height: width * 1.5)
          .position(x: width * 0.95, y: proxy.size.height * 0.72)
      }
      .allowsHitTesting(false)
    }
    .accessibilityHidden(true)
  }
}

// MARK: - Chrome

/// The date stepper, as a toolbar principal item. It draws no background of its own — the toolbar
/// is already glass, and a second material inside it reads as a smudge.
///
/// Forward is disabled on the newest day rather than hidden, so the control does not change shape
/// as you walk back through history.
struct MavDateStepper: View {
  @Binding var day: Date
  let canGoForward: Bool
  var unit: Calendar.Component = .day
  @State private var showCalendar = false

  private var title: String {
    let calendar = Calendar.current
    if calendar.isDateInToday(day) { return "Today" }
    if calendar.isDateInYesterday(day) { return "Yesterday" }
    return day.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated))
  }

  var body: some View {
    HStack(spacing: 2) {
      Button {
        day = Calendar.current.date(byAdding: unit, value: -1, to: day) ?? day
      } label: {
        Image(systemName: "chevron.left")
          .font(.system(size: 12, weight: .semibold))
          .frame(width: 30, height: 34)
          .contentShape(.rect)
      }
      .accessibilityLabel("Previous day")

      // The title is a control, not a label. Stepping a day at a time is fine for yesterday and
      // useless for last March, so tapping it opens a calendar.
      Button {
        showCalendar = true
      } label: {
        Text(title)
          .mavType(.caption)
          .foregroundStyle(MavTheme.ink)
          .contentTransition(.numericText())
          .animation(MavTheme.calm, value: title)
          .lineLimit(1)
          .frame(minWidth: 78, minHeight: 34)
          .contentShape(.rect)
      }
      .accessibilityLabel(day.formatted(date: .complete, time: .omitted))
      .accessibilityHint("Opens a calendar to pick a day")

      Button {
        day = Calendar.current.date(byAdding: unit, value: 1, to: day) ?? day
      } label: {
        Image(systemName: "chevron.right")
          .font(.system(size: 12, weight: .semibold))
          .frame(width: 30, height: 34)
          .contentShape(.rect)
      }
      .disabled(!canGoForward)
      .accessibilityLabel("Next day")
    }
    .foregroundStyle(MavTheme.inkSecondary)
    .popover(isPresented: $showCalendar) {
      MavDayPicker(day: $day) { showCalendar = false }
    }
  }
}

/// The calendar behind the date title. A real `DatePicker`, so it brings its own month navigation,
/// its own locale and calendar handling, and its own VoiceOver rotor.
struct MavDayPicker: View {
  @Binding var day: Date
  let onDone: () -> Void

  var body: some View {
    VStack(spacing: 0) {
      DatePicker(
        "Day",
        selection: $day,
        // The future holds no recorded days, so it is not offered.
        in: ...Date(),
        displayedComponents: .date
      )
      .datePickerStyle(.graphical)
      .tint(MavTheme.accent)
      .padding(.horizontal, 8)

      Divider()

      Button("Jump to today") {
        day = Date()
        onDone()
      }
      .mavType(.label)
      .frame(maxWidth: .infinity, minHeight: 48)
    }
    .frame(width: 340)
    .presentationCompactAdaptation(.popover)
  }
}

/// The device chip, as a toolbar trailing item — so it sits hard against the right edge.
///
/// The battery percentage is shown whenever the core has one. It disappears together with the link,
/// because a battery percentage with no link is a stale number pretending to be live.
struct MavDeviceChip: View {
  let batteryPercent: Int?
  let connected: Bool
  let deviceName: String?
  let action: () -> Void

  private var summary: String {
    guard connected else { return "No device connected. Open device settings." }
    let name = deviceName ?? "Device"
    guard let batteryPercent else { return "\(name), connected." }
    return "\(name), \(batteryPercent) percent battery, connected."
  }

  var body: some View {
    Button(action: action) {
      HStack(spacing: 6) {
        if connected, let batteryPercent {
          Text("\(batteryPercent)%")
            .mavType(.caption)
            .monospacedDigit()
        }
        MavStrapGlyph(connected: connected)
      }
      .foregroundStyle(MavTheme.ink)
      .frame(minHeight: 34)
      .contentShape(.rect)
    }
    .accessibilityLabel(summary)
  }
}

struct MavStrapGlyph: View {
  let connected: Bool

  var body: some View {
    RoundedRectangle(cornerRadius: 5.5, style: .continuous)
      .strokeBorder(MavTheme.ink.opacity(0.85), lineWidth: 1.6)
      .frame(width: 15, height: 21)
      .overlay(alignment: .topTrailing) {
        Circle()
          .fill(connected ? MavTheme.liveInk() : MavTheme.ink.opacity(0.25))
          .frame(width: 5.5, height: 5.5)
          .offset(x: 3.5, y: 2.5)
      }
      .accessibilityHidden(true)
  }
}

// MARK: - Structure

struct MavSectionHeader: View {
  let title: String

  var body: some View {
    Text(title)
      .mavType(.title)
      .foregroundStyle(MavTheme.ink)
      .frame(maxWidth: .infinity, alignment: .leading)
      .padding(.top, MavTheme.sectionGap)
      .padding(.bottom, 8)
      .accessibilityAddTraits(.isHeader)
  }
}

/// A neutral card.
struct MavTile<Content: View>: View {
  var padded = true
  @ViewBuilder var content: Content

  var body: some View {
    content
      .padding(padded ? MavTheme.tilePadding : 0)
      .frame(maxWidth: .infinity, alignment: .leading)
      .mavSurface(MavTheme.tileShape)
  }
}

/// A card whose surface carries a metric's identity. The wash is resolved here from the family, so
/// no caller can tint a card with something that is not one of the seven.
struct MavStatusCard<Content: View>: View {
  /// Which metric this card belongs to, or nil when it is not a metric — a connector, a device, a
  /// prompt. The wash names identity, never a verdict.
  var family: MavFamily?
  var shape: AnyShape = AnyShape(MavTheme.cardShape)
  @ViewBuilder var content: Content

  var body: some View {
    content
      .padding(MavTheme.tilePadding)
      .frame(maxWidth: .infinity, alignment: .leading)
      .mavSurface(shape, tint: family.map { MavTheme.tint($0) } ?? MavTheme.neutralTint)
  }
}

/// The honest absence. It renders the core's reason and never a dash in the shape of a value.
struct MavUnavailableCard: View {
  let name: String
  let reason: String

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      Text(name)
        .mavType(.label)
        .foregroundStyle(MavTheme.ink)
      Text(reason)
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)
        .fixedSize(horizontal: false, vertical: true)
    }
    .padding(.horizontal, MavTheme.tilePadding)
    .padding(.vertical, 17)
    .frame(maxWidth: .infinity, alignment: .leading)
    .mavSurface(MavTheme.tileShape, tint: MavTheme.neutralTint)
    // The dashed edge is the one thing that separates an absent metric from a present one at a
    // glance. It sits over the glass rather than replacing it, so the material stays consistent.
    .overlay {
      MavTheme.tileShape.strokeBorder(
        MavTheme.hairlineStrong, style: StrokeStyle(lineWidth: 1, dash: [5, 4]))
    }
    .accessibilityElement(children: .combine)
    .accessibilityLabel("\(name). \(reason)")
  }
}

/// A settings-style row. `detail` is a second line, `value` a trailing string.
struct MavRow<Trailing: View>: View {
  let title: String
  var detail: String?
  var accessibilityValue: String?
  @ViewBuilder var trailing: Trailing

  var body: some View {
    HStack(spacing: 13) {
      VStack(alignment: .leading, spacing: 3) {
        Text(title).mavType(.label).foregroundStyle(MavTheme.ink)
        if let detail {
          Text(detail)
            .mavType(.sub)
            .foregroundStyle(MavTheme.inkSecondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
      Spacer(minLength: 8)
      trailing
    }
    .padding(.horizontal, MavTheme.tilePadding)
    .padding(.vertical, 14)
    .frame(minHeight: 44)
    .contentShape(.rect)
  }
}

extension MavRow where Trailing == EmptyView {
  init(title: String, detail: String? = nil, accessibilityValue: String? = nil) {
    self.init(
      title: title, detail: detail, accessibilityValue: accessibilityValue, trailing: { EmptyView() }
    )
  }
}

/// A row that pushes somewhere.
struct MavNavRow: View {
  let title: String
  var detail: String?
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      MavRow(title: title, detail: detail) {
        Image(systemName: "chevron.right")
          .font(.system(size: 13, weight: .semibold))
          .foregroundStyle(MavTheme.inkSecondary)
      }
    }
    .buttonStyle(.plain)
    .accessibilityAddTraits(.isButton)
  }
}

/// A switch row. `role: .switch` is what makes VoiceOver announce it as on or off.
struct MavToggleRow: View {
  let title: String
  var detail: String?
  var badge: String?
  @Binding var isOn: Bool

  var body: some View {
    MavRow(title: title, detail: detail) {
      Toggle("", isOn: $isOn)
        .labelsHidden()
        .tint(MavTheme.accent)
    }
    .accessibilityElement(children: .combine)
    .accessibilityLabel(badge.map { "\(title), \($0)" } ?? title)
  }
}

/// A capability or state chip. `enabled: false` reads as struck through *and* says so, because a
/// strikethrough alone is a colour-free but still purely visual signal.
struct MavChip: View {
  let text: String
  var enabled = true

  var body: some View {
    Text(text)
      .mavType(.sub)
      .foregroundStyle(MavTheme.inkSecondary)
      .padding(.horizontal, 12)
      .padding(.vertical, 7)
      .mavSurface(.capsule)
      .opacity(enabled ? 1 : 0.45)
      .strikethrough(!enabled)
      .accessibilityLabel(enabled ? text : "\(text), not provided")
  }
}

/// A compact secondary marker.
struct MavBadge: View {
  let text: String

  var body: some View {
    Text(text)
      .mavType(.caption)
      .foregroundStyle(MavTheme.inkSecondary)
      .padding(.horizontal, 7)
      .padding(.vertical, 4)
      .mavSurface(MavTheme.chipShape)
  }
}

/// A hairline between rows in a grouped tile.
struct MavDivider: View {
  var body: some View {
    Rectangle()
      .fill(MavTheme.hairline)
      .frame(height: 1)
      .padding(.leading, MavTheme.tilePadding)
      .accessibilityHidden(true)
  }
}

/// The primary affordance shape, used once per screen at most.
/// The one affirmative action a screen is allowed, as a label. Shared by the button and the link
/// forms so the accent treatment is written once and the two cannot drift apart.
private struct MavPrimaryLabel: View {
  let title: String
  var detail: String?
  var systemImage: String?

  var body: some View {
    HStack(spacing: 14) {
      if let systemImage {
        Image(systemName: systemImage)
          .font(.system(size: 15, weight: .semibold))
          .foregroundStyle(MavTheme.onAccent)
          .frame(width: 24, height: 24)
      }
      VStack(alignment: .leading, spacing: 3) {
        Text(title).mavType(.label).foregroundStyle(MavTheme.onAccent)
        if let detail {
          Text(detail).mavType(.sub).foregroundStyle(MavTheme.onAccent.opacity(0.76))
        }
      }
      Spacer(minLength: 8)
      Image(systemName: "chevron.right")
        .font(.system(size: 14, weight: .semibold))
        .foregroundStyle(MavTheme.onAccent.opacity(0.76))
    }
    .padding(.horizontal, 18)
    .padding(.vertical, 14)
    .frame(maxWidth: .infinity, alignment: .leading)
    .contentShape(.rect)
  }
}

struct MavPrimaryButton: View {
  let title: String
  var detail: String?
  var systemImage: String?
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      MavPrimaryLabel(title: title, detail: detail, systemImage: systemImage)
    }
    .buttonStyle(.glassProminent)
    .tint(MavTheme.accent)
    .accessibilityLabel(detail.map { "\(title). \($0)" } ?? title)
  }
}

/// The same affirmative action, when it *pushes* rather than acts. A `NavigationLink` rather than a
/// `Button` that mutates a path, so the system supplies the press behaviour, the accessibility
/// traits, and the back gesture on the destination.
struct MavPrimaryLink<Value: Hashable>: View {
  let title: String
  var detail: String?
  var systemImage: String?
  let value: Value

  var body: some View {
    NavigationLink(value: value) {
      MavPrimaryLabel(title: title, detail: detail, systemImage: systemImage)
    }
    .buttonStyle(.glassProminent)
    .tint(MavTheme.accent)
    .accessibilityLabel(detail.map { "\(title). \($0)" } ?? title)
  }
}

/// A compact utility shortcut. The icon gets the glass interaction; the label stays quiet.
struct MavToolShortcut: View {
  let title: String
  let systemImage: String
  let action: () -> Void

  var body: some View {
    VStack(spacing: 5) {
      Button(action: action) {
        Image(systemName: systemImage)
          .font(.system(size: 16, weight: .medium))
          .frame(width: 42, height: 42)
      }
      .buttonStyle(.glass)
      .accessibilityLabel(title)

      Text(title)
        .mavType(.caption)
        .foregroundStyle(MavTheme.inkSecondary)
        .lineLimit(1)
    }
    .frame(maxWidth: .infinity)
  }
}

/// A quiet secondary action, used in a row beneath the primary one.
struct MavQuietButton: View {
  let title: String
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      Text(title).mavType(.body)
    }
    .buttonStyle(.glass)
    .tint(MavTheme.inkSecondary)
    .controlSize(.small)
  }
}

/// A full-width action. `.glass` and `.glassProminent` are the system's own button styles, so these
/// pick up the real material, the press animation, and the destructive-role treatment rather than
/// approximating all three.
struct MavWideButton: View {
  let title: String
  var prominent = false
  var destructive = false
  let action: () -> Void

  var body: some View {
    Button(role: destructive ? .destructive : nil, action: action) {
      Text(title)
        .mavType(.label)
        .frame(maxWidth: .infinity, minHeight: 26)
    }
    .buttonStyle(.glass)
    .controlSize(.large)
    .tint(prominent ? MavTheme.accent : nil)
  }
}
