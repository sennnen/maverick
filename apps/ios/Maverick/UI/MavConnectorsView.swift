import SwiftUI

// Connector management, reachable from exactly one row — in the device sheet.
//
// The approval card is a security surface. This lane restyles it and does not alter a single fact
// it states: the connector id, the version, the publisher key, the capabilities it asked for, and
// the permissions it wants are all shown before anyone can approve, exactly as before.

struct MavConnectorsView: View {
  @EnvironmentObject private var connectors: ConnectorManager
  @State private var showImporter = false

  var body: some View {
    MavDetailScaffold(title: "Connectors") {
      switch connectors.machine.phase {
      case .idle:
        idle
      case .inspecting:
        MavTile {
          HStack(spacing: 12) {
            ProgressView()
            Text("Inspecting the artifact").mavType(.body).foregroundStyle(MavTheme.ink)
          }
        }
      case .awaitingApproval(let summary):
        approval(summary)
      case .installing(let summary):
        MavTile {
          HStack(spacing: 12) {
            ProgressView()
            Text("Installing \(summary.displayName)").mavType(.body).foregroundStyle(MavTheme.ink)
          }
        }
      case .installed(let id):
        outcome(title: "Installed", detail: id)
      case .failed(let message):
        outcome(title: "Import failed", detail: message)
      case .rolledBack(let id):
        outcome(title: "Rolled back", detail: id)
      case .revoked(let id):
        outcome(title: "Revoked", detail: id)
      }
    }
    .fileImporter(isPresented: $showImporter, allowedContentTypes: [.data]) { result in
      switch result {
      case .success(let url): connectors.importFile(url, origin: .file)
      case .failure(let error): connectors.reportAcquisitionFailure(error)
      }
    }
  }

  // MARK: Idle

  @ViewBuilder private var idle: some View {
    MavTile {
      VStack(alignment: .leading, spacing: 7) {
        Text("Signed drivers").mavType(.title).foregroundStyle(MavTheme.ink)
        Text(
          "A connector lets Maverick read one wearable. Every connector is signed and installed "
          + "only after you approve it."
        )
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)
        .fixedSize(horizontal: false, vertical: true)
      }
    }

    MavPrimaryButton(title: "Import a connector", detail: "From a file", systemImage: "square.and.arrow.down") {
      showImporter = true
    }

    MavSectionHeader(title: "Installed")
    if connectors.installed.isEmpty {
      Text("No connectors installed.")
        .mavType(.body)
        .foregroundStyle(MavTheme.inkSecondary)
        .padding(.vertical, 8)
    } else {
      VStack(spacing: 0) {
        ForEach(Array(connectors.installed.enumerated()), id: \.offset) { index, record in
          if index > 0 { MavDivider() }
          MavRow(
            title: record.connectorId,
            detail: "Version \(record.version)"
          ) {
            Menu {
              Button("Connect") { connectors.connect(record) }
              Button("Roll back") { connectors.rollback(record.connectorId) }
              Button("Remove", role: .destructive) { connectors.remove(record) }
            } label: {
              Image(systemName: "ellipsis")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(MavTheme.inkSecondary)
                .frame(width: 44, height: 44)
                .contentShape(.rect)
            }
            .accessibilityLabel("Actions for \(record.connectorId)")
          }
        }
      }
      .mavSurface(MavTheme.tileShape)
    }

    if !connectors.registryEntries.isEmpty {
      MavSectionHeader(title: "Registry")
      VStack(spacing: 0) {
        ForEach(Array(connectors.registryEntries.enumerated()), id: \.offset) { index, entry in
          if index > 0 { MavDivider() }
          Button {
            connectors.importRegistryEntry(entry)
          } label: {
            MavRow(title: entry.connectorId, detail: "Version \(entry.version)") {
              Text("Install").mavType(.label).foregroundStyle(MavTheme.accent)
            }
          }
          .buttonStyle(.plain)
        }
      }
      .mavSurface(MavTheme.tileShape)
    }

    if let error = connectors.registryError {
      MavUnavailableCard(name: "Registry", reason: error)
    }
  }

  // MARK: Approval

  private func approval(_ summary: ConnectorApprovalSummary) -> some View {
    VStack(alignment: .leading, spacing: MavTheme.cardSpacing) {
      MavStatusCard {
        VStack(alignment: .leading, spacing: 9) {
          Text("Approve this connector?").mavType(.title).foregroundStyle(MavTheme.ink)
          Text(summary.displayName).mavType(.title).foregroundStyle(MavTheme.ink)
          Text(
            "\(summary.connectorID) · version \(summary.version)\n"
            + "Signed by \(summary.publisherKeyID)"
            + (summary.sourceName.isEmpty ? "" : "\nFrom \(summary.sourceName)")
          )
          .mavType(.sub)
          .foregroundStyle(MavTheme.inkSecondary)
          .fixedSize(horizontal: false, vertical: true)
          if !summary.detail.isEmpty {
            Text(summary.detail)
              .mavType(.body)
              .foregroundStyle(MavTheme.inkSecondary)
              .fixedSize(horizontal: false, vertical: true)
          }
        }
      }

      if !summary.capabilities.isEmpty {
        MavSectionHeader(title: "It will be able to")
        MavTile {
          MavWrap(items: summary.capabilities) { MavChip(text: $0) }
        }
      }

      if !summary.permissions.isEmpty {
        MavSectionHeader(title: "It is asking for")
        MavTile {
          MavWrap(items: summary.permissions) { MavChip(text: $0) }
        }
      }

      MavTile {
        Text(
          "\(summary.fixtureCount) golden fixture\(summary.fixtureCount == 1 ? "" : "s") shipped "
          + "with this artifact. Approving grants exactly what is listed above and nothing else."
        )
        .mavType(.sub)
        .foregroundStyle(MavTheme.inkSecondary)
        .fixedSize(horizontal: false, vertical: true)
      }

      HStack(spacing: 10) {
        MavWideButton(title: "Cancel") { connectors.cancel() }
        MavWideButton(title: "Approve") { connectors.approve() }
      }
      .padding(.top, 8)
    }
  }

  private func outcome(title: String, detail: String) -> some View {
    VStack(alignment: .leading, spacing: MavTheme.cardSpacing) {
      MavStatusCard {
        VStack(alignment: .leading, spacing: 7) {
          Text(title).mavType(.title).foregroundStyle(MavTheme.ink)
          Text(detail)
            .mavType(.body)
            .foregroundStyle(MavTheme.inkSecondary)
            .fixedSize(horizontal: false, vertical: true)
        }
      }
      MavWideButton(title: "Done") { connectors.cancel() }
    }
  }
}

/// A flowing row of chips. `LazyVGrid` cannot do this without fixed columns, and a capability list
/// has no fixed width.
struct MavWrap<Item: Hashable, Content: View>: View {
  let items: [Item]
  @ViewBuilder let content: (Item) -> Content

  var body: some View {
    FlowLayout(spacing: 7) {
      ForEach(items, id: \.self) { content($0) }
    }
  }
}

struct FlowLayout: Layout {
  var spacing: CGFloat = 7

  func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
    let width = proposal.width ?? .infinity
    var x: CGFloat = 0
    var y: CGFloat = 0
    var rowHeight: CGFloat = 0
    for subview in subviews {
      let size = subview.sizeThatFits(.unspecified)
      if x + size.width > width, x > 0 {
        x = 0
        y += rowHeight + spacing
        rowHeight = 0
      }
      x += size.width + spacing
      rowHeight = max(rowHeight, size.height)
    }
    return CGSize(width: proposal.width ?? x, height: y + rowHeight)
  }

  func placeSubviews(
    in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()
  ) {
    var x = bounds.minX
    var y = bounds.minY
    var rowHeight: CGFloat = 0
    for subview in subviews {
      let size = subview.sizeThatFits(.unspecified)
      if x + size.width > bounds.maxX, x > bounds.minX {
        x = bounds.minX
        y += rowHeight + spacing
        rowHeight = 0
      }
      subview.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
      x += size.width + spacing
      rowHeight = max(rowHeight, size.height)
    }
  }
}
