import Combine
import Foundation

enum MavStrengthSetKind: String, Codable, CaseIterable, Identifiable {
  case warmup
  case working
  case drop
  case failure

  var id: String { rawValue }

  var shortLabel: String {
    switch self {
    case .warmup: "W"
    case .working: "N"
    case .drop: "D"
    case .failure: "F"
    }
  }

  var label: String {
    switch self {
    case .warmup: "Warm-up"
    case .working: "Working"
    case .drop: "Drop set"
    case .failure: "To failure"
    }
  }
}

struct MavStrengthSet: Codable, Identifiable, Equatable {
  var id = UUID()
  var kind: MavStrengthSetKind = .working
  var weight = ""
  var reps = "8"
  var rir = "2"
  var complete = false
}

struct MavStrengthExercise: Codable, Identifiable, Equatable {
  var id = UUID()
  var name: String
  var category: String
  var note = ""
  var previous = "—"
  var sets: [MavStrengthSet]
}

struct MavStrengthRoutine: Codable, Identifiable, Equatable {
  var id = UUID()
  var name: String
  var exercises: [MavStrengthExercise]
}

struct MavStrengthWorkoutRecord: Codable, Identifiable, Equatable {
  var id = UUID()
  var date = Date()
  var routineName: String
  var durationSeconds: Int
  var exercises: [MavStrengthExercise]

  var completedSets: Int { exercises.flatMap(\.sets).filter(\.complete).count }
  var volume: Double {
    var total = 0.0
    for exercise in exercises {
      for set in exercise.sets where set.complete {
        let load = Double(set.weight) ?? 0
        let repetitions = Int(set.reps) ?? 0
        total += load * Double(repetitions)
      }
    }
    return total
  }
}

struct MavExerciseDefinition: Identifiable, Hashable {
  let name: String
  let category: String
  var id: String { ExerciseLibrary.slug(name) }
}

enum MavStrengthLibrary {
  static let categories = ["Chest", "Back", "Shoulders", "Legs", "Arms", "Core", "Full body"]

  static let exercises: [MavExerciseDefinition] = [
    .init(name: "Bench press", category: "Chest"),
    .init(name: "Incline dumbbell press", category: "Chest"),
    .init(name: "Cable fly", category: "Chest"),
    .init(name: "Push-up", category: "Chest"),
    .init(name: "Pull-up", category: "Back"),
    .init(name: "Barbell row", category: "Back"),
    .init(name: "Lat pulldown", category: "Back"),
    .init(name: "Seated cable row", category: "Back"),
    .init(name: "Overhead press", category: "Shoulders"),
    .init(name: "Lateral raise", category: "Shoulders"),
    .init(name: "Rear delt fly", category: "Shoulders"),
    .init(name: "Back squat", category: "Legs"),
    .init(name: "Deadlift", category: "Legs"),
    .init(name: "Romanian deadlift", category: "Legs"),
    .init(name: "Leg press", category: "Legs"),
    .init(name: "Leg curl", category: "Legs"),
    .init(name: "Calf raise", category: "Legs"),
    .init(name: "Biceps curl", category: "Arms"),
    .init(name: "Hammer curl", category: "Arms"),
    .init(name: "Triceps pushdown", category: "Arms"),
    .init(name: "Skull crusher", category: "Arms"),
    .init(name: "Plank", category: "Core"),
    .init(name: "Cable crunch", category: "Core"),
    .init(name: "Hanging leg raise", category: "Core"),
    .init(name: "Kettlebell swing", category: "Full body"),
    .init(name: "Clean and press", category: "Full body"),
  ]

  static func exercise(_ name: String, sets: Int = 3, reps: Int = 8) -> MavStrengthExercise {
    let definition = exercises.first { $0.name == name }
    return MavStrengthExercise(
      name: name,
      category: definition?.category ?? "Other",
      sets: (0..<sets).map { index in
        MavStrengthSet(kind: index == 0 && sets > 3 ? .warmup : .working, reps: "\(reps)")
      })
  }

  static let starterRoutines: [MavStrengthRoutine] = [
    MavStrengthRoutine(
      name: "Full body",
      exercises: [
        exercise("Back squat"),
        exercise("Bench press"),
        exercise("Barbell row"),
        exercise("Romanian deadlift"),
      ]),
    MavStrengthRoutine(
      name: "Upper body",
      exercises: [
        exercise("Bench press"),
        exercise("Barbell row"),
        exercise("Overhead press"),
        exercise("Lat pulldown"),
        exercise("Biceps curl"),
        exercise("Triceps pushdown"),
      ]),
    MavStrengthRoutine(
      name: "Lower body",
      exercises: [
        exercise("Back squat", sets: 4),
        exercise("Romanian deadlift"),
        exercise("Leg press"),
        exercise("Leg curl"),
        exercise("Calf raise"),
      ]),
  ]
}

@MainActor
final class MavStrengthStore: ObservableObject {
  @Published var routines: [MavStrengthRoutine] {
    didSet { persist() }
  }
  @Published var history: [MavStrengthWorkoutRecord] {
    didSet { persist() }
  }

  private let defaults: UserDefaults
  private static let routinesKey = "mav.strength.routines.v2"
  private static let historyKey = "mav.strength.history.v2"

  init(defaults: UserDefaults = .standard) {
    self.defaults = defaults
    routines =
      Self.decode([MavStrengthRoutine].self, defaults.data(forKey: Self.routinesKey))
      ?? MavStrengthLibrary.starterRoutines
    history =
      Self.decode([MavStrengthWorkoutRecord].self, defaults.data(forKey: Self.historyKey))
      ?? []
  }

  func freshSession(from routine: MavStrengthRoutine?) -> (name: String, exercises: [MavStrengthExercise]) {
    guard let routine else { return ("New workout", []) }
    return (
      routine.name,
      routine.exercises.map { exercise in
        var copy = exercise
        copy.id = UUID()
        copy.sets = exercise.sets.map { set in
          var copy = set
          copy.id = UUID()
          copy.complete = false
          return copy
        }
        return copy
      })
  }

  func saveRoutine(name: String, exercises: [MavStrengthExercise]) {
    let clean = exercises.map { exercise in
      var copy = exercise
      copy.note = ""
      copy.previous = "—"
      copy.sets = exercise.sets.map { set in
        var copy = set
        copy.complete = false
        return copy
      }
      return copy
    }
    routines.insert(MavStrengthRoutine(name: name, exercises: clean), at: 0)
  }

  func deleteRoutine(_ routine: MavStrengthRoutine) {
    routines.removeAll { $0.id == routine.id }
  }

  func finish(
    routineName: String,
    startedAt: Date,
    exercises: [MavStrengthExercise]
  ) {
    history.insert(
      MavStrengthWorkoutRecord(
        routineName: routineName,
        durationSeconds: max(1, Int(Date().timeIntervalSince(startedAt))),
        exercises: exercises),
      at: 0)
    if history.count > 100 { history.removeLast(history.count - 100) }
  }

  private func persist() {
    defaults.set(try? JSONEncoder().encode(routines), forKey: Self.routinesKey)
    defaults.set(try? JSONEncoder().encode(history), forKey: Self.historyKey)
  }

  private static func decode<T: Decodable>(_ type: T.Type, _ data: Data?) -> T? {
    guard let data else { return nil }
    return try? JSONDecoder().decode(type, from: data)
  }
}
