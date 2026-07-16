import SwiftUI

// Journal — the morning check-in, built on the native journal store. Yes/no
// behaviours as tap-chips grouped the same way Android groups them; numeric
// items (caffeine, alcohol…) get steppers. Answers persist immediately and
// feed the behaviour→recovery insights.

struct AuraJournalView: View {
  @EnvironmentObject private var repo: Repository
  @StateObject private var catalog = JournalCatalogStore()

  /// The full display list: starters ∪ imported ∪ custom, folded by the catalog.
  @State private var resolved: [JournalCatalogItem] = []
  /// question → today's answer (nil = unanswered).
  @State private var answers: [String: Bool] = [:]
  @State private var numbers: [String: Double] = [:]
  @State private var recentDays: [String: Int] = [:]   // dayKey → answered count (7-day strip)
  @State private var loaded = false

  private var dayKey: String { Repository.localDayKey(Date()) }

  var body: some View {
    AuraSheet(title: "Journal", family: .energy) {
      header

      ForEach(JournalGroup.allCases, id: \.self) { group in
        let items = resolved.filter { $0.group == group }
        if !items.isEmpty {
          VStack(alignment: .leading, spacing: 12) {
            AuraSectionHeader(title: groupName(group))
            VStack(spacing: 0) {
              ForEach(items) { item in
                itemRow(item)
                if item.id != items.last?.id {
                  Rectangle().fill(AuraDesign.hairline).frame(height: 1).padding(.leading, 18)
                }
              }
            }
            .padding(.vertical, 4)
            .background(AuraDesign.card, in: AuraDesign.tileShape)
            .overlay(AuraDesign.tileShape.strokeBorder(AuraDesign.hairline, lineWidth: 1))
          }
        }
      }

      Text("Answers stay on this device and sharpen your recovery insights over time.")
        .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.45))
        .padding(.horizontal, 4)
    }
    .task { await load() }
  }

  // MARK: Header — date + 7-day streak strip

  private var header: some View {
    VStack(alignment: .leading, spacing: 14) {
      HStack {
        VStack(alignment: .leading, spacing: 3) {
          Text(Date.now.formatted(.dateTime.weekday(.wide).month().day()))
            .font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          Text("\(answeredCount) of \(visibleCount) answered")
            .font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.55))
        }
        Spacer()
        weekStrip
      }
    }
    .auraDarkCard(padding: 18)
  }

  private var answeredCount: Int {
    resolved.filter { answers[$0.canonical] != nil || numbers[$0.canonical] != nil }.count
  }
  private var visibleCount: Int { resolved.count }

  private var weekStrip: some View {
    HStack(spacing: 5) {
      ForEach(lastSevenDays(), id: \.self) { key in
        let n = recentDays[key] ?? 0
        Circle()
          .fill(n > 0 ? AuraDesign.Family.energy.glow : AuraDesign.ink.opacity(0.14))
          .frame(width: 8, height: 8)
          .shadow(color: n > 0 ? AuraDesign.Family.energy.glow.opacity(0.7) : .clear, radius: 3)
      }
    }
    .accessibilityLabel(Text("Last seven days"))
  }

  private func lastSevenDays() -> [String] {
    (0..<7).reversed().compactMap {
      Calendar.current.date(byAdding: .day, value: -$0, to: Date())
        .map(Repository.localDayKey)
    }
  }

  // MARK: Item rows

  @ViewBuilder
  private func itemRow(_ item: JournalCatalogItem) -> some View {
    if item.kind.isNumeric {
      numericRow(item)
    } else {
      boolRow(item)
    }
  }

  private func boolRow(_ item: JournalCatalogItem) -> some View {
    let current = answers[item.canonical]
    return HStack(spacing: 12) {
      Text(item.display)
        .font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
        .lineLimit(2).minimumScaleFactor(0.85)
      Spacer(minLength: 8)
      HStack(spacing: 4) {
        answerChip("No", active: current == false, tint: AuraDesign.ink.opacity(0.7)) {
          set(item, yes: current == false ? nil : false)
        }
        answerChip("Yes", active: current == true, tint: AuraDesign.Family.energy.glow) {
          set(item, yes: current == true ? nil : true)
        }
      }
    }
    .padding(.horizontal, 18).padding(.vertical, 10)
  }

  private func answerChip(_ label: String, active: Bool, tint: Color,
                          action: @escaping () -> Void) -> some View {
    Button {
      withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) { action() }
    } label: {
      Text(label)
        .font(AuraDesign.caption)
        .foregroundStyle(active ? Color.black : AuraDesign.ink.opacity(0.65))
        .padding(.horizontal, 14).padding(.vertical, 7)
        .background(active ? AnyShapeStyle(tint) : AnyShapeStyle(AuraDesign.ink.opacity(0.08)),
                    in: Capsule())
        .contentShape(Capsule())
    }
    .buttonStyle(.plain)
    .accessibilityAddTraits(active ? [.isButton, .isSelected] : .isButton)
  }

  private func numericRow(_ item: JournalCatalogItem) -> some View {
    let value = numbers[item.canonical]
    let unit = item.kind.unitLabel ?? ""
    let step: Double = unit.lowercased().contains("mg") ? 25 : 1
    return HStack(spacing: 12) {
      VStack(alignment: .leading, spacing: 2) {
        Text(item.display)
          .font(AuraDesign.label).foregroundStyle(AuraDesign.ink.opacity(0.92))
          .lineLimit(2).minimumScaleFactor(0.85)
        if !unit.isEmpty {
          Text(unit).font(AuraDesign.caption).foregroundStyle(AuraDesign.ink.opacity(0.5))
        }
      }
      Spacer(minLength: 8)
      HStack(spacing: 10) {
        stepBtn("minus") {
          let v = max(0, (value ?? 0) - step)
          setNumeric(item, value: v == 0 ? nil : v)
        }
        Text(value.map { $0.truncatingRemainder(dividingBy: 1) == 0 ? String(Int($0)) : String(format: "%.1f", $0) } ?? "0")
          .font(AuraDesign.number(20)).foregroundStyle(AuraDesign.ink)
          .monospacedDigit()
          .frame(minWidth: 44)
          .contentTransition(.numericText())
        stepBtn("plus") { setNumeric(item, value: (value ?? 0) + step) }
      }
    }
    .padding(.horizontal, 18).padding(.vertical, 10)
  }

  private func stepBtn(_ icon: String, action: @escaping () -> Void) -> some View {
    Button {
      withAnimation(.snappy(duration: 0.15)) { action() }
    } label: {
      Image(systemName: icon)
        .font(.system(size: 13, weight: .semibold))
        .foregroundStyle(AuraDesign.ink)
        .frame(width: 34, height: 34)
        .background(AuraDesign.ink.opacity(0.08), in: Circle())
        .contentShape(Circle())
    }
    .buttonStyle(.plain)
  }

  // MARK: Persistence

  private func set(_ item: JournalCatalogItem, yes: Bool?) {
    answers[item.canonical] = yes
    guard let yes else {
      // Un-answering re-saves as "no" would be a lie; the store has no delete,
      // so keep the last explicit answer out by writing nothing.
      return
    }
    Task { await repo.saveJournalAnswer(day: dayKey, question: item.canonical, answeredYes: yes) }
  }

  private func setNumeric(_ item: JournalCatalogItem, value: Double?) {
    numbers[item.canonical] = value
    guard let value else { return }
    Task { await repo.saveJournalNumeric(day: dayKey, question: item.canonical, value: value) }
  }

  private func load() async {
    guard !loaded else { return }
    loaded = true
    let entries = await repo.journalEntries(days: 60)
    var counts: [String: Int] = [:]
    for e in entries {
      counts[e.day, default: 0] += 1
      if e.day == dayKey {
        if let v = e.numericValue { numbers[e.question] = v }
        else { answers[e.question] = e.answeredYes }
      }
    }
    recentDays = counts
    // Starters ∪ every question ever logged/imported, minus hidden, via the catalog fold.
    let importedQs = Array(Set(entries.map(\.question)))
    resolved = catalog.resolvedItems(imported: importedQs)
  }

  private func groupName(_ g: JournalGroup) -> String {
    switch g {
    case .supplements: "Supplements"
    case .nutrition: "Nutrition"
    case .lifestyle: "Lifestyle"
    case .health: "Health"
    case .behaviour: "Behaviour"
    case .other: "Other"
    }
  }
}
