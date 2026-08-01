import SwiftUI

struct MavStrengthView: View {
  let standalone: Bool
  let onComplete: (() -> Void)?
  @EnvironmentObject private var model: AppModel
  @Environment(\.dismiss) private var dismiss
  @StateObject private var store = MavStrengthStore()
  @State private var sessionName = ""
  @State private var exercises: [MavStrengthExercise] = []
  @State private var startedAt = Date()
  @State private var logging = false
  @State private var showBuilder = false

  init(standalone: Bool = true, onComplete: (() -> Void)? = nil) {
    self.standalone = standalone
    self.onComplete = onComplete
  }

  var body: some View {
    Group {
      if standalone {
        NavigationStack { strengthContent }
      } else {
        strengthContent
      }
    }
  }

  private var strengthContent: some View {
    Group {
      if logging {
        MavStrengthLogger(
          store: store,
          sessionName: $sessionName,
          exercises: $exercises,
          startedAt: startedAt,
          onFinish: {
            logging = false
            if let onComplete {
              onComplete()
            } else {
              dismiss()
            }
          })
      } else {
        routineHome
      }
    }
    .animation(MavTheme.calm, value: logging)
    .navigationTitle(logging ? sessionName : "Strength")
    .navigationBarTitleDisplayMode(.inline)
    .toolbar {
      if standalone && !logging {
        ToolbarItem(placement: .cancellationAction) {
          Button("Close") { dismiss() }
        }
      }
    }
    .sheet(isPresented: $showBuilder) {
      MavRoutineBuilder(store: store) { routine in
        begin(routine)
      }
    }
  }

  private var routineHome: some View {
    ZStack {
      MavTheme.canvas.ignoresSafeArea()
      ScrollView {
        VStack(alignment: .leading, spacing: MavTheme.cardSpacing) {
          VStack(alignment: .leading, spacing: 8) {
            Text("Start with a routine")
              .mavType(.display)
              .foregroundStyle(MavTheme.ink)
            Text("Your exercises, set types and targets are ready before the first set.")
              .mavType(.body)
              .foregroundStyle(MavTheme.inkSecondary)
          }
          .padding(.vertical, 10)

          MavPrimaryButton(title: "Start empty workout", systemImage: "play.fill") {
            begin(nil)
          }

          MavSectionHeader(title: "Routines")
          VStack(spacing: 0) {
            ForEach(Array(store.routines.enumerated()), id: \.element.id) { index, routine in
              if index > 0 { MavDivider() }
              Button { begin(routine) } label: {
                HStack(spacing: 14) {
                  Image(systemName: "list.bullet.rectangle")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(MavTheme.ink)
                    .frame(width: 30, height: 30)
                    .mavSurface(MavTheme.chipShape)
                  VStack(alignment: .leading, spacing: 3) {
                    Text(routine.name).mavType(.label).foregroundStyle(MavTheme.ink)
                    Text(routineSummary(routine))
                      .mavType(.sub)
                      .foregroundStyle(MavTheme.inkSecondary)
                      .lineLimit(1)
                  }
                  Spacer(minLength: 8)
                  Image(systemName: "chevron.right")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(MavTheme.inkSecondary)
                }
                .padding(.horizontal, MavTheme.tilePadding)
                .padding(.vertical, 14)
                .contentShape(.rect)
              }
              .buttonStyle(.plain)
              .contextMenu {
                Button("Delete routine", role: .destructive) {
                  store.deleteRoutine(routine)
                }
              }
            }
          }
          .mavSurface(MavTheme.tileShape)

          Button { showBuilder = true } label: {
            Label("Create routine", systemImage: "plus")
              .mavType(.label)
              .frame(maxWidth: .infinity, minHeight: 48)
          }
          .buttonStyle(.glass)

          if !store.history.isEmpty {
            MavSectionHeader(title: "Recent strength")
            VStack(spacing: 0) {
              ForEach(Array(store.history.prefix(4).enumerated()), id: \.element.id) { index, record in
                if index > 0 { MavDivider() }
                HStack {
                  VStack(alignment: .leading, spacing: 3) {
                    Text(record.routineName).mavType(.label).foregroundStyle(MavTheme.ink)
                    Text(record.date.formatted(date: .abbreviated, time: .omitted))
                      .mavType(.sub)
                      .foregroundStyle(MavTheme.inkSecondary)
                  }
                  Spacer()
                  Text("\(record.completedSets) sets")
                    .mavType(.sub)
                    .foregroundStyle(MavTheme.inkSecondary)
                }
                .padding(.horizontal, MavTheme.tilePadding)
                .padding(.vertical, 14)
              }
            }
            .mavSurface(MavTheme.tileShape)
          }
        }
        .padding(.horizontal, MavTheme.screenMargin)
        .padding(.bottom, 36)
      }
    }
  }

  private func begin(_ routine: MavStrengthRoutine?) {
    let session = store.freshSession(from: routine)
    sessionName = session.name
    exercises = session.exercises
    startedAt = Date()
    if model.usingDebugFixture, routine == nil {
      sessionName = "Push day"
      exercises = MavStrengthLibrary.starterRoutines[1].exercises.prefix(3).map { exercise in
        var copy = exercise
        copy.previous = exercise.name == "Bench press" ? "60 × 8" : "—"
        copy.sets = copy.sets.enumerated().map { index, set in
          var copy = set
          if exercise.name == "Bench press" {
            copy.weight = index < 2 ? "60" : "62.5"
            copy.reps = index < 2 ? "8" : "6"
            copy.complete = index < 2
          }
          return copy
        }
        return copy
      }
    }
    logging = true
  }

  private func routineSummary(_ routine: MavStrengthRoutine) -> String {
    let names = routine.exercises.prefix(3).map(\.name).joined(separator: " · ")
    return routine.exercises.count > 3 ? "\(names) +\(routine.exercises.count - 3)" : names
  }
}

private struct MavStrengthLogger: View {
  @ObservedObject var store: MavStrengthStore
  @Binding var sessionName: String
  @Binding var exercises: [MavStrengthExercise]
  let startedAt: Date
  let onFinish: () -> Void

  @State private var restEnd: Date?
  @State private var showExercisePicker = false
  @State private var replacementIndex: Int?
  @State private var confirmFinish = false
  @State private var showSaveRoutine = false
  @State private var routineName = ""

  var body: some View {
    ZStack {
      MavTheme.canvas.ignoresSafeArea()
      ScrollView {
        VStack(alignment: .leading, spacing: MavTheme.cardSpacing) {
          sessionHeader
          if restEnd != nil { restBanner }

          if exercises.isEmpty {
            MavTile {
              VStack(alignment: .leading, spacing: 7) {
                Text("Add your first exercise").mavType(.title)
                Text("Build the workout as you go, or save it as a routine when you finish.")
                  .mavType(.body)
                  .foregroundStyle(MavTheme.inkSecondary)
              }
            }
          }

          ForEach(exercises.indices, id: \.self) { index in
            exerciseCard(index)
          }

          Button {
            replacementIndex = nil
            showExercisePicker = true
          } label: {
            Label("Add exercise", systemImage: "plus")
              .mavType(.label)
              .frame(maxWidth: .infinity, minHeight: 48)
          }
          .buttonStyle(.glass)

          MavWideButton(title: "Finish workout") { confirmFinish = true }
        }
        .padding(.horizontal, MavTheme.screenMargin)
        .padding(.top, 10)
        .padding(.bottom, 36)
      }
    }
    .sheet(isPresented: $showExercisePicker) {
      MavExercisePicker(
        excluding: replacementIndex == nil ? Set(exercises.map(\.name)) : [],
        onSelect: { definition in
          let exercise = MavStrengthLibrary.exercise(definition.name)
          if let replacementIndex, exercises.indices.contains(replacementIndex) {
            exercises[replacementIndex] = exercise
          } else {
            exercises.append(exercise)
          }
          showExercisePicker = false
        })
    }
    .alert("Finish workout?", isPresented: $confirmFinish) {
      Button("Finish") { finish(saveRoutine: false) }
      Button("Finish & save routine") {
        routineName = sessionName == "New workout" ? "" : sessionName
        showSaveRoutine = true
      }
      Button("Keep logging", role: .cancel) {}
    } message: {
      Text("\(completedSets) completed sets · \(exercises.count) exercises.")
    }
    .sheet(isPresented: $showSaveRoutine) {
      NavigationStack {
        Form {
          TextField("Routine name", text: $routineName)
          Text("\(exercises.count) exercises · \(exercises.flatMap(\.sets).count) sets")
            .foregroundStyle(MavTheme.inkSecondary)
        }
        .navigationTitle("Save routine")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
          ToolbarItem(placement: .cancellationAction) {
            Button("Cancel") { showSaveRoutine = false }
          }
          ToolbarItem(placement: .confirmationAction) {
            Button("Save") {
              let cleanName = routineName.trimmingCharacters(in: .whitespacesAndNewlines)
              guard !cleanName.isEmpty else { return }
              sessionName = cleanName
              finish(saveRoutine: true)
              showSaveRoutine = false
            }
            .disabled(routineName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
          }
        }
      }
    }
  }

  private var sessionHeader: some View {
    MavTile {
      HStack(alignment: .top) {
        strengthStat("Elapsed") {
          TimelineView(.periodic(from: .now, by: 1)) { context in
            Text(elapsed(context.date))
              .mavType(.numeralMedium)
              .monospacedDigit()
          }
        }
        Spacer()
        strengthStat("Completed") {
          Text("\(completedSets) sets").mavType(.numeralMedium)
        }
        Spacer()
        strengthStat("Volume", trailing: true) {
          Text(volumeText).mavType(.numeralMedium)
        }
      }
    }
  }

  private func strengthStat<Content: View>(
    _ label: String,
    trailing: Bool = false,
    @ViewBuilder content: () -> Content
  ) -> some View {
    VStack(alignment: trailing ? .trailing : .leading, spacing: 5) {
      Text(label).mavType(.caption).foregroundStyle(MavTheme.inkSecondary)
      content().foregroundStyle(MavTheme.ink)
    }
    .accessibilityElement(children: .combine)
  }

  private var restBanner: some View {
    TimelineView(.periodic(from: .now, by: 1)) { context in
      let remaining = max(0, Int((restEnd ?? context.date).timeIntervalSince(context.date)))
      HStack(spacing: 12) {
        Image(systemName: remaining > 0 ? "timer" : "checkmark")
          .foregroundStyle(MavTheme.ink)
        VStack(alignment: .leading, spacing: 2) {
          Text(remaining > 0 ? "Rest" : "Rest complete").mavType(.caption)
          Text("\(remaining / 60):\(String(format: "%02d", remaining % 60))")
            .mavType(.numeralSmall)
            .monospacedDigit()
        }
        Spacer()
        Button(remaining > 0 ? "+30s" : "Dismiss") {
          if remaining > 0 {
            restEnd = (restEnd ?? context.date).addingTimeInterval(30)
          } else {
            restEnd = nil
          }
        }
        .buttonStyle(.glass)
        if remaining > 0 {
          Button("Skip") { restEnd = nil }.buttonStyle(.glass)
        }
      }
      .padding(MavTheme.tilePadding)
      .mavSurface(MavTheme.tileShape)
    }
  }

  private func exerciseCard(_ exerciseIndex: Int) -> some View {
    MavTile {
      VStack(alignment: .leading, spacing: 12) {
        HStack(alignment: .top) {
          VStack(alignment: .leading, spacing: 3) {
            Text(exercises[exerciseIndex].name).mavType(.title)
            Text(exercises[exerciseIndex].category)
              .mavType(.sub)
              .foregroundStyle(MavTheme.inkSecondary)
          }
          Spacer()
          Menu {
            Button("Replace exercise") {
              replacementIndex = exerciseIndex
              showExercisePicker = true
            }
            Button("Move up") { moveExercise(exerciseIndex, by: -1) }
              .disabled(exerciseIndex == 0)
            Button("Move down") { moveExercise(exerciseIndex, by: 1) }
              .disabled(exerciseIndex == exercises.count - 1)
            Divider()
            Button("Remove exercise", role: .destructive) {
              exercises.remove(at: exerciseIndex)
            }
          } label: {
            Image(systemName: "ellipsis")
              .frame(width: 44, height: 44)
              .contentShape(.rect)
          }
          .accessibilityLabel("Actions for \(exercises[exerciseIndex].name)")
        }

        setHeader

        ForEach(exercises[exerciseIndex].sets.indices, id: \.self) { setIndex in
          setRow(exerciseIndex, setIndex)
        }

        HStack(spacing: 10) {
          Button {
            let last = exercises[exerciseIndex].sets.last ?? MavStrengthSet()
            exercises[exerciseIndex].sets.append(
              MavStrengthSet(kind: .working, weight: last.weight, reps: last.reps, rir: last.rir))
          } label: {
            Label("Add set", systemImage: "plus")
              .mavType(.label)
              .frame(maxWidth: .infinity, minHeight: 44)
          }
          .buttonStyle(.glass)

          Button {
            exercises[exerciseIndex].note =
              exercises[exerciseIndex].note.isEmpty ? " " : exercises[exerciseIndex].note
          } label: {
            Image(systemName: "note.text")
              .frame(width: 44, height: 44)
          }
          .buttonStyle(.glass)
          .accessibilityLabel("Add note")
        }

        if !exercises[exerciseIndex].note.isEmpty {
          TextField("Exercise note", text: $exercises[exerciseIndex].note, axis: .vertical)
            .textFieldStyle(.plain)
            .padding(12)
            .background(MavTheme.sunken, in: MavTheme.chipShape)
        }
      }
    }
  }

  private var setHeader: some View {
    HStack(spacing: 6) {
      Text("Set").frame(width: 38, alignment: .center)
      Text("Prev").frame(width: 48, alignment: .center)
      Text("kg").frame(maxWidth: .infinity)
      Text("Reps").frame(width: 50)
      Text("RIR").frame(width: 42)
      Text("").frame(width: 44)
    }
    .mavType(.caption)
    .foregroundStyle(MavTheme.inkSecondary)
    .accessibilityHidden(true)
  }

  private func setRow(_ exerciseIndex: Int, _ setIndex: Int) -> some View {
    let set = exercises[exerciseIndex].sets[setIndex]
    return HStack(spacing: 6) {
      Menu {
        ForEach(MavStrengthSetKind.allCases) { kind in
          Button(kind.label) { exercises[exerciseIndex].sets[setIndex].kind = kind }
        }
        Divider()
        Button("Remove set", role: .destructive) {
          exercises[exerciseIndex].sets.remove(at: setIndex)
        }
      } label: {
        Text(set.kind == .working ? "\(setIndex + 1)" : set.kind.shortLabel)
          .mavType(.label)
          .foregroundStyle(MavTheme.ink)
          .frame(width: 38, height: 44)
          .background(MavTheme.raised, in: MavTheme.chipShape)
      }
      .accessibilityLabel("Set \(setIndex + 1), \(set.kind.label)")

      Text(exercises[exerciseIndex].previous)
        .mavType(.caption)
        .foregroundStyle(MavTheme.inkSecondary)
        .lineLimit(1)
        .minimumScaleFactor(0.7)
        .frame(width: 48)

      strengthField(
        "Weight",
        text: $exercises[exerciseIndex].sets[setIndex].weight,
        keyboard: .decimalPad)
      strengthField(
        "Repetitions",
        text: $exercises[exerciseIndex].sets[setIndex].reps,
        width: 50,
        keyboard: .numberPad)
      strengthField(
        "Reps in reserve",
        text: $exercises[exerciseIndex].sets[setIndex].rir,
        width: 42,
        keyboard: .numberPad)

      Button {
        exercises[exerciseIndex].sets[setIndex].complete.toggle()
        if !set.complete {
          restEnd = Date().addingTimeInterval(
            TimeInterval(WorkoutPrefs.defaultRestSeconds()))
        }
      } label: {
        Image(systemName: set.complete ? "checkmark.circle.fill" : "circle")
          .font(.system(size: 22))
          .foregroundStyle(set.complete ? MavTheme.ink : MavTheme.inkSecondary)
          .frame(width: 44, height: 44)
      }
      .accessibilityLabel(set.complete ? "Set completed" : "Complete set")
    }
  }

  private func strengthField(
    _ label: String,
    text: Binding<String>,
    width: CGFloat? = nil,
    keyboard: UIKeyboardType
  ) -> some View {
    TextField("—", text: text)
      .keyboardType(keyboard)
      .multilineTextAlignment(.center)
      .mavType(.label)
      .frame(maxWidth: width == nil ? .infinity : width, minHeight: 44)
      .background(MavTheme.sunken, in: MavTheme.chipShape)
      .accessibilityLabel(label)
  }

  private var completedSets: Int { exercises.flatMap(\.sets).filter(\.complete).count }

  private var volumeText: String {
    var value = 0.0
    for exercise in exercises {
      for set in exercise.sets where set.complete {
        let load = Double(set.weight) ?? 0
        let repetitions = Int(set.reps) ?? 0
        value += load * Double(repetitions)
      }
    }
    if value >= 1_000 { return String(format: "%.1ft", value / 1_000) }
    return "\(Int(value.rounded()))kg"
  }

  private func elapsed(_ now: Date) -> String {
    let seconds = max(0, Int(now.timeIntervalSince(startedAt)))
    return String(format: "%02d:%02d", seconds / 60, seconds % 60)
  }

  private func moveExercise(_ index: Int, by delta: Int) {
    let target = index + delta
    guard exercises.indices.contains(target) else { return }
    exercises.swapAt(index, target)
  }

  private func finish(saveRoutine: Bool) {
    if saveRoutine { store.saveRoutine(name: sessionName, exercises: exercises) }
    store.finish(routineName: sessionName, startedAt: startedAt, exercises: exercises)
    onFinish()
  }
}

private struct MavExercisePicker: View {
  let excluding: Set<String>
  let onSelect: (MavExerciseDefinition) -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var query = ""
  @State private var category = "All"

  private var filtered: [MavExerciseDefinition] {
    MavStrengthLibrary.exercises.filter { exercise in
      !excluding.contains(exercise.name)
        && (category == "All" || exercise.category == category)
        && (query.isEmpty || exercise.name.localizedCaseInsensitiveContains(query))
    }
  }

  var body: some View {
    NavigationStack {
      List {
        Section {
          ScrollView(.horizontal) {
            HStack(spacing: 8) {
              ForEach(["All"] + MavStrengthLibrary.categories, id: \.self) { item in
                Button(item) { category = item }
                  .buttonStyle(.bordered)
                  .tint(category == item ? MavTheme.ink : MavTheme.inkSecondary)
              }
            }
          }
          .scrollIndicators(.hidden)
          .listRowInsets(EdgeInsets())
          .listRowBackground(Color.clear)
        }

        ForEach(filtered) { exercise in
          Button { onSelect(exercise) } label: {
            HStack {
              VStack(alignment: .leading, spacing: 3) {
                Text(exercise.name).foregroundStyle(MavTheme.ink)
                Text(exercise.category).foregroundStyle(MavTheme.inkSecondary)
              }
              Spacer()
              Image(systemName: "plus.circle")
                .foregroundStyle(MavTheme.ink)
            }
          }
          .accessibilityHint("Adds this exercise")
        }
      }
      .searchable(text: $query, prompt: "Search exercises")
      .navigationTitle("Exercises")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Cancel") { dismiss() }
        }
      }
    }
  }
}

private struct MavRoutineBuilder: View {
  @ObservedObject var store: MavStrengthStore
  let onStart: (MavStrengthRoutine) -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var name = ""
  @State private var exercises: [MavStrengthExercise] = []
  @State private var showPicker = false

  var body: some View {
    NavigationStack {
      List {
        Section("Name") {
          TextField("Routine name", text: $name)
        }
        Section("Exercises") {
          ForEach(exercises) { exercise in
            Text(exercise.name)
          }
          .onDelete { exercises.remove(atOffsets: $0) }
          .onMove { exercises.move(fromOffsets: $0, toOffset: $1) }

          Button("Add exercise") { showPicker = true }
        }
      }
      .navigationTitle("New routine")
      .navigationBarTitleDisplayMode(.inline)
      .environment(\.editMode, .constant(.active))
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Cancel") { dismiss() }
        }
        ToolbarItem(placement: .confirmationAction) {
          Button("Save") {
            let routine = MavStrengthRoutine(
              name: name.trimmingCharacters(in: .whitespacesAndNewlines),
              exercises: exercises)
            store.routines.insert(routine, at: 0)
            dismiss()
          }
          .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || exercises.isEmpty)
        }
      }
      .sheet(isPresented: $showPicker) {
        MavExercisePicker(excluding: Set(exercises.map(\.name))) { definition in
          exercises.append(MavStrengthLibrary.exercise(definition.name))
          showPicker = false
        }
      }
    }
  }
}
