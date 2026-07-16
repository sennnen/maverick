import Foundation

// Cardio session configuration (§4): the per-sport sticky start config, the global
// milestone-interval deep settings, the per-zone haptic alert modes (§5), and the
// strength rest default. All UserDefaults — small, single-user, on-device.

/// What ends (or measures) a cardio session. Mutually exclusive — pick one (§4.1).
enum WorkoutGoalKind: String, Codable, CaseIterable, Identifiable {
    case none, distance, time, calories
    var id: String { rawValue }

    var label: String {
        switch self {
        case .none: "Free"
        case .distance: "Distance"
        case .time: "Time"
        case .calories: "Calories"
        }
    }
}

/// The chosen end condition. `value` is SI/native per kind: km for distance,
/// minutes for time, kcal for calories, ignored for `.none`.
struct WorkoutGoal: Codable, Equatable {
    var kind: WorkoutGoalKind = .none
    var value: Double = 0

    static let none = WorkoutGoal()
    var isActive: Bool { kind != .none && value > 0 }
}

/// Optional zone-time target for a session ("Zone 2 for 15 min", §4.4). Feeds the
/// live zone bars' target marker + the met-checkmark.
struct WorkoutZoneTarget: Codable, Equatable {
    /// 1…5.
    var zone: Int
    var minutes: Int
}

/// Everything the confirm screen configures, persisted per sport after each session
/// so the next start opens pre-filled (§4.5 — sticky settings ARE the template system).
struct WorkoutConfig: Codable, Equatable {
    var goal: WorkoutGoal = .none
    var zoneTarget: WorkoutZoneTarget?
    /// Per-session GPS override; nil = the sport's catalogue default.
    var gpsEnabled: Bool?
    var keepScreenOn = false
}

/// Per-zone haptic alert trigger (§5.1).
enum ZoneAlertMode: String, Codable, CaseIterable, Identifiable {
    case off, enter, exit, both
    var id: String { rawValue }

    var label: String {
        switch self {
        case .off: "Off"
        case .enter: "On enter"
        case .exit: "On exit"
        case .both: "Both"
        }
    }

    var firesOnEnter: Bool { self == .enter || self == .both }
    var firesOnExit: Bool { self == .exit || self == .both }
}

/// Interim-buzz cadence for a time end condition (§4.3).
enum TimeMilestoneMode: String, Codable, CaseIterable, Identifiable {
    case halfway, every10, every15, off
    var id: String { rawValue }
    var label: String {
        switch self {
        case .halfway: "Halfway"
        case .every10: "Every 10 min"
        case .every15: "Every 15 min"
        case .off: "Off"
        }
    }
}

/// Interim-buzz cadence for a calorie end condition (§4.3).
enum CalorieMilestoneMode: String, Codable, CaseIterable, Identifiable {
    case halfway, every50, every100, off
    var id: String { rawValue }
    var label: String {
        switch self {
        case .halfway: "Halfway"
        case .every50: "Every 50 kcal"
        case .every100: "Every 100 kcal"
        case .off: "Off"
        }
    }
}

enum WorkoutPrefs {

    // MARK: Per-sport sticky config (§4.5)

    private static func configKey(_ sport: String) -> String {
        "workout.config.\(ExerciseLibrary.slug(sport))"
    }

    static func config(for sport: String, defaults: UserDefaults = .standard) -> WorkoutConfig {
        guard let data = defaults.data(forKey: configKey(sport)),
              let cfg = try? JSONDecoder().decode(WorkoutConfig.self, from: data) else {
            return WorkoutConfig()
        }
        return cfg
    }

    static func save(_ config: WorkoutConfig, for sport: String,
                     defaults: UserDefaults = .standard) {
        guard let data = try? JSONEncoder().encode(config) else { return }
        defaults.set(data, forKey: configKey(sport))
    }

    // MARK: Milestone deep settings (§4.3)

    static let distanceEveryKey = "workout.milestone.distanceEvery"
    static let timeModeKey = "workout.milestone.timeMode"
    static let calorieModeKey = "workout.milestone.calorieMode"

    /// Distance interim spacing in the user's DISPLAY unit (km or mi), default 1.
    static func distanceEveryUnits(defaults: UserDefaults = .standard) -> Double {
        let v = defaults.double(forKey: distanceEveryKey)
        return v > 0 ? v : 1
    }

    static func timeMode(defaults: UserDefaults = .standard) -> TimeMilestoneMode {
        TimeMilestoneMode(rawValue: defaults.string(forKey: timeModeKey) ?? "") ?? .halfway
    }

    static func calorieMode(defaults: UserDefaults = .standard) -> CalorieMilestoneMode {
        CalorieMilestoneMode(rawValue: defaults.string(forKey: calorieModeKey) ?? "") ?? .halfway
    }

    // MARK: Strength rest default (§3.7)

    static let restSecondsKey = "strength.defaultRestSeconds"

    static func defaultRestSeconds(defaults: UserDefaults = .standard) -> Int {
        let v = defaults.integer(forKey: restSecondsKey)
        return v > 0 ? v : 90
    }
}

/// Sport-name slugging (the strength library arrives with the workout lane; only the slug is
/// needed for the per-sport config keys above).
enum ExerciseLibrary {
    /// Slug a display name into a stable id ("Bench Press" → "bench-press").
    static func slug(_ name: String) -> String {
        name.lowercased()
            .components(separatedBy: CharacterSet.alphanumerics.inverted)
            .filter { !$0.isEmpty }
            .joined(separator: "-")
    }
}
