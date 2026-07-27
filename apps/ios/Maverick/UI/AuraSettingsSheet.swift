import SwiftUI

// The ONE app-wide settings sheet, opened from the top-right cog on every hub
// (never a tab, never tab-specific). Profile · Device · Integrations ·
// Appearance · Data · Coach & Journal · About.

struct AuraSettingsSheet: View {
  @EnvironmentObject private var repo: Repository
  @EnvironmentObject private var model: AppModel
  @EnvironmentObject private var live: LiveState
  @EnvironmentObject private var profile: ProfileStore
  @EnvironmentObject private var health: HealthKitBridge

  @Environment(\.dismiss) private var dismiss
  @AppStorage(AppearanceMode.storageKey) private var appearanceRaw = AppearanceMode.system.rawValue

  @State private var showPairing = false
  @State private var showMigrate = false
  @AppStorage(AuraDataProtection.storageKey) private var fileProtectionOn = false
  /// Battery saver, persisted so the choice survives a restart (ADR-030).
  @AppStorage("aura.lowPowerEnabled") private var lowPowerOn = false
  @AppStorage(JournalReminder.enabledKey) private var journalReminderOn = false
  @AppStorage(JournalReminder.minutesKey) private var journalReminderMinutes = 8 * 60
  @State private var showHealth = false
  @State private var showCoach = false
  @State private var showJournal = false
  @State private var showBackupSync = false
  @State private var showAllSettings = false
  @State private var showDiagnostics = false
  @State private var exporting = false
  @State private var exportURL: URL?
  @State private var exportError: String?

  var body: some View {
    NavigationStack {
      ScrollView {
        VStack(alignment: .leading, spacing: AuraDesign.sectionGap) {
          profileSection
          unitsSection
          deviceSection
          personalSection
          batterySection
          systemHealthSection
          workoutHapticsSection
          notificationsSection
          appearanceSection
          dataSection
          moreSection
          about
        }
        .padding(.horizontal, AuraDesign.screenMargin)
        .padding(.top, 6)
        .padding(.bottom, 48)
      }
      .scrollIndicators(.hidden)
      .auraScreen()
      .safeAreaInset(edge: .top) { bar }
      .sheet(isPresented: $showPairing) { AuraPairingView() }
      .sheet(isPresented: $showMigrate) { AuraMigrateView() }
      .sheet(isPresented: $showHealth) { AuraHealthView() }
      .sheet(isPresented: $showCoach) { AuraCoachView() }
      .sheet(isPresented: $showJournal) { AuraJournalView() }
      .sheet(isPresented: $showDiagnostics) { AuraDiagnosticsView() }
      .sheet(isPresented: $showBackupSync) {
        NavigationStack {
          BackupSyncView()
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
              ToolbarItem(placement: .topBarTrailing) {
                Button("Done") { showBackupSync = false }
              }
            }
        }
      }
      .sheet(isPresented: $showAllSettings) {
        NavigationStack {
          SettingsView()
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
              ToolbarItem(placement: .topBarTrailing) {
                Button("Done") { showAllSettings = false }
              }
            }
        }
      }
    }
    // A sheet is its own presentation and doesn't reliably re-inherit the app root's
    // `.preferredColorScheme` when the theme flips WHILE it's open (a known SwiftUI
    // asymmetry — dark→light stuck until reopened). Owning the scheme here, keyed on
    // the same @AppStorage the toggle writes, makes the flip land live in both directions.
    .preferredColorScheme(AppearanceMode.resolve(appearanceRaw).colorScheme)
  }

  private var bar: some View {
    HStack {
      Text("Settings").font(AuraDesign.heading(20)).foregroundStyle(AuraDesign.ink)
      Spacer()
      Button { dismiss() } label: {
        Image(systemName: "xmark")
          .font(.system(size: 15, weight: .bold)).foregroundStyle(AuraDesign.ink)
          .frame(width: 40, height: 40)
          .background(.ultraThinMaterial, in: Circle())
          .contentShape(Circle())
      }
      .buttonStyle(.plain)
    }
    .padding(.horizontal, AuraDesign.screenMargin)
    .padding(.top, 10).padding(.bottom, 8)
  }

  private func group<Content: View>(_ title: String, @ViewBuilder _ content: () -> Content) -> some View {
    VStack(alignment: .leading, spacing: 12) {
      AuraSectionHeader(title: title)
      VStack(spacing: 0) { content() }
        .padding(.vertical, 4)
        .background(AuraDesign.card, in: AuraDesign.tileShape)
        .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
    }
  }

  private var divider: some View {
    Rectangle().fill(AuraDesign.ink.opacity(0.08)).frame(height: 1).padding(.leading, 18)
  }

  // MARK: Profile

  private var profileSection: some View {
    group("Profile") {
      stepperRow("Age", value: "\(profile.age)",
                 dec: { profile.age = max(13, profile.age - 1) },
                 inc: { profile.age = min(100, profile.age + 1) })
      divider
      sexRow
      divider
      stepperRow("Weight", value: String(format: "%.0f kg", profile.weightKg),
                 dec: { profile.weightKg = max(30, profile.weightKg - 1) },
                 inc: { profile.weightKg = min(250, profile.weightKg + 1) })
      divider
      stepperRow("Height", value: String(format: "%.0f cm", profile.heightCm),
                 dec: { profile.heightCm = max(120, profile.heightCm - 1) },
                 inc: { profile.heightCm = min(230, profile.heightCm + 1) })
      divider
      stepperRow("Max HR", value: profile.hrMaxOverride > 0 ? "\(profile.hrMaxOverride)" : "auto",
                 dec: { profile.hrMaxOverride = max(0, profile.hrMax - 1) },
                 inc: { profile.hrMaxOverride = min(230, profile.hrMax + 1) })
    }
  }

  private func stepperRow(_ label: String, value: String,
                          dec: @escaping () -> Void, inc: @escaping () -> Void) -> some View {
    HStack(spacing: 12) {
      Text(label).font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
      Spacer()
      Text(value).font(AuraDesign.number(18)).foregroundStyle(AuraDesign.ink).monospacedDigit()
      HStack(spacing: 0) {
        Button(action: dec) {
          Image(systemName: "minus")
            .font(.system(size: 13, weight: .semibold)).foregroundStyle(AuraDesign.ink)
            .frame(width: 34, height: 30).contentShape(Rectangle())
        }
        Rectangle().fill(AuraDesign.ink.opacity(0.15)).frame(width: 1, height: 16)
        Button(action: inc) {
          Image(systemName: "plus")
            .font(.system(size: 13, weight: .semibold)).foregroundStyle(AuraDesign.ink)
            .frame(width: 34, height: 30).contentShape(Rectangle())
        }
      }
      .buttonStyle(.plain)
      .background(AuraDesign.ink.opacity(0.08), in: Capsule())
    }
    .padding(.horizontal, 18).padding(.vertical, 11)
  }

  private var sexRow: some View {
    HStack(spacing: 12) {
      Text("Sex").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
      Spacer()
      HStack(spacing: 4) {
        ForEach(["male", "female", "nonbinary"], id: \.self) { s in
          let active = profile.sex == s
          Button { profile.sex = s } label: {
            Text(s == "nonbinary" ? "NB" : s.prefix(1).uppercased() + s.dropFirst())
              .font(AuraDesign.caption)
              .foregroundStyle(active ? Color.black : AuraDesign.ink.opacity(0.65))
              .padding(.horizontal, 12).padding(.vertical, 6)
              .background(active ? AuraDesign.accent : .clear, in: Capsule())
              .contentShape(Capsule())
          }
          .buttonStyle(.plain)
        }
      }
      .padding(3)
      .background(AuraDesign.ink.opacity(0.08), in: Capsule())
    }
    .padding(.horizontal, 18).padding(.vertical, 11)
  }

  // MARK: Device

  private var deviceSection: some View {
    group("Device") {
      AuraInfoRow(label: "Wearable", value: live.advertisingName ?? "Not connected")
      divider
      AuraInfoRow(label: "Status", value: live.bonded ? "Paired · encrypted" : "Not paired")
      divider
      AuraInfoRow(label: "Battery",
                  value: live.batteryPct.map { "\(Int($0.rounded()))%\(live.charging == true ? " ⚡" : "")" } ?? "--")
      divider
      AuraNavRow(icon: "dot.radiowaves.left.and.right", title: "Find my strap",
                 detail: "Buzz it", tint: AuraDesign.accentInk) { model.buzz(loops: 2) }
      divider
      AuraNavRow(icon: "badge.plus.radiowaves.right",
                 title: live.bonded ? "Re-pair strap" : "Pair a strap",
                 tint: AuraDesign.ink.opacity(0.85)) { showPairing = true }
    }
  }

  // MARK: Coach + Journal (the old "More" leftovers, never homeless)

  private var personalSection: some View {
    group("Personal") {
      AuraNavRow(icon: "sparkles", title: "Coach",
                 detail: "Private, bring-your-own-key",
                 tint: AuraDesign.Family.effort.glow) { showCoach = true }
      divider
      AuraNavRow(icon: "book.closed", title: "Journal history",
                 detail: "Behaviours → recovery",
                 tint: AuraDesign.Family.energy.glow) { showJournal = true }
    }
  }

  // MARK: System health (Android parity: master switch + status · Sync status & log · System permissions)

  private var healthStatusDetail: String {
    switch health.auth {
    case .authorized: "Apple Health fills gaps when the wearable is off-wrist. Direct telemetry always wins and is never overwritten."
    case .denied: "Access denied. Grant it under Settings › Health › Data Access, or tap to review."
    case .unavailable: "Apple Health isn't available on this device."
    case .entitlementMissing: "This build can't talk to Apple Health (missing entitlement)."
    case .unknown: "Nothing is shared until you connect."
    }
  }

  private var batterySection: some View {
    group("Battery") {
      HStack(spacing: 12) {
        VStack(alignment: .leading, spacing: 2) {
          Text("Battery saver").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Text("Syncs history less often and drops device diagnostics to save strap and phone battery. Live heart rate keeps working.")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
            .fixedSize(horizontal: false, vertical: true)
        }
        Spacer()
        Toggle("", isOn: Binding(
          get: { lowPowerOn },
          set: { on in
            lowPowerOn = on
            repo.setLowPower(on)
          }))
          .labelsHidden()
          .tint(AuraDesign.Family.heart.glow)
      }
      .padding(.horizontal, 18).padding(.vertical, 11)
    }
  }

  private var systemHealthSection: some View {
    group("System health") {
      HStack(spacing: 12) {
        VStack(alignment: .leading, spacing: 2) {
          Text("System health sync").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Text(healthStatusDetail).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
            .fixedSize(horizontal: false, vertical: true)
        }
        Spacer()
        Toggle("", isOn: Binding(
          get: { health.auth == .authorized },
          set: { on in
            if on {
              Task { await health.requestAuthorization() }
            } else {
              // HealthKit grants can't be revoked from inside the app (Apple's own restriction) — send
              // the user to the sync/status sheet, which surfaces the "Open Health settings" path.
              showHealth = true
            }
          }))
          .labelsHidden()
          .disabled(health.auth == .unavailable || health.auth == .entitlementMissing)
          .tint(AuraDesign.Family.heart.glow)
      }
      .padding(.horizontal, 18).padding(.vertical, 11)
      divider
      AuraNavRow(icon: "heart.text.square", title: "Sync status & log",
                 detail: "Imports · write-back",
                 tint: AuraDesign.Family.heart.glow) { showHealth = true }
      divider
      AuraNavRow(icon: "gearshape", title: "System permissions",
                 detail: "Apple Health") {
        #if canImport(UIKit)
        // Same deep link AuraHealthView's "Open Health app" row already uses — there is no direct URL
        // scheme into Settings › Health › Data Access, so this opens the Health app itself.
        if let url = URL(string: "x-apple-health://") {
          UIApplication.shared.open(url)
        }
        #endif
      }
    }
  }

  // MARK: Units

  @AppStorage(UnitPrefs.systemKey) private var unitSystemRawSetting = UnitSystem.metric.rawValue

  private var unitsSection: some View {
    group("Units") {
      HStack(spacing: 12) {
        VStack(alignment: .leading, spacing: 2) {
          Text("Measurements").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Text(unitSystemRawSetting == UnitSystem.imperial.rawValue ? "lb · miles" : "kg · kilometres")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
        }
        Spacer()
        HStack(spacing: 4) {
          ForEach(UnitSystem.allCases) { system in
            let active = unitSystemRawSetting == system.rawValue
            Button { unitSystemRawSetting = system.rawValue } label: {
              Text(system == .imperial ? "Imperial" : "Metric")
                .font(AuraDesign.caption)
                .foregroundStyle(active ? Color.black : AuraDesign.ink.opacity(0.65))
                .padding(.horizontal, 12).padding(.vertical, 6)
                .background(active ? AnyShapeStyle(AuraDesign.accent) : AnyShapeStyle(.clear), in: Capsule())
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
          }
        }
        .padding(3)
        .background(AuraDesign.ink.opacity(0.08), in: Capsule())
      }
      .padding(.horizontal, 18).padding(.vertical, 11)
    }
  }

  // MARK: Workout haptics (§4.3 / §5 / §3.7)

  @AppStorage(WorkoutPrefs.distanceEveryKey) private var milestoneDistanceEvery = 1.0
  @AppStorage(WorkoutPrefs.timeModeKey) private var milestoneTimeMode = TimeMilestoneMode.halfway.rawValue
  @AppStorage(WorkoutPrefs.calorieModeKey) private var milestoneCalorieMode = CalorieMilestoneMode.halfway.rawValue
  @AppStorage(WorkoutPrefs.restSecondsKey) private var strengthRestDefault = 90
  @AppStorage(UnitPrefs.systemKey) private var unitSystemRaw = UnitSystem.metric.rawValue

  private var workoutHapticsSection: some View {
    let distanceUnit = unitSystemRaw == UnitSystem.imperial.rawValue ? "mi" : "km"
    return group("Workout haptics") {
      sectionCaption("Zone alerts", "Buzz count = the zone. Z3 taps 3×, Z5 taps 5×.")
      AuraZoneAlertRows(behavior: model.behavior)
      divider
      sectionCaption("Goal milestones", "A light tap at each mark; a strong buzz at your goal.")
      divider
      menuRow("Distance") {
        ForEach([0.5, 1.0, 2.0, 5.0], id: \.self) { v in
          Button { milestoneDistanceEvery = v } label: {
            Label(distanceEveryLabel(v, unit: distanceUnit),
                  systemImage: milestoneDistanceEvery == v ? "checkmark" : "")
          }
        }
      } label: {
        distanceEveryLabel(milestoneDistanceEvery, unit: distanceUnit)
      }
      divider
      menuRow("Time") {
        ForEach(TimeMilestoneMode.allCases) { mode in
          Button { milestoneTimeMode = mode.rawValue } label: {
            Label(mode.label, systemImage: milestoneTimeMode == mode.rawValue ? "checkmark" : "")
          }
        }
      } label: {
        (TimeMilestoneMode(rawValue: milestoneTimeMode) ?? .halfway).label
      }
      divider
      menuRow("Calories") {
        ForEach(CalorieMilestoneMode.allCases) { mode in
          Button { milestoneCalorieMode = mode.rawValue } label: {
            Label(mode.label, systemImage: milestoneCalorieMode == mode.rawValue ? "checkmark" : "")
          }
        }
      } label: {
        (CalorieMilestoneMode(rawValue: milestoneCalorieMode) ?? .halfway).label
      }
      divider
      menuRow("Rest timer") {
        ForEach([30, 45, 60, 90, 120, 150, 180, 240, 300], id: \.self) { s in
          Button { strengthRestDefault = s } label: {
            Label(restLabel(s), systemImage: strengthRestDefault == s ? "checkmark" : "")
          }
        }
      } label: {
        restLabel(strengthRestDefault)
      }
    }
  }

  /// A compact "label + one-line why" caption row that heads a cluster of controls,
  /// so the individual rows below can stay a single tight line each.
  private func sectionCaption(_ title: String, _ why: String) -> some View {
    VStack(alignment: .leading, spacing: 2) {
      Text(title).font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
      Text(why).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
        .fixedSize(horizontal: false, vertical: true)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(.horizontal, 18).padding(.vertical, 10)
  }

  private func distanceEveryLabel(_ v: Double, unit: String) -> String {
    v == v.rounded() ? "Every \(Int(v)) \(unit)" : String(format: "Every %.1f %@", v, unit)
  }

  private func restLabel(_ s: Int) -> String {
    s % 60 == 0 ? "\(s / 60) min" : "\(s / 60):\(String(format: "%02d", s % 60)) min"
  }

  /// The five per-zone alert pickers. A standalone view observing `BehaviorStore`
  /// directly — a nested-object edit doesn't republish through `AppModel`, so the
  /// parent sheet alone would render stale mode labels.
  private struct AuraZoneAlertRows: View {
    @ObservedObject var behavior: BehaviorStore

    var body: some View {
      ForEach(0..<5, id: \.self) { i in
        Rectangle().fill(AuraDesign.ink.opacity(0.08)).frame(height: 1).padding(.leading, 18)
        HStack(spacing: 12) {
          Circle().fill(AuraZoneBars.tints[i]).frame(width: 9, height: 9)
          Text("Zone \(i + 1)").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Spacer()
          Menu {
            ForEach(ZoneAlertMode.allCases) { mode in
              Button {
                var modes = behavior.zoneAlertModes
                if modes.indices.contains(i) { modes[i] = mode }
                behavior.zoneAlertModes = modes
              } label: {
                Label(mode.label,
                      systemImage: behavior.zoneAlertModes[safe: i] == mode ? "checkmark" : "")
              }
            }
          } label: {
            Text((behavior.zoneAlertModes[safe: i] ?? .off).label)
              .font(AuraDesign.sub)
              .foregroundStyle((behavior.zoneAlertModes[safe: i] ?? .off) == .off
                               ? AuraDesign.ink.opacity(0.45) : AuraDesign.accentInk)
              .padding(.horizontal, 12).padding(.vertical, 6)
              .background(AuraDesign.ink.opacity(0.08), in: Capsule())
              .contentShape(Capsule())
          }
        }
        .padding(.horizontal, 18).padding(.vertical, 8)
      }
    }
  }

  private func menuRow(_ title: String, detail: String = "",
                       @ViewBuilder items: @escaping () -> some View,
                       label: () -> String) -> some View {
    HStack(spacing: 12) {
      VStack(alignment: .leading, spacing: 2) {
        Text(title).font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
        if !detail.isEmpty {
          Text(detail).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
        }
      }
      Spacer()
      Menu {
        items()
      } label: {
        Text(label())
          .font(AuraDesign.sub).foregroundStyle(AuraDesign.accentInk)
          .padding(.horizontal, 12).padding(.vertical, 6)
          .background(AuraDesign.ink.opacity(0.08), in: Capsule())
          .contentShape(Capsule())
      }
    }
    .padding(.horizontal, 18).padding(.vertical, 11)
  }

  // MARK: Notifications

  private var notificationsSection: some View {
    group("Notifications") {
      HStack(spacing: 12) {
        VStack(alignment: .leading, spacing: 2) {
          Text("Morning check-in").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Text("A daily nudge with a one-tap \"Log how you feel\" action.")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
        }
        Spacer()
        Toggle("", isOn: Binding(
          get: { journalReminderOn },
          set: { on in
            journalReminderOn = on
            JournalReminder.apply(enabled: on, minutes: journalReminderMinutes)
          }))
          .labelsHidden()
          .tint(AuraDesign.Family.energy.glow)
      }
      .padding(.horizontal, 18).padding(.vertical, 11)
      if journalReminderOn {
        divider
        HStack(spacing: 12) {
          Text("Time").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Spacer()
          DatePicker("", selection: Binding(
            get: {
              Calendar.current.date(bySettingHour: journalReminderMinutes / 60,
                                    minute: journalReminderMinutes % 60,
                                    second: 0, of: Date()) ?? Date()
            },
            set: { date in
              let c = Calendar.current.dateComponents([.hour, .minute], from: date)
              journalReminderMinutes = (c.hour ?? 8) * 60 + (c.minute ?? 0)
              JournalReminder.apply(enabled: true, minutes: journalReminderMinutes)
            }), displayedComponents: .hourAndMinute)
            .labelsHidden()
        }
        .padding(.horizontal, 18).padding(.vertical, 7)
      }
    }
  }

  // MARK: Appearance

  private var appearanceSection: some View {
    group("Appearance") {
      HStack(spacing: 12) {
        Text("Theme").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
        Spacer()
        HStack(spacing: 4) {
          ForEach(AppearanceMode.allCases) { mode in
            let active = appearanceRaw == mode.rawValue
            Button { appearanceRaw = mode.rawValue } label: {
              Text(mode.label)
                .font(AuraDesign.caption)
                .foregroundStyle(active ? Color.black : AuraDesign.ink.opacity(0.65))
                .padding(.horizontal, 12).padding(.vertical, 6)
                .background(active ? AuraDesign.accent : .clear, in: Capsule())
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
          }
        }
        .padding(3)
        .background(AuraDesign.ink.opacity(0.08), in: Capsule())
      }
      .padding(.horizontal, 18).padding(.vertical, 11)
    }
  }

  // MARK: Data

  private var dataSection: some View {
    group("Data") {
      if let url = exportURL {
        ShareLink(item: url) {
          HStack(spacing: 14) {
            Image(systemName: "square.and.arrow.up")
              .font(.system(size: 16, weight: .medium)).foregroundStyle(AuraDesign.accentInk)
              .frame(width: 26)
            Text("Share exported CSV").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
            Spacer()
          }
          .padding(.horizontal, 18).padding(.vertical, 15)
          .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
      } else {
        AuraNavRow(icon: "square.and.arrow.up", title: "Export CSV",
                   detail: exporting ? "Exporting…" : (exportError ?? "Portable archive"),
                   tint: AuraDesign.ink.opacity(0.85)) {
          guard !exporting else { return }
          exporting = true; exportError = nil
          Task {
            switch await CsvExport.run(repo: repo) {
            case .exported(let url): exportURL = url
            case .cancelled: break
            case .failure(let msg): exportError = msg
            }
            exporting = false
          }
        }
      }
      divider
      AuraNavRow(icon: "shippingbox.and.arrow.backward", title: "Migrate from the original app",
                 detail: "Export zip", tint: AuraDesign.Family.charge.glow) { showMigrate = true }
      divider
      AuraNavRow(icon: "icloud.and.arrow.up", title: "Backup & Sync",
                 detail: "Your folder, your files") { showBackupSync = true }
      divider
      HStack(spacing: 12) {
        VStack(alignment: .leading, spacing: 2) {
          // NOTE: kept iOS's own copy here (not Android's "Encrypt backups at rest" wording) —
          // this toggle and Android's are NOT functionally equivalent. iOS's `AuraDataProtection`
          // (StrandiOS/System/AuraPrivacyAndReminders.swift) stamps NSFileProtectionCompleteUnlessOpen
          // on the WHOLE store directory (the live DB + WAL/SHM), while Android's `AuraDataProtection`
          // (android/.../data/AuraDataProtection.kt) only wraps `.mavbak` BACKUP snapshots in an
          // AES/GCM envelope and never touches the live database. Relabelling iOS to "backups at rest"
          // would misdescribe what the toggle protects. Flagged in the parity report rather than
          // silently reconciled — a real behavioural difference, not just copy drift.
          Text("Encrypt data at rest").font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Text("Optional hardware encryption. Off keeps exported logs readable when you share feedback.")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
            .fixedSize(horizontal: false, vertical: true)
        }
        Spacer()
        Toggle("", isOn: Binding(
          get: { fileProtectionOn },
          set: { on in
            fileProtectionOn = on
            AuraDataProtection.apply(on)
          }))
          .labelsHidden()
          .tint(AuraDesign.Family.vitals.glow)
      }
      .padding(.horizontal, 18).padding(.vertical, 11)
      divider
      AuraNavRow(icon: "internaldrive", title: "Storage & diagnostics",
                 detail: "DB size · integrity") { showDiagnostics = true }
    }
  }

  // MARK: More (everything without an Aura port yet)

  private var moreSection: some View {
    group("More") {
      AuraNavRow(icon: "gearshape", title: "All settings",
                 detail: "Alarms · automations · units · debug") { showAllSettings = true }
    }
  }

  // MARK: About

  private var about: some View {
    VStack(alignment: .leading, spacing: 10) {
      Text("Maverick").font(AuraDesign.heading(17)).foregroundStyle(AuraDesign.ink)
      Text("Connects directly to approved wearables over Bluetooth. No account, no cloud, nothing ever leaves this device.")
        .font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.55))
        .fixedSize(horizontal: false, vertical: true)
    }
    .padding(.horizontal, 4)
  }
}
