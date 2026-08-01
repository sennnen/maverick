import SwiftUI

// `Vitals` — the numbers tab. One row per metric, built from the core's availability set rather
// than from a hardcoded list, so a metric no connector can supply is an honest unavailable card and
// never an empty frame.
//
// A row is a button that pushes. It does not expand in place: two disclosure mechanisms on one
// control is how the old hubs got confusing.

struct MavVitalsView: View {
  @ObservedObject var shell: MavShellState
  @EnvironmentObject private var model: AppModel
  @EnvironmentObject private var profile: ProfileStore
  @EnvironmentObject private var connectors: ConnectorManager

  private var rows: [MavMetricRow] {
    MavMetricMapper.rows(
      from: model.dailySnapshot,
      cycleEnabled: profile.tracksCycle || model.usingDebugFixture)
  }

  var body: some View {
    MavTabScroll {
      if model.usingDebugFixture {
        HStack(spacing: 9) {
          MavBadge(text: "Sample")
          Text("Fixture data — nothing is connected.")
            .mavType(.sub)
            .foregroundStyle(MavTheme.inkSecondary)
        }
        .padding(.top, 4)
        .accessibilityElement(children: .combine)
      }

      if connectors.ecgCapabilities.contains(where: { $0.stream == "ecg" })
        || !connectors.ecgResults.isEmpty
        || connectors.ecgCapture != nil
      {
        MavSectionHeader(title: "Heart")
        NavigationLink {
          MavEcgView()
        } label: {
          MavRow(
            title: "ECG",
            detail: connectors.ecgCapture.map { captureTitle($0.phase) }
              ?? connectors.ecgResults.first.map { rhythmTitle($0.rhythm) }
              ?? "30-second recording"
          ) {
            Image(systemName: "chevron.right")
              .font(.system(size: 12, weight: .semibold))
              .foregroundStyle(MavTheme.inkSecondary)
          }
        }
        .buttonStyle(.plain)
        .mavSurface(MavTheme.tileShape)
      }

      ForEach(MavMetric.Group.allCases, id: \.self) { group in
        let groupRows = rows.filter {
          $0.metric.group == group && ($0.isAvailable || $0.metric.family == .cycle)
        }
        if !groupRows.isEmpty {
          if group.rawValue != "Vitals" {
            MavSectionHeader(title: group.rawValue)
          }
          ForEach(groupRows) { row in
            MavVitalRow(row: row)
          }
        }
      }
    }
  }

  private func captureTitle(_ phase: String) -> String {
    switch phase {
    case "calibrating": "Checking signal"
    case "recording": "Recording"
    case "analysing": "Analysing"
    default: "Latest result"
    }
  }
}

/// The Oura shape. The surface wash and the band name **which metric** this is; the verdict is the
/// word in the header and the marker's position in the band. Colouring the surface by verdict was
/// the wrong idea twice over — it made a bad night look alarming before the number was read, and it
/// meant the same card changed colour day to day, so nothing was recognisable by sight.
///
/// Every row carries a veiled landscape, including the ones with nothing to report. At the veil's
/// strength the photograph survives as texture rather than as a picture — enough to stop a column
/// of cards reading as a spreadsheet, not enough to compete with the number — and a row that loses
/// its texture the moment a metric goes quiet makes the whole column twitch as data arrives.
struct MavVitalRow: View {
  let row: MavMetricRow

  /// Crop follows from the family rather than from the row's position, so a metric keeps the same
  /// landscape band across refreshes instead of reshuffling under the reader.
  private var sceneCrop: MavScene.Crop {
    switch row.metric.family {
    case .charge, .heart: .high
    case .rest, .energy: .middle
    case .effort, .vitals, .cycle: .low
    }
  }

  var body: some View {
    switch row.state {
    case .value(let text, _, let band, _, let word):
      NavigationLink {
        MavMetricDestination(metric: row.metric)
      } label: {
        VStack(alignment: .leading, spacing: 14) {
          header(word: word)

          HStack(alignment: .bottom, spacing: 18) {
            HStack(alignment: .firstTextBaseline, spacing: 3) {
              Text(text)
                .mavType(.numeralLarge)
                .foregroundStyle(MavTheme.ink)
              if let unit = row.metric.unit {
                Text(unit).mavType(.sub).foregroundStyle(MavTheme.inkSecondary)
              }
            }

            if let band {
              MavBaselineBar(
                band: band,
                lowText: MavMetricMapper.decimal(band.low, places: 0),
                highText: MavMetricMapper.decimal(band.high, places: 0),
                accessibilitySummary:
                  "\(text)\(row.metric.unit.map { " \($0)" } ?? "'"). Your normal range is "
                  + "\(MavMetricMapper.decimal(band.low, places: 0)) to "
                  + "\(MavMetricMapper.decimal(band.high, places: 0)).",
                family: row.metric.family
              )
              .padding(.bottom, 4)
            } else {
              Spacer(minLength: 0)
            }
          }
        }
        .padding(.horizontal, MavTheme.tilePadding)
        .padding(.top, 16)
        .padding(.bottom, 17)
        .frame(maxWidth: .infinity, alignment: .leading)
        .mavInteractiveSurface(MavTheme.tileShape, tint: MavTheme.tint(row.metric.family))
        // The landscape sits *under* the glass rather than on top of it, which is the whole point
        // of the material: it refracts the photograph instead of covering it.
        .background {
          MavScene(crop: sceneCrop, treatment: .veiled).clipShape(MavTheme.tileShape)
        }
        .contentShape(.rect)
      }
      .buttonStyle(.plain)
      .accessibilityElement(children: .combine)
      .accessibilityLabel(
        "\(row.metric.name), \(word), \(text)\(row.metric.unit.map { " \($0)" } ?? "")")
      .accessibilityHint("Opens the full history")

    case .unavailable(let reason):
      if row.metric.family == .cycle {
        NavigationLink {
          MavCycleView()
        } label: {
          HStack(spacing: 14) {
            Image(systemName: "circle.dotted")
              .foregroundStyle(row.metric.family.hue)
            VStack(alignment: .leading, spacing: 3) {
              Text("Cycle").mavType(.label).foregroundStyle(MavTheme.ink)
              Text("Period history and cycle day")
                .mavType(.sub)
                .foregroundStyle(MavTheme.inkSecondary)
            }
            Spacer()
            Image(systemName: "chevron.right")
              .font(.system(size: 12, weight: .semibold))
              .foregroundStyle(MavTheme.inkSecondary)
          }
          .padding(MavTheme.tilePadding)
          .mavInteractiveSurface(MavTheme.tileShape, tint: MavTheme.tint(.cycle))
          .background {
            MavScene(crop: sceneCrop, treatment: .veiled).clipShape(MavTheme.tileShape)
          }
        }
        .buttonStyle(.plain)
        .accessibilityHint("Opens cycle history")
      } else {
        MavUnavailableCard(name: row.metric.name, reason: reason)
          .background {
            MavScene(crop: sceneCrop, treatment: .veiled).clipShape(MavTheme.tileShape)
          }
      }
    }
  }

  private func header(word: String) -> some View {
    HStack(spacing: 9) {
      Image(systemName: icon)
        .font(.system(size: 11, weight: .medium))
        .foregroundStyle(row.metric.family.hue)
        .frame(width: 24, height: 24)
        .mavSurface(MavTheme.chipShape)

      Text(row.metric.name)
        .mavType(.label)
        .foregroundStyle(MavTheme.ink)

      Spacer(minLength: 6)

      // The verdict is a word, always. Colour is never the only thing saying it.
      Text(word)
        .mavType(.caption)
        .foregroundStyle(MavTheme.inkSecondary)

      Image(systemName: "chevron.right")
        .font(.system(size: 11, weight: .semibold))
        .foregroundStyle(MavTheme.inkSecondary)
    }
  }

  private var icon: String {
    switch row.metric.family {
    case .charge: "bolt.heart"
    case .rest: "moon"
    case .effort: "flame"
    case .heart: "heart"
    case .energy: "leaf"
    case .vitals: "waveform.path"
    case .cycle: "circle.dotted"
    }
  }
}

/// One metric route shared by Vitals and Today. Cycle owns a dedicated history/logging experience;
/// it must never fall through to the generic numeric-detail template merely because it has a value.
struct MavMetricDestination: View {
  let metric: MavMetric

  @ViewBuilder var body: some View {
    if metric.family == .cycle {
      MavCycleView()
    } else {
      MavMetricDetailView(metric: metric)
    }
  }
}
