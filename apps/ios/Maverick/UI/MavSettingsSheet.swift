import SwiftUI

// Settings holds what is genuinely a preference, and nothing else.
//
// No device row, no pairing entry, no connector row, no battery saver: all four live in the device
// sheet, which is one tap from every tab. Nothing here is a stand-in that opens another settings
// screen, and no destination is more than two pushes deep.

struct MavSettingsSheet: View {
  @EnvironmentObject private var profile: ProfileStore
  @Environment(\.dismiss) private var dismiss
  @AppStorage(AppearanceMode.storageKey) private var appearanceRaw = AppearanceMode.system.rawValue

  var body: some View {
    NavigationStack {
      ZStack {
        MavTheme.canvas.ignoresSafeArea()

        ScrollView {
          VStack(alignment: .leading, spacing: MavTheme.cardSpacing) {
            MavSectionHeader(title: "You")
            VStack(spacing: 0) {
              NavigationLink { MavProfileView() } label: {
                MavRow(title: "Body profile", detail: profileSummary) { chevron }
              }
              .buttonStyle(.plain)
              MavDivider()
              NavigationLink { MavJournalView() } label: {
                MavRow(title: "Journal", detail: "What you log against your days") { chevron }
              }
              .buttonStyle(.plain)
            }
            .mavSurface(MavTheme.tileShape)

            MavSectionHeader(title: "Appearance")
            MavTile {
              Picker("Appearance", selection: $appearanceRaw) {
                Text("System").tag(AppearanceMode.system.rawValue)
                Text("Light").tag(AppearanceMode.light.rawValue)
                Text("Dark").tag(AppearanceMode.dark.rawValue)
              }
              .pickerStyle(.segmented)
            }

            MavSectionHeader(title: "Units")
            MavTile {
              MavUnitsControls()
            }

            MavSectionHeader(title: "Data")
            VStack(spacing: 0) {
              NavigationLink { MavDataView() } label: {
                MavRow(title: "Storage and export", detail: "Everything stays on this phone") {
                  chevron
                }
              }
              .buttonStyle(.plain)
              MavDivider()
              NavigationLink { MavDiagnosticsView() } label: {
                MavRow(title: "Diagnostics", detail: "Connection and data details") {
                  chevron
                }
              }
              .buttonStyle(.plain)
            }
            .mavSurface(MavTheme.tileShape)

            MavSectionHeader(title: "About")
            MavTile {
              VStack(alignment: .leading, spacing: 7) {
                Text("Maverick").mavType(.title).foregroundStyle(MavTheme.ink)
                Text(
                  "Every byte of decoding and analytics runs on this device. Nothing leaves it."
                )
                .mavType(.sub)
                .foregroundStyle(MavTheme.inkSecondary)
                .fixedSize(horizontal: false, vertical: true)
                Text(versionLine)
                  .mavType(.sub)
                  .foregroundStyle(MavTheme.inkSecondary)
                  .padding(.top, 4)
              }
            }
          }
          .padding(.horizontal, MavTheme.screenMargin)
          .padding(.bottom, 40)
        }
        .scrollIndicators(.hidden)
      }
      .navigationTitle("Settings")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .topBarTrailing) { Button("Done") { dismiss() } }
      }
    }
  }

  private var chevron: some View {
    Image(systemName: "chevron.right")
      .font(.system(size: 13, weight: .semibold))
      .foregroundStyle(MavTheme.inkSecondary)
  }

  private var profileSummary: String {
    "\(profile.age) years · \(Int(profile.weightKg)) kg · max \(profile.hrMax) bpm"
  }

  private var versionLine: String {
    let version = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "—"
    let build = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "—"
    return "Version \(version) (\(build))"
  }
}

/// The unit choices, which are display preferences and therefore genuinely the platform's to own.
struct MavUnitsControls: View {
  @AppStorage(UnitPrefs.effortScaleKey) private var effortScale = EffortScale.hundred.rawValue
  @AppStorage(UnitPrefs.systemKey) private var system = UnitSystem.metric.rawValue

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      VStack(alignment: .leading, spacing: 8) {
        Text("Effort axis").mavType(.label).foregroundStyle(MavTheme.ink)
        Picker("Effort axis", selection: $effortScale) {
          Text("0–100").tag(EffortScale.hundred.rawValue)
          Text("0–21").tag(EffortScale.whoop.rawValue)
        }
        .pickerStyle(.segmented)
      }

      VStack(alignment: .leading, spacing: 8) {
        Text("Measurements").mavType(.label).foregroundStyle(MavTheme.ink)
        Picker("Measurements", selection: $system) {
          Text("Metric").tag(UnitSystem.metric.rawValue)
          Text("Imperial").tag(UnitSystem.imperial.rawValue)
        }
        .pickerStyle(.segmented)
      }
    }
  }
}

// MARK: - Profile

struct MavProfileView: View {
  @EnvironmentObject private var profile: ProfileStore

  var body: some View {
    MavDetailScaffold(title: "Body profile") {
      MavTile {
        Text(
          "Used for personal zones and cycle insights. Stored only on this phone."
        )
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)
        .fixedSize(horizontal: false, vertical: true)
      }

      VStack(spacing: 0) {
        stepperRow("Age", value: $profile.age, range: 5...120, unit: "years")
        MavDivider()
        MavRow(
          title: "Sex",
          detail: profile.tracksCycle ? "Cycle insights appear automatically." : nil
        ) {
          Picker("Sex", selection: $profile.sex) {
            Text("Female").tag("female")
            Text("Male").tag("male")
          }
          .pickerStyle(.menu)
          .labelsHidden()
        }
        MavDivider()
        doubleRow("Weight", value: $profile.weightKg, range: 20...300, step: 0.5, unit: "kg")
        MavDivider()
        doubleRow("Height", value: $profile.heightCm, range: 90...250, step: 1, unit: "cm")
        MavDivider()
        stepperRow(
          "Max heart rate", value: $profile.hrMaxOverride, range: 0...230, unit: "bpm",
          zeroLabel: "Automatic (\(profile.hrMax))")
      }
      .mavSurface(MavTheme.tileShape)
    }
  }

  private func stepperRow(
    _ title: String, value: Binding<Int>, range: ClosedRange<Int>, unit: String,
    zeroLabel: String? = nil
  ) -> some View {
    MavRow(title: title) {
      Stepper(
        value: value, in: range,
        label: {
          Text(value.wrappedValue == 0 ? (zeroLabel ?? "0") : "\(value.wrappedValue) \(unit)")
            .mavType(.label)
            .monospacedDigit()
            .foregroundStyle(MavTheme.inkSecondary)
        })
    }
  }

  private func doubleRow(
    _ title: String, value: Binding<Double>, range: ClosedRange<Double>, step: Double, unit: String
  ) -> some View {
    MavRow(title: title) {
      Stepper(
        value: value, in: range, step: step,
        label: {
          Text("\(value.wrappedValue.formatted(.number.precision(.fractionLength(0)))) \(unit)")
            .mavType(.label)
            .monospacedDigit()
            .foregroundStyle(MavTheme.inkSecondary)
        })
    }
  }
}

// MARK: - Data

struct MavDataView: View {
  @EnvironmentObject private var repo: Repository
  @State private var sizeBytes: Int64?

  var body: some View {
    MavDetailScaffold(title: "Storage") {
      MavTile {
        VStack(alignment: .leading, spacing: 7) {
          Text("On this device only").mavType(.title).foregroundStyle(MavTheme.ink)
          Text(
            "Your history stays on this phone. There is no account or cloud copy."
          )
          .mavType(.body)
          .foregroundStyle(MavTheme.inkSecondary)
          .fixedSize(horizontal: false, vertical: true)
        }
      }

      VStack(spacing: 0) {
        MavRow(title: "Store size") {
          Text(sizeBytes.map { ByteCountFormatter.string(fromByteCount: $0, countStyle: .file) } ?? "—")
            .mavType(.label)
            .monospacedDigit()
            .foregroundStyle(MavTheme.inkSecondary)
        }
        MavDivider()
        MavRow(title: "Scored days") {
          Text("\(repo.days.count)")
            .mavType(.label)
            .monospacedDigit()
            .foregroundStyle(MavTheme.inkSecondary)
        }
      }
      .mavSurface(MavTheme.tileShape)
    }
    .task {
      sizeBytes = await repo.storeHandle()?.databaseFileSizeBytes()
    }
  }
}
