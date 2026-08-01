import Foundation

// The sport catalogue.
//
// This used to be a nested tuple literal inside the start screen, which meant the screen was the
// only thing that could answer "does this sport have a route?" — and so nothing asked. The confirm
// screen needs exactly that, because offering GPS on a rowing erg is noise and withholding it on a
// trail run is a missing feature.
//
// The Kotlin twin is `MavSportCatalog.kt` and carries the same list in the same order. A sport that
// exists on one platform only is a parity break, so the two are asserted against each other by
// name in the parity tests.

/// One activity the wearer can start.
struct MavSport: Identifiable, Hashable, Sendable {
  let name: String
  /// SF Symbol on iOS; the Kotlin twin maps the same sport to its own icon set.
  let systemImage: String
  let detail: String
  /// Whether a route and a distance mean anything for this activity. Drives the GPS option and
  /// whether `distance` is offered as an end condition.
  let isDistance: Bool
  /// Strength is logged rather than timed, so it has no end condition and no zone target — it
  /// leaves the cardio flow entirely and opens the logger.
  let isStrength: Bool

  var id: String { name }

  init(
    _ name: String, _ systemImage: String, _ detail: String,
    isDistance: Bool = false, isStrength: Bool = false
  ) {
    self.name = name
    self.systemImage = systemImage
    self.detail = detail
    self.isDistance = isDistance
    self.isStrength = isStrength
  }
}

/// A named group of sports, in the order the start screen shows them.
struct MavSportCategory: Identifiable, Hashable, Sendable {
  let title: String
  let sports: [MavSport]
  var id: String { title }
}

enum MavSportCatalog {

  static let categories: [MavSportCategory] = [
    MavSportCategory(
      title: "Strength",
      sports: [
        MavSport(
          "Strength training", "dumbbell", "Routines, exercises, sets and rest", isStrength: true),
        MavSport(
          "Functional fitness", "figure.strengthtraining.functional",
          "Circuits and mixed resistance"),
      ]),
    MavSportCategory(
      title: "Run & walk",
      sports: [
        MavSport("Outdoor run", "figure.run", "GPS run", isDistance: true),
        MavSport("Treadmill", "figure.run.treadmill", "Indoor run"),
        MavSport("Walking", "figure.walk", "Outdoor or indoor walk", isDistance: true),
        MavSport("Hiking", "figure.hiking", "Trail and elevation", isDistance: true),
      ]),
    MavSportCategory(
      title: "Cardio",
      sports: [
        MavSport("Cycling", "bicycle", "Indoor or outdoor", isDistance: true),
        MavSport("Swimming", "figure.pool.swim", "Pool or open water"),
        MavSport("Rowing", "figure.rower", "Erg or water"),
        MavSport("Elliptical", "figure.elliptical", "Indoor cardio"),
      ]),
    MavSportCategory(
      title: "Mind & body",
      sports: [
        MavSport("Yoga", "figure.yoga", "Yoga practice"),
        MavSport("Pilates", "figure.pilates", "Mat or reformer"),
        MavSport("Mobility", "figure.flexibility", "Movement and recovery"),
      ]),
    MavSportCategory(
      title: "Sports",
      sports: [
        MavSport("Football", "figure.soccer", "Training or match"),
        MavSport("Tennis", "figure.tennis", "Singles or doubles"),
        MavSport("Basketball", "figure.basketball", "Training or game"),
        MavSport("Boxing", "figure.boxing", "Bag, pads or sparring"),
      ]),
    MavSportCategory(
      title: "Other",
      sports: [MavSport("Other activity", "figure.mixed.cardio", "Anything else")]),
  ]

  static let all: [MavSport] = categories.flatMap(\.sports)

  /// Look a sport up by the name a stored config or a running session carries.
  static func sport(named name: String) -> MavSport? {
    all.first { $0.name == name }
  }
}

/// The confirm screen's number formatting, as pure functions so a test can assert the conversions
/// without rendering a control.
///
/// The rule they exist to protect: a goal is **stored natively** — kilometres, minutes,
/// kilocalories — and converted only on the way to a label. A stored value that meant miles on one
/// phone and kilometres on another is a class of bug worth designing out rather than testing for.
enum MavGoalText {

  /// The value a freshly picked end condition starts at.
  static func defaultValue(_ kind: WorkoutGoalKind, isImperial: Bool) -> Double {
    switch kind {
    case .none: 0
    // Three miles, stored in kilometres.
    case .distance: isImperial ? 3 / UnitFormatter.milesPerKilometer : 5
    case .time: 30
    case .calories: 300
    }
  }

  /// How a stored goal reads in the wearer's units. Whole numbers drop their decimal.
  static func display(_ goal: WorkoutGoal, isImperial: Bool) -> String {
    guard goal.isActive else { return "" }
    let value =
      goal.kind == .distance && isImperial ? UnitFormatter.kmToMiles(goal.value) : goal.value
    let rounded = (value * 10).rounded() / 10
    return rounded == rounded.rounded()
      ? String(Int(rounded.rounded())) : String(format: "%.1f", rounded)
  }

  /// The unit label shown beside the field.
  static func unit(_ kind: WorkoutGoalKind, distanceUnit: String) -> String {
    switch kind {
    case .none: ""
    case .distance: distanceUnit
    case .time: "min"
    case .calories: "kcal"
    }
  }
}
