import SwiftUI

// The device sheet. One tap from every tab, and the *only* place in the app where device state,
// pairing, device controls, or a route to connector management exists.
//
// That exclusivity is the point of the whole lane. The old shell had connection in three places and
// connector management in two, and Settings repeated both, so nobody could tell which copy was the
// real one. Settings now holds no device row at all.

struct MavDeviceSheet: View {
  @EnvironmentObject private var connectors: ConnectorManager
  @EnvironmentObject private var live: LiveState
  @EnvironmentObject private var model: AppModel
  @EnvironmentObject private var repo: Repository
  @Environment(\.dismiss) private var dismiss
  @State private var lowPower = false

  private var connection: ConnectorConnectionState { connectors.connection }

  var body: some View {
    NavigationStack {
      ZStack {
        MavTheme.canvas.ignoresSafeArea()

        ScrollView {
          VStack(alignment: .leading, spacing: MavTheme.cardSpacing) {
            if connection.connected {
              paired
            } else {
              unpaired
            }

            MavSectionHeader(title: "Connectors")
            NavigationLink {
              MavConnectorsView()
            } label: {
              MavRow(
                title: "Manage connectors",
                detail: installedSummary
              ) {
                Image(systemName: "chevron.right")
                  .font(.system(size: 13, weight: .semibold))
                  .foregroundStyle(MavTheme.inkSecondary)
              }
            }
            .buttonStyle(.plain)
            .mavSurface(MavTheme.tileShape)

          }
          .padding(.horizontal, MavTheme.screenMargin)
          .padding(.bottom, 40)
        }
        .scrollIndicators(.hidden)
      }
      .navigationTitle("Your device")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .topBarTrailing) {
          Button("Close") { dismiss() }
        }
      }
    }
    .presentationDetents([.large])
    .onAppear { lowPower = repo.lowPowerIsOn }
  }

  private var installedSummary: String {
    let count = connectors.installed.count
    return count == 1 ? "1 installed" : "\(count) installed"
  }

  // MARK: Paired

  @ViewBuilder private var paired: some View {
    MavStatusCard {
      VStack(alignment: .leading, spacing: 5) {
        Text(live.advertisingName ?? connection.connectorID ?? "Device")
          .mavType(.title)
          .foregroundStyle(MavTheme.ink)
        Text(provenanceLine)
          .mavType(.sub)
          .foregroundStyle(MavTheme.inkSecondary)
          .fixedSize(horizontal: false, vertical: true)

        HStack(spacing: 14) {
          statCell("Battery", connection.batteryPercent.map { "\($0)%" } ?? "—")
          statCell("Wrist", connection.onWrist.map { $0 ? "On" : "Off" } ?? "—")
          statCell("Link", connection.label)
        }
        .padding(.top, 18)
      }
    }

    MavSectionHeader(title: "Live")
    MavTile {
      VStack(alignment: .leading, spacing: 4) {
        Text(connection.heartRateBPM.map { "\($0) bpm" } ?? "No sample yet")
          .mavType(.numeralMedium)
          .foregroundStyle(MavTheme.ink)
        Text(sampleAge)
          .mavType(.sub)
          .foregroundStyle(MavTheme.inkSecondary)
      }
      .accessibilityElement(children: .combine)
    }

    if let progress = model.syncProgress {
      MavSectionHeader(title: "History sync")
      MavTile {
        Text(progress).mavType(.body).foregroundStyle(MavTheme.ink)
      }
    }

    MavSectionHeader(title: "Controls")
    VStack(spacing: 0) {
      MavToggleRow(
        title: "Battery saver",
        detail: "Uses less power by syncing history less often.",
        isOn: Binding(
          get: { lowPower },
          set: { newValue in
            lowPower = newValue
            repo.setLowPower(newValue)
          }))
      // Connector-declared controls (ADR-031) render here, from `device-controls/v1`. The core
      // does not publish that block yet, so the section holds only the host-owned row rather than
      // an empty heading promising something that is not there.
    }
    .mavSurface(MavTheme.tileShape)

    HStack(spacing: 10) {
      MavWideButton(title: "Disconnect") { connectors.disconnect() }
      MavWideButton(title: "Forget device", destructive: true) { connectors.disconnect() }
    }
    .padding(.top, 12)
  }

  private var provenanceLine: String {
    connection.label
  }

  private var sampleAge: String {
    MavPresent.sampleAgeLabel(
      asOfUnixMs: Int64(Date().timeIntervalSince1970 * 1000),
      lastSampleUnixMs: connection.lastSampleWallTimeMs,
      connected: connection.connected) ?? "Streaming"
  }

  private func statCell(_ key: String, _ value: String) -> some View {
    VStack(alignment: .leading, spacing: 4) {
      Text(key).mavType(.caption).foregroundStyle(MavTheme.inkSecondary)
      Text(value).mavType(.numeralSmall).foregroundStyle(MavTheme.ink).lineLimit(1)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .accessibilityElement(children: .combine)
    .accessibilityLabel("\(key), \(value == "—" ? "no value" : value)")
  }

  // MARK: Unpaired

  @ViewBuilder private var unpaired: some View {
    MavStatusCard {
      VStack(alignment: .leading, spacing: 8) {
        Text("Nothing connected")
          .mavType(.title)
          .foregroundStyle(MavTheme.ink)
        Text(
          connectors.installed.isEmpty
            ? "No connector is installed yet. A connector is the signed driver that knows how to "
              + "talk to your strap — install one and it appears here."
            : "Choose the connector for your strap, then pair it below."
        )
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)
        .fixedSize(horizontal: false, vertical: true)
        if let error = connection.errorMessage {
          Text(error)
            .mavType(.sub)
            .foregroundStyle(MavTheme.destructiveInk())
            .fixedSize(horizontal: false, vertical: true)
            .padding(.top, 4)
        }
      }
    }

    if !connectors.installed.isEmpty {
      MavSectionHeader(title: "Connector")
      VStack(spacing: 0) {
        ForEach(Array(connectors.installed.enumerated()), id: \.offset) { index, record in
          if index > 0 { MavDivider() }
          Button {
            connectors.connect(record)
          } label: {
            MavRow(
              title: record.connectorId,
              detail: "Version \(record.version)"
            ) {
              Text("Pair")
                .mavType(.label)
                .foregroundStyle(MavTheme.accent)
            }
          }
          .buttonStyle(.plain)
          .accessibilityLabel("Pair with \(record.connectorId), version \(record.version)")
        }
      }
      .mavSurface(MavTheme.tileShape)
    }

    if !connectors.discoveredDevices.isEmpty {
      MavSectionHeader(title: "Nearby")
      VStack(spacing: 0) {
        ForEach(Array(connectors.discoveredDevices.sorted { $0.rssi > $1.rssi }.enumerated()),
                id: \.element.id) { index, device in
          if index > 0 { MavDivider() }
          Button {
            connectors.selectDevice(device.id)
          } label: {
            MavRow(title: device.name, detail: "Signal \(device.rssi) dBm") {
              Image(systemName: "chevron.right")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(MavTheme.inkSecondary)
            }
          }
          .buttonStyle(.plain)
          .accessibilityLabel("\(device.name), signal strength \(device.rssi) decibel milliwatts")
        }
      }
      .mavSurface(MavTheme.tileShape)
    }
  }
}
