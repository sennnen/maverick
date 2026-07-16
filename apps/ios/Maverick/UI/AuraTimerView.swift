import SwiftUI

// General-purpose countdown timer (§7): sauna, cold plunge, cooking, stretching.
// Set a duration, the band buzzes at zero and keeps insisting until acknowledged —
// on-screen Stop or a band double-tap (only while ringing, §2).

struct AuraTimerView: View {
  @EnvironmentObject private var model: AppModel
  @ObservedObject private var timer: CountdownTimer
  @EnvironmentObject private var live: LiveState

  @State private var hours: Int
  @State private var minutes: Int
  @State private var seconds: Int
  /// 1 s UI clock — the engine's heartbeat drives state; this keeps the big number moving.
  @State private var now = Date()
  private let clock = Timer.publish(every: 0.5, on: .main, in: .common).autoconnect()

  init(timer: CountdownTimer) {
    self.timer = timer
    let last = max(timer.lastDurationSeconds, 1)
    _hours = State(initialValue: min(last / 3600, 23))
    _minutes = State(initialValue: (last % 3600) / 60)
    _seconds = State(initialValue: last % 60)
  }

  var body: some View {
    AuraSheet(title: "Timer", family: .energy) {
      hero
      if timer.isRinging {
        ringingCard
      } else if timer.isRunning || timer.pausedRemaining != nil {
        runningControls
      } else {
        durationPicker
        startButton
      }
      if !live.bonded && !timer.isRinging {
        Text("No strap connected. The timer still counts here and notifies on this device; the wrist buzz joins when the strap connects.")
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.45))
          .fixedSize(horizontal: false, vertical: true)
          .padding(.horizontal, 4)
      }
    }
    .onReceive(clock) { now = $0 }
  }

  // MARK: Hero

  private var hero: some View {
    VStack(alignment: .leading, spacing: 16) {
      HStack {
        Text("Countdown").auraLabel()
        Spacer()
        if timer.isRinging {
          AuraStatusChip(text: "Time's up", kind: .negative, pulsing: true)
        } else if timer.isRunning {
          AuraStatusChip(text: "Running", kind: .positive, pulsing: true)
        } else if timer.pausedRemaining != nil {
          AuraStatusChip(text: "Paused", kind: .caution)
        }
      }
      Text(displayTime)
        .font(AuraDesign.mega(84)).foregroundStyle(AuraDesign.ink)
        .monospacedDigit()
        .contentTransition(.numericText())
        .lineLimit(1).minimumScaleFactor(0.5)
      if timer.isRunning || timer.pausedRemaining != nil {
        AuraSlider(value: progress, glow: AuraDesign.Family.energy.glow)
      } else {
        Text("Buzzes your wrist when it hits zero. Double-tap the band to stop it.")
          .font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.7))
          .fixedSize(horizontal: false, vertical: true)
      }
    }
    .frame(maxWidth: .infinity, minHeight: 210, alignment: .leading)
    .auraGlowTile(.energy, padding: 22, radius: 34)
  }

  private var displayTime: String {
    let s = timer.isRinging ? 0 : (timer.remaining ?? pickedSeconds)
    let _ = now   // re-derive every clock tick while running
    return s >= 3600
      ? String(format: "%d:%02d:%02d", s / 3600, (s % 3600) / 60, s % 60)
      : String(format: "%02d:%02d", s / 60, s % 60)
  }

  private var progress: Double {
    guard let r = timer.remaining, timer.lastDurationSeconds > 0 else { return 0 }
    return 1 - Double(r) / Double(timer.lastDurationSeconds)
  }

  private var pickedSeconds: Int { hours * 3600 + minutes * 60 + seconds }

  // MARK: Idle → duration picker

  private var durationPicker: some View {
    VStack(alignment: .leading, spacing: 14) {
      HStack(spacing: 6) {
        ForEach([1, 2, 5, 10, 15, 30], id: \.self) { m in
          let active = hours == 0 && minutes == m && seconds == 0
          Button { hours = 0; minutes = m; seconds = 0 } label: {
            Text("\(m)m")
              .font(AuraDesign.caption).monospacedDigit()
              .foregroundStyle(active ? Color.black : AuraDesign.ink.opacity(0.7))
              .padding(.vertical, 9).frame(maxWidth: .infinity)
              .background(active ? AnyShapeStyle(AuraDesign.accent) : AnyShapeStyle(AuraDesign.ink.opacity(0.08)),
                          in: Capsule())
              .contentShape(Capsule())
          }
          .buttonStyle(.plain)
        }
      }
      // Three endless wheels, iOS-clock style: hours · minutes · seconds, to the second.
      LoopingTimePicker(hours: $hours, minutes: $minutes, seconds: $seconds)
        .frame(height: 160)
    }
    .auraDarkCard(padding: 18)
  }

  private var startButton: some View {
    Button {
      guard pickedSeconds > 0 else { return }
      withAnimation(.spring(response: 0.4, dampingFraction: 0.85)) {
        timer.start(seconds: pickedSeconds)
      }
    } label: {
      Text("Start")
        .font(AuraDesign.label).foregroundStyle(pickedSeconds > 0 ? Color.black : AuraDesign.ink.opacity(0.4))
        .frame(maxWidth: .infinity).padding(.vertical, 16)
        .background(pickedSeconds > 0 ? AnyShapeStyle(AuraDesign.accent) : AnyShapeStyle(AuraDesign.ink.opacity(0.08)),
                    in: Capsule())
        .contentShape(Capsule())
    }
    .buttonStyle(AuraPressStyle())
    .disabled(pickedSeconds == 0)
  }

  // MARK: Running / paused controls

  private var runningControls: some View {
    HStack(spacing: 12) {
      if timer.pausedRemaining != nil {
        controlButton("Resume", prominent: true) { timer.resumePaused() }
      } else {
        controlButton("Pause", prominent: false) { timer.pause() }
      }
      controlButton("Reset", prominent: false) {
        withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) { timer.reset() }
      }
    }
  }

  private func controlButton(_ title: String, prominent: Bool,
                             action: @escaping () -> Void) -> some View {
    Button(action: action) {
      Text(title)
        .font(AuraDesign.label)
        .foregroundStyle(prominent ? Color.black : AuraDesign.ink)
        .frame(maxWidth: .infinity).padding(.vertical, 14)
        .background(prominent ? AnyShapeStyle(AuraDesign.accent) : AnyShapeStyle(AuraDesign.ink.opacity(0.08)),
                    in: Capsule())
        .contentShape(Capsule())
    }
    .buttonStyle(AuraPressStyle())
  }

  // MARK: Ringing

  private var ringingCard: some View {
    VStack(spacing: 14) {
      Button {
        withAnimation(.spring(response: 0.35, dampingFraction: 0.85)) { timer.acknowledge() }
      } label: {
        Text("Stop")
          .font(AuraDesign.label).foregroundStyle(Color.white)
          .frame(maxWidth: .infinity).padding(.vertical, 16)
          .background(AuraDesign.dyn(dark: 0xC2273B, light: 0xD83A44), in: Capsule())
          .contentShape(Capsule())
      }
      .buttonStyle(AuraPressStyle())
      Text("Or double-tap your band.")
        .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
    }
  }
}
