import Foundation

// Plain read models for values the Aura UI renders. Mav has no Swift-side store: rows arrive from
// the Rust core through the FFI snapshot.

/// One day's cached metrics ("YYYY-MM-DD" day key). Absent stays absent — columns are optionals.
struct DailyMetric: Equatable, Codable {
  let day: String
  let totalSleepMin: Double?
  let efficiency: Double?
  let deepMin: Double?
  let remMin: Double?
  let lightMin: Double?
  let disturbances: Int?
  let restingHr: Int?
  let avgHrv: Double?
  let recovery: Double?
  let strain: Double?
  let exerciseCount: Int?
  let spo2Pct: Double?
  let skinTempDevC: Double?
  let respRateBpm: Double?
  let steps: Int?
  let activeKcalEst: Double?
  let sourcePriority: Int?

  init(day: String, totalSleepMin: Double? = nil, efficiency: Double? = nil, deepMin: Double? = nil,
       remMin: Double? = nil, lightMin: Double? = nil, disturbances: Int? = nil, restingHr: Int? = nil,
       avgHrv: Double? = nil, recovery: Double? = nil, strain: Double? = nil, exerciseCount: Int? = nil,
       spo2Pct: Double? = nil, skinTempDevC: Double? = nil, respRateBpm: Double? = nil,
       steps: Int? = nil, activeKcalEst: Double? = nil, sourcePriority: Int? = nil) {
    self.day = day; self.totalSleepMin = totalSleepMin; self.efficiency = efficiency
    self.deepMin = deepMin; self.remMin = remMin; self.lightMin = lightMin
    self.disturbances = disturbances; self.restingHr = restingHr; self.avgHrv = avgHrv
    self.recovery = recovery; self.strain = strain; self.exerciseCount = exerciseCount
    self.spo2Pct = spo2Pct; self.skinTempDevC = skinTempDevC; self.respRateBpm = respRateBpm
    self.steps = steps; self.activeKcalEst = activeKcalEst; self.sourcePriority = sourcePriority
  }
}

/// One sleep session; `stagesJSON` is the verbatim stage-segments JSON array.
struct CachedSleepSession: Equatable, Codable {
  let startTs: Int
  let endTs: Int
  let efficiency: Double?
  let restingHr: Int?
  let avgHrv: Double?
  let stagesJSON: String?
  let userEdited: Bool
  let startTsAdjusted: Int?
  var effectiveStartTs: Int { startTsAdjusted ?? startTs }

  init(startTs: Int, endTs: Int, efficiency: Double?, restingHr: Int?, avgHrv: Double?,
       stagesJSON: String?, userEdited: Bool = false, startTsAdjusted: Int? = nil) {
    self.startTs = startTs; self.endTs = endTs; self.efficiency = efficiency
    self.restingHr = restingHr; self.avgHrv = avgHrv; self.stagesJSON = stagesJSON
    self.userEdited = userEdited; self.startTsAdjusted = startTsAdjusted
  }
}

/// Per-day sleep figures a WHOOP export carried verbatim; preferred over recomputations.
struct ImportedSleepFigures: Equatable {
  var performancePct: Double?
  var consistencyPct: Double?
  var needMin: Double?
  var debtMin: Double?
}

/// One logged journal answer for a day.
struct JournalEntry: Equatable, Codable {
  let day: String
  let question: String
  let answeredYes: Bool
  let notes: String?
  let numericValue: Double?

  init(day: String, question: String, answeredYes: Bool, notes: String?, numericValue: Double? = nil) {
    self.day = day; self.question = question; self.answeredYes = answeredYes
    self.notes = notes; self.numericValue = numericValue
  }
}

/// One workout; `zonesJSON` is verbatim HR-zone-percentages JSON, times unix seconds.
struct WorkoutRow: Equatable, Codable {
  let startTs: Int
  let endTs: Int
  let sport: String
  let source: String
  let durationS: Double?
  let energyKcal: Double?
  let avgHr: Int?
  let maxHr: Int?
  let strain: Double?
  let distanceM: Double?
  let zonesJSON: String?
  let notes: String?

  init(startTs: Int, endTs: Int, sport: String, source: String, durationS: Double? = nil,
       energyKcal: Double? = nil, avgHr: Int? = nil, maxHr: Int? = nil, strain: Double? = nil,
       distanceM: Double? = nil, zonesJSON: String? = nil, notes: String? = nil) {
    self.startTs = startTs; self.endTs = endTs; self.sport = sport; self.source = source
    self.durationS = durationS; self.energyKcal = energyKcal; self.avgHr = avgHr; self.maxHr = maxHr
    self.strain = strain; self.distanceM = distanceM; self.zonesJSON = zonesJSON; self.notes = notes
  }
}

/// Compact store-size snapshot for the diagnostics card.
struct DataVolume: Sendable, Equatable {
  let dbRows: Int
  let importedDays: Int
  let workouts: Int
  let lastRenderRows: Int?
}
