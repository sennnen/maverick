import Foundation

/// Read facade with NOOP `Repository`'s member surface, backed by the Rust core instead of a
/// Swift-side GRDB store. Day history, sleeps, journal and workouts stay HONESTLY empty until
/// the core exposes the matching read models — no cached scores, no fabricated rows. The pure
/// day-resolution statics (logical day, widget anchor, vitals carry) are ported verbatim so the
/// hubs anchor on exactly the day NOOP would.
@MainActor
final class Repository: ObservableObject {
  @Published private(set) var days: [DailyMetric] = []
  @Published private(set) var sleeps: [CachedSleepSession] = []
  @Published private(set) var importedSleep: [String: ImportedSleepFigures] = [:]
  /// Bumped on every refresh so `.task(id:)` view reloads re-run.
  @Published private(set) var refreshSeq = 0

  func refresh() async {
    refreshSeq += 1
  }

  // MARK: Reads (empty until the core serves them)

  func exploreSeries(key: String, source: String, days: Int = 4000,
                     fullHistory: Bool = false) async -> [(day: String, value: Double)] { [] }

  func workoutRows(days: Int = 4000) async -> [WorkoutRow] { [] }

  func workoutZoneMinutes(from: Int, to: Int, age: Int) async -> [Double]? { nil }

  // MARK: Journal (interim on-device persistence)
  //
  // The core has no journal lane yet. Answers still must never vanish, so they persist to
  // UserDefaults under the same (day, question) natural key and migrate into the core store
  // when that lane lands.

  private static let journalDefaultsKey = "mav.journal.entries"

  func journalEntries(days: Int = 4000) async -> [JournalEntry] {
    Self.loadJournal()
  }

  func saveJournalAnswer(day: String, question: String, answeredYes: Bool, notes: String? = nil) async {
    upsertJournal(JournalEntry(day: day, question: question, answeredYes: answeredYes, notes: notes))
  }

  func saveJournalNumeric(day: String, question: String, value: Double, notes: String? = nil) async {
    upsertJournal(JournalEntry(day: day, question: question, answeredYes: true, notes: notes,
                               numericValue: value))
  }

  private static func loadJournal() -> [JournalEntry] {
    guard let data = UserDefaults.standard.data(forKey: journalDefaultsKey),
          let entries = try? JSONDecoder().decode([JournalEntry].self, from: data) else { return [] }
    return entries
  }

  private func upsertJournal(_ entry: JournalEntry) {
    var entries = Self.loadJournal()
    entries.removeAll { $0.day == entry.day && $0.question == entry.question }
    entries.append(entry)
    entries.sort { ($0.day, $0.question) < ($1.day, $1.question) }
    if let data = try? JSONEncoder().encode(entries) {
      UserDefaults.standard.set(data, forKey: Self.journalDefaultsKey)
    }
    refreshSeq += 1
  }

  // MARK: Store diagnostics

  func storeHandle() async -> MavStoreHandle? {
    MavStoreHandle(databaseURL: MavStore.databaseURL())
  }

  func dataVolumeSnapshot() async -> DataVolume? { nil }

  func checkpointForBackup() async -> Bool { false }

  // MARK: Canonical source ids

  static let whoopSource = "my-whoop"
  static let appleHealthSource = "apple-health"
  static let healthConnectSource = "health-connect"

  // MARK: Day resolution (ported verbatim from NOOP Repository)

  /// Prefer the LOCAL-calendar-day row when it differs from the logical day AND has a banked
  /// night; otherwise the logical-day row (the anti-blank guard for the post-midnight window).
  nonisolated static func resolveToday(days: [DailyMetric], logicalKey: String, localKey: String) -> DailyMetric? {
    if localKey != logicalKey,
       let localRow = days.last(where: { $0.day == localKey && $0.totalSleepMin != nil }) {
      return localRow
    }
    return days.last(where: { $0.day == logicalKey })
  }

  /// The single anchor row every surface resolves "today" through: today's row when it is
  /// recovery-scored, else the freshest strictly-prior scored day (future-day guarded).
  nonisolated static func widgetAnchor(days: [DailyMetric], logicalKey: String, localKey: String) -> DailyMetric? {
    let todayRow = resolveToday(days: days, logicalKey: logicalKey, localKey: localKey)
    if todayRow?.recovery != nil { return todayRow }
    let carriedKey = todayRow?.day ?? logicalKey
    return days.last(where: { $0.recovery != nil && $0.day < carriedKey })
  }

  static func widgetAnchor(days: [DailyMetric], now: Date = Date()) -> DailyMetric? {
    widgetAnchor(days: days, logicalKey: logicalDayKey(now), localKey: localDayKey(now))
  }

  /// The recovery-independent overnight-vitals carry: the freshest strictly-prior day carrying
  /// any of HRV / resting HR / respiratory. Vitals only — never feeds Charge/Effort/Rest.
  static func lastVitalsDay(days: [DailyMetric], now: Date = Date()) -> DailyMetric? {
    lastVitalsDay(days: days, todayKey: max(logicalDayKey(now), localDayKey(now)))
  }

  nonisolated static func lastVitalsDay(days: [DailyMetric], todayKey: String) -> DailyMetric? {
    days.last(where: {
      ($0.avgHrv != nil || $0.restingHr != nil || $0.respRateBpm != nil) && $0.day < todayKey
    })
  }

  /// 04:00 local — the hour the logical day rolls. Between midnight and then, Today stays put.
  nonisolated static let logicalDayRolloverHour = 4

  nonisolated private static let dayKeyFormatter: DateFormatter = {
    let f = DateFormatter()
    f.dateFormat = "yyyy-MM-dd"
    f.locale = Locale(identifier: "en_US_POSIX")
    return f
  }()

  nonisolated static func localDayKey(_ date: Date) -> String { dayKeyFormatter.string(from: date) }

  nonisolated static func logicalDay(_ now: Date, rolloverHour: Int = logicalDayRolloverHour) -> Date {
    now.addingTimeInterval(TimeInterval(-rolloverHour * 3600))
  }

  nonisolated static func logicalDayKey(_ now: Date, rolloverHour: Int = logicalDayRolloverHour) -> String {
    localDayKey(logicalDay(now, rolloverHour: rolloverHour))
  }
}
