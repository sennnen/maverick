import Foundation

// Cycle tracking, and the arithmetic behind it.
//
// Every number here is counted from period starts the user logged themselves. Nothing is inferred
// from a sensor: the core has a `cycle_phase` analytic that reads nightly skin temperature, and when
// that is admitted and available the screen shows it *as well*, clearly labelled as the core's. The
// two are never blended, because one is a fact about what someone typed and the other is a model
// output.
//
// This is not a medical device, it does not predict fertility, and it does not prevent pregnancy.
// That sentence appears on the screen, not just here.

struct MavCycleLog: Codable, Equatable, Sendable {
  /// Period start days, `yyyy-MM-dd`, ascending and unique.
  var periodStarts: [String] = []

  static let disclaimer =
    "Estimates only, counted from the dates you logged. Maverick is not a medical device, does not "
    + "predict fertility, and does not prevent pregnancy."

  private static let defaultsKey = "mav.cycle.log"

  static func load() -> MavCycleLog {
    guard let data = UserDefaults.standard.data(forKey: defaultsKey),
      let log = try? JSONDecoder().decode(MavCycleLog.self, from: data)
    else { return MavCycleLog() }
    return log
  }

  func save() {
    guard let data = try? JSONEncoder().encode(self) else { return }
    UserDefaults.standard.set(data, forKey: Self.defaultsKey)
  }

  mutating func logStart(_ day: String) {
    guard !periodStarts.contains(day) else { return }
    periodStarts.append(day)
    periodStarts.sort()
    save()
  }

  mutating func removeStart(_ day: String) {
    periodStarts.removeAll { $0 == day }
    save()
  }
}

/// The derived view of a cycle log. Pure, so every rule below is a test rather than a screenshot.
enum MavCycle {

  private static let formatter: DateFormatter = {
    let formatter = DateFormatter()
    formatter.dateFormat = "yyyy-MM-dd"
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.timeZone = .current
    return formatter
  }()

  static func date(_ key: String) -> Date? { formatter.date(from: key) }
  static func key(_ date: Date) -> String { formatter.string(from: date) }

  /// Whole days between two day keys, counted on the calendar rather than by dividing seconds, so a
  /// daylight-saving boundary does not knock the count out by one.
  static func days(from start: String, to end: String) -> Int? {
    guard let startDate = date(start), let endDate = date(end) else { return nil }
    return Calendar.current.dateComponents([.day], from: startDate, to: endDate).day
  }

  /// Cycle day, 1-based on the day of the last logged start on or before `day`.
  static func cycleDay(log: MavCycleLog, on day: String) -> Int? {
    guard let start = log.periodStarts.last(where: { $0 <= day }) else { return nil }
    return days(from: start, to: day).map { $0 + 1 }
  }

  /// Completed cycle lengths, oldest first. A cycle is only complete once the next one started.
  static func completedLengths(log: MavCycleLog) -> [Int] {
    zip(log.periodStarts, log.periodStarts.dropFirst()).compactMap { days(from: $0, to: $1) }
  }

  /// The estimate is a *range*, from the user's own recent cycles, and it refuses to exist below
  /// three completed cycles. Two points is not a pattern, and saying so is better than a number.
  static func nextPeriodRange(log: MavCycleLog) -> (earliest: String, latest: String)? {
    let lengths = completedLengths(log: log).suffix(6)
    guard lengths.count >= 3, let lastStart = log.periodStarts.last,
      let lastDate = date(lastStart),
      let shortest = lengths.min(), let longest = lengths.max()
    else { return nil }
    let calendar = Calendar.current
    guard let earliest = calendar.date(byAdding: .day, value: shortest, to: lastDate),
      let latest = calendar.date(byAdding: .day, value: longest, to: lastDate)
    else { return nil }
    return (key(earliest), key(latest))
  }

  static func medianLength(log: MavCycleLog) -> Int? {
    let lengths = completedLengths(log: log).sorted()
    guard !lengths.isEmpty else { return nil }
    return lengths[lengths.count / 2]
  }

  /// How many more cycles are needed before an estimate exists. Nil once there are enough.
  static func cyclesNeeded(log: MavCycleLog) -> Int? {
    let have = completedLengths(log: log).count
    return have >= 3 ? nil : 3 - have
  }
}
