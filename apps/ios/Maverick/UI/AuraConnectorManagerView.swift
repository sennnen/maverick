import SwiftUI
import UniformTypeIdentifiers

extension UTType {
  static let maverickConnector = UTType(importedAs: "dev.maverick.connector", conformingTo: .data)
}

struct AuraPairingView: View {
  @EnvironmentObject private var connectors: ConnectorManager
  @Environment(\.dismiss) private var dismiss
  @State private var showingImporter = false
  @State private var remoteURL = ""

  var body: some View {
    NavigationStack {
      ScrollView {
        VStack(alignment: .leading, spacing: AuraDesign.sectionGap) {
          intro
          acquisition
          status
          installed
          releasePolicy
        }
        .padding(.horizontal, AuraDesign.screenMargin)
        .padding(.top, 12)
        .padding(.bottom, 44)
      }
      .scrollIndicators(.hidden)
      .background(AuraDesign.bg.ignoresSafeArea())
      .navigationTitle("Device connectors")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .topBarTrailing) { Button("Done") { dismiss() } }
      }
      .fileImporter(
        isPresented: $showingImporter,
        allowedContentTypes: [.maverickConnector],
        allowsMultipleSelection: false
      ) { result in
        switch result {
        case let .success(urls):
          if let url = urls.first { connectors.importFile(url) }
        case let .failure(error):
          connectors.reportAcquisitionFailure(error)
        }
      }
    }
  }

  private var intro: some View {
    VStack(alignment: .leading, spacing: 10) {
      Image(systemName: "sensor.tag.radiowaves.forward")
        .font(.system(size: 26, weight: .medium))
        .foregroundStyle(AuraDesign.Family.heart.glow)
      Text("Add the signed connector for your wearable, then approve exactly what it can access.")
        .font(AuraDesign.sub)
        .foregroundStyle(AuraDesign.ink.opacity(0.72))
        .fixedSize(horizontal: false, vertical: true)
      Label("Runs locally. No account or cloud upload.", systemImage: "lock.shield")
        .font(AuraDesign.caption)
        .foregroundStyle(AuraDesign.ink.opacity(0.55))
    }
  }

  private var acquisition: some View {
    VStack(alignment: .leading, spacing: 12) {
      AuraSectionHeader(title: "Import")
      Button { showingImporter = true } label: {
        Label("Choose .mavconn file", systemImage: "doc.badge.plus")
          .frame(maxWidth: .infinity, minHeight: 48)
      }
      .buttonStyle(.borderedProminent)
      .tint(AuraDesign.accent)
      .foregroundStyle(Color.black)
      .disabled(!connectors.releasePolicy.managerEnabled)

      HStack(spacing: 10) {
        TextField("https://…/connector.mavconn", text: $remoteURL)
          .textInputAutocapitalization(.never)
          .keyboardType(.URL)
          .autocorrectionDisabled()
          .padding(.horizontal, 14)
          .frame(minHeight: 48)
          .background(AuraDesign.card, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
          .accessibilityLabel("Connector HTTPS URL")
        Button("Inspect") {
          guard let url = URL(string: remoteURL) else {
            connectors.reportAcquisitionFailure(ConnectorAcquisitionError.unsupportedURL)
            return
          }
          connectors.importRemote(url)
        }
        .buttonStyle(.bordered)
        .frame(minHeight: 44)
        .disabled(!connectors.releasePolicy.remoteImportEnabled)
      }
    }
  }

  @ViewBuilder
  private var status: some View {
    switch connectors.phase {
    case .idle:
      EmptyView()
    case .inspecting:
      statusCard(icon: "checkmark.shield", title: "Inspecting signature…", body: "No connector code runs before approval.") {
        ProgressView().tint(AuraDesign.accent)
      }
    case let .awaitingApproval(summary):
      approvalCard(summary)
    case let .installing(summary):
      statusCard(icon: "shippingbox", title: "Installing \(summary.displayName)…", body: "The signed artifact is being committed atomically.") {
        ProgressView().tint(AuraDesign.accent)
      }
    case let .installed(id):
      statusCard(icon: "checkmark.seal.fill", title: "Connector installed", body: id) { EmptyView() }
    case let .failed(message):
      statusCard(icon: "exclamationmark.triangle", title: "Couldn’t use connector", body: message) {
        Button("Dismiss") { connectors.cancel() }.buttonStyle(.bordered)
      }
    case let .rolledBack(id):
      statusCard(icon: "arrow.uturn.backward.circle", title: "Rolled back safely", body: id) { EmptyView() }
    case let .revoked(id):
      statusCard(icon: "hand.raised.slash", title: "Connector disabled", body: "\(id) is no longer trusted.") { EmptyView() }
    }
  }

  private func approvalCard(_ summary: ConnectorApprovalSummary) -> some View {
    VStack(alignment: .leading, spacing: 14) {
      Label("Review before installing", systemImage: "checkmark.shield")
        .font(AuraDesign.heading(19)).foregroundStyle(AuraDesign.ink)
      Text(summary.displayName).font(AuraDesign.heading(24)).foregroundStyle(AuraDesign.ink)
      if !summary.detail.isEmpty {
        Text(summary.detail).font(AuraDesign.sub).foregroundStyle(AuraDesign.ink.opacity(0.68))
      }
      approvalRow("Publisher", summary.publisherKeyID)
      approvalRow("Version", summary.version)
      approvalRow("Source", summary.sourceName)
      approvalRow("Self-tests", "\(summary.fixtureCount) verified fixtures")
      if !summary.permissions.isEmpty {
        Text("Permissions").font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.5))
        ForEach(summary.permissions, id: \.self) { permission in
          Label(permission, systemImage: "checkmark.circle")
            .font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.82))
        }
      }
      if !summary.capabilities.isEmpty {
        Text("Data streams: \(summary.capabilities.joined(separator: ", "))")
          .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.58))
          .fixedSize(horizontal: false, vertical: true)
      }
      HStack(spacing: 12) {
        Button("Cancel") { connectors.cancel() }
          .buttonStyle(.bordered)
          .frame(maxWidth: .infinity, minHeight: 46)
        Button("Approve & install") { connectors.approve() }
          .buttonStyle(.borderedProminent)
          .tint(AuraDesign.accent)
          .foregroundStyle(Color.black)
          .frame(maxWidth: .infinity, minHeight: 46)
      }
    }
    .padding(20)
    .background(AuraDesign.card, in: AuraDesign.tileShape)
    .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.accent.opacity(0.45), lineWidth: 1))
  }

  private func approvalRow(_ label: String, _ value: String) -> some View {
    HStack(alignment: .firstTextBaseline) {
      Text(label).foregroundStyle(AuraDesign.ink.opacity(0.5))
      Spacer()
      Text(value).foregroundStyle(AuraDesign.ink.opacity(0.84)).multilineTextAlignment(.trailing)
    }
    .font(AuraDesign.caption)
  }

  private func statusCard<Accessory: View>(
    icon: String,
    title: String,
    body: String,
    @ViewBuilder accessory: () -> Accessory
  ) -> some View {
    HStack(alignment: .top, spacing: 14) {
      Image(systemName: icon).font(.system(size: 20, weight: .semibold)).foregroundStyle(AuraDesign.accent)
      VStack(alignment: .leading, spacing: 5) {
        Text(title).font(AuraDesign.label).foregroundStyle(AuraDesign.ink)
        Text(body).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.62))
          .fixedSize(horizontal: false, vertical: true)
      }
      Spacer()
      accessory()
    }
    .padding(18)
    .background(AuraDesign.card, in: AuraDesign.tileShape)
    .accessibilityElement(children: .combine)
  }

  @ViewBuilder
  private var installed: some View {
    if !connectors.installed.isEmpty {
      VStack(alignment: .leading, spacing: 12) {
        AuraSectionHeader(title: "Installed")
        ForEach(connectors.installed, id: \.artifactDigest) { record in
          VStack(alignment: .leading, spacing: 10) {
            HStack {
              VStack(alignment: .leading, spacing: 3) {
                Text(record.connectorId).font(AuraDesign.label).foregroundStyle(AuraDesign.ink)
                Text("v\(record.version) · \(record.publisherKeyId)")
                  .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.52))
              }
              Spacer()
              Text(record.disabledReason == nil ? (record.active ? "Active" : "Installed") : "Disabled")
                .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.64))
            }
            if let reason = record.disabledReason {
              Text(reason).font(AuraDesign.caption).foregroundStyle(Color.orange)
            }
            HStack {
              Button(record.active ? "Connect" : "Activate first") { connectors.connect(record) }
                .buttonStyle(.borderedProminent)
                .tint(AuraDesign.accent)
                .foregroundStyle(Color.black)
                .disabled(!record.active || record.disabledReason != nil)
              Button("Roll back") { connectors.rollback(record.connectorId) }
                .buttonStyle(.bordered).disabled(!record.active)
              Button("Remove", role: .destructive) { connectors.remove(record) }.buttonStyle(.bordered)
            }
          }
          .padding(18)
          .background(AuraDesign.card, in: AuraDesign.tileShape)
        }
      }
    }
  }

  private var releasePolicy: some View {
    VStack(alignment: .leading, spacing: 8) {
      Label("Release trust policy", systemImage: "checkmark.shield")
        .font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.78))
      Text("Only official publisher keys configured by this release are accepted. File paths and URLs are reduced to one-way digests before entering the core.")
        .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.5))
        .fixedSize(horizontal: false, vertical: true)
    }
    .padding(.horizontal, 4)
  }
}
