import SwiftUI

// Surfaces + text helpers + motion. Luminous radial-glow content tiles, a
// near-black screen backdrop, and iOS Liquid Glass reserved for chrome. All text
// helpers resolve to the adaptive ink colour at strong opacity for real contrast.

// MARK: - Glass chrome (pills, tab bar, FAB only)

private struct AuraGlassModifier<S: Shape>: ViewModifier {
  let shape: S
  let interactive: Bool
  @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

  @ViewBuilder
  func body(content: Content) -> some View {
    if reduceTransparency {
      content.background(AuraDesign.card, in: shape)
    } else {
      content.background(.ultraThinMaterial, in: shape)
    }
  }
}

extension View {
  func auraGlass(_ shape: some Shape, interactive: Bool = false) -> some View {
    modifier(AuraGlassModifier(shape: shape, interactive: interactive))
  }
}

// MARK: - Radial-glow tile (the signature content surface)

private struct GlowTileModifier: ViewModifier {
  var family: AuraDesign.Family?
  var padding: CGFloat
  var radius: CGFloat
  @Environment(\.colorScheme) private var colorScheme

  func body(content: Content) -> some View {
    let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
    let dark = colorScheme == .dark
    return content
      .padding(padding)
      .frame(maxWidth: .infinity, alignment: .leading)
      .background {
        GeometryReader { geo in
          let maxDim = max(geo.size.width, geo.size.height)
          ZStack {
            if let family {
              shape.fill(
                RadialGradient(
                  colors: [family.glow, family.glow, family.glow.opacity(dark ? 0.72 : 0.82), family.glowEdge],
                  center: UnitPoint(x: 0.5, y: dark ? 0.66 : 0.5),
                  startRadius: 2,
                  endRadius: maxDim * (dark ? 0.95 : 0.78)
                )
              )
              // Top scrim: labels + numerals stay crisp where they sit; the
              // bottom of the tile keeps its vivid glow.
              if dark {
                shape.fill(
                  LinearGradient(
                    stops: [.init(color: .black.opacity(0.34), location: 0),
                            .init(color: .clear, location: 0.55)],
                    startPoint: .top, endPoint: .bottom
                  )
                )
              }
            } else {
              shape.fill(
                RadialGradient(colors: [AuraDesign.card, AuraDesign.cardEdge],
                               center: UnitPoint(x: 0.5, y: 0.4), startRadius: 2, endRadius: maxDim)
              )
            }
            shape.strokeBorder(.white.opacity(dark ? 0.10 : 0.5), lineWidth: 1)
          }
        }
      }
      .clipShape(shape)
      // Emitted-light bloom: the tile casts an outer glow in its own hue. Stronger
      // in light so the pastel cards read as lit against the off-white canvas.
      .shadow(color: (family?.glow ?? .clear).opacity(dark ? 0.42 : 0.5), radius: dark ? 24 : 26, y: 6)
  }
}

extension View {
  /// A rounded tile with a family radial glow (or the neutral card when nil).
  func auraGlowTile(_ family: AuraDesign.Family? = nil,
                    padding: CGFloat = AuraDesign.tilePadding,
                    radius: CGFloat = AuraDesign.tileRadius) -> some View {
    modifier(GlowTileModifier(family: family, padding: padding, radius: radius))
  }

  /// Editorial screen backdrop: pure black (or off-white) + one faint lead glow.
  func auraScreen(_ lead: AuraDesign.Family? = nil) -> some View {
    background {
      ZStack {
        AuraDesign.bg
        if let lead {
          RadialGradient(colors: [lead.glow.opacity(0.26), lead.glow.opacity(0.05), .clear],
                         center: UnitPoint(x: 0.5, y: -0.02), startRadius: 0, endRadius: 540)
        }
      }
      .ignoresSafeArea()
    }
  }

  /// Title-case tile label — strong, legible, no tracked all-caps.
  func auraLabel() -> some View {
    font(AuraDesign.label)
      .foregroundStyle(AuraDesign.ink.opacity(0.92))
      .lineLimit(1)
      .minimumScaleFactor(0.85)
  }
}

// MARK: - Sheet chrome (every flyout gets a title + close, no exceptions)

/// The shared flyout container: themed backdrop, title bar with an always-
/// present ✕, scrollable content. Everything presented in a `.sheet` sits in
/// one of these so no flyout can trap the user.
struct AuraSheet<Content: View>: View {
  let title: String
  var family: AuraDesign.Family?
  var scrolls = true
  @ViewBuilder var content: () -> Content

  @Environment(\.dismiss) private var dismiss

  var body: some View {
    Group {
      if scrolls {
        ScrollView {
          VStack(alignment: .leading, spacing: AuraDesign.sectionGap) { content() }
            .padding(.horizontal, AuraDesign.screenMargin)
            .padding(.top, 4)
            .padding(.bottom, 48)
        }
        .scrollIndicators(.hidden)
      } else {
        content()
      }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .auraScreen(family)
    .safeAreaInset(edge: .top) { AuraSheetBar(title: title) }
  }
}

/// Title + ✕ bar used by every sheet (and pushed flyouts that present modally).
struct AuraSheetBar: View {
  let title: String
  @Environment(\.dismiss) private var dismiss

  var body: some View {
    HStack {
      Text(title).font(AuraDesign.heading(20)).foregroundStyle(AuraDesign.ink)
      Spacer()
      Button { dismiss() } label: {
        Image(systemName: "xmark")
          .font(.system(size: 15, weight: .bold))
          .foregroundStyle(AuraDesign.ink)
          .frame(width: 40, height: 40)
          .auraGlass(Circle(), interactive: true)
          .contentShape(Circle())
      }
      .buttonStyle(.plain)
      .accessibilityLabel(Text("Close"))
    }
    .padding(.horizontal, AuraDesign.screenMargin)
    .padding(.top, 10)
    .padding(.bottom, 8)
  }
}

// MARK: - Motion

private struct AuraRevealModifier: ViewModifier {
  let revealed: Bool
  let index: Int
  @Environment(\.accessibilityReduceMotion) private var reduceMotion

  func body(content: Content) -> some View {
    content
      .opacity(revealed ? 1 : 0)
      .offset(y: (revealed || reduceMotion) ? 0 : 18)
      .animation(
        reduceMotion ? .easeOut(duration: 0.2)
                     : .spring(response: 0.55, dampingFraction: 0.86).delay(Double(index) * 0.07),
        value: revealed
      )
  }
}

extension View {
  func auraReveal(_ revealed: Bool, index: Int) -> some View {
    modifier(AuraRevealModifier(revealed: revealed, index: index))
  }
}

struct AuraPressStyle: ButtonStyle {
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  func makeBody(configuration: Configuration) -> some View {
    configuration.label
      .scaleEffect(configuration.isPressed && !reduceMotion ? 0.97 : 1)
      .animation(.spring(response: 0.3, dampingFraction: 0.62), value: configuration.isPressed)
  }
}
