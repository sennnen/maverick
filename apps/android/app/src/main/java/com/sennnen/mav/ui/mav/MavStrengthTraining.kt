package com.sennnen.mav.ui.mav

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

enum class MavStrengthSetKind(val label: String, val shortLabel: String) {
    WARMUP("Warm-up", "W"),
    WORKING("Working", "N"),
    DROP("Drop set", "D"),
    FAILURE("To failure", "F"),
}

data class MavStrengthSet(
    val id: String = UUID.randomUUID().toString(),
    val kind: MavStrengthSetKind = MavStrengthSetKind.WORKING,
    val weight: String = "",
    val reps: String = "8",
    val rir: String = "2",
    val complete: Boolean = false,
)

data class MavStrengthExercise(
    val id: String = UUID.randomUUID().toString(),
    val name: String,
    val category: String,
    val note: String = "",
    val previous: String = "—",
    val sets: List<MavStrengthSet>,
)

data class MavStrengthRoutine(
    val id: String = UUID.randomUUID().toString(),
    val name: String,
    val exercises: List<MavStrengthExercise>,
)

data class MavStrengthWorkoutRecord(
    val id: String = UUID.randomUUID().toString(),
    val timestamp: Long = System.currentTimeMillis(),
    val routineName: String,
    val durationSeconds: Int,
    val exercises: List<MavStrengthExercise>,
) {
    val completedSets: Int get() = exercises.sumOf { exercise -> exercise.sets.count { it.complete } }
    val volume: Double
        get() = exercises.flatMap { it.sets }.filter { it.complete }.sumOf { set ->
            (set.weight.toDoubleOrNull() ?: 0.0) * (set.reps.toIntOrNull() ?: 0)
        }
}

data class MavExerciseDefinition(val name: String, val category: String)

object MavStrengthLibrary {
    val categories = listOf("Chest", "Back", "Shoulders", "Legs", "Arms", "Core", "Full body")

    val exercises = listOf(
        MavExerciseDefinition("Bench press", "Chest"),
        MavExerciseDefinition("Incline dumbbell press", "Chest"),
        MavExerciseDefinition("Cable fly", "Chest"),
        MavExerciseDefinition("Push-up", "Chest"),
        MavExerciseDefinition("Pull-up", "Back"),
        MavExerciseDefinition("Barbell row", "Back"),
        MavExerciseDefinition("Lat pulldown", "Back"),
        MavExerciseDefinition("Seated cable row", "Back"),
        MavExerciseDefinition("Overhead press", "Shoulders"),
        MavExerciseDefinition("Lateral raise", "Shoulders"),
        MavExerciseDefinition("Rear delt fly", "Shoulders"),
        MavExerciseDefinition("Back squat", "Legs"),
        MavExerciseDefinition("Deadlift", "Legs"),
        MavExerciseDefinition("Romanian deadlift", "Legs"),
        MavExerciseDefinition("Leg press", "Legs"),
        MavExerciseDefinition("Leg curl", "Legs"),
        MavExerciseDefinition("Calf raise", "Legs"),
        MavExerciseDefinition("Biceps curl", "Arms"),
        MavExerciseDefinition("Hammer curl", "Arms"),
        MavExerciseDefinition("Triceps pushdown", "Arms"),
        MavExerciseDefinition("Skull crusher", "Arms"),
        MavExerciseDefinition("Plank", "Core"),
        MavExerciseDefinition("Cable crunch", "Core"),
        MavExerciseDefinition("Hanging leg raise", "Core"),
        MavExerciseDefinition("Kettlebell swing", "Full body"),
        MavExerciseDefinition("Clean and press", "Full body"),
    )

    fun exercise(name: String, setCount: Int = 3, reps: Int = 8): MavStrengthExercise {
        val definition = exercises.firstOrNull { it.name == name }
        return MavStrengthExercise(
            name = name,
            category = definition?.category ?: "Other",
            sets = List(setCount) { index ->
                MavStrengthSet(
                    kind = if (index == 0 && setCount > 3) MavStrengthSetKind.WARMUP
                    else MavStrengthSetKind.WORKING,
                    reps = reps.toString(),
                )
            },
        )
    }

    val starterRoutines = listOf(
        MavStrengthRoutine(
            name = "Full body",
            exercises = listOf(
                exercise("Back squat"),
                exercise("Bench press"),
                exercise("Barbell row"),
                exercise("Romanian deadlift"),
            ),
        ),
        MavStrengthRoutine(
            name = "Upper body",
            exercises = listOf(
                exercise("Bench press"),
                exercise("Barbell row"),
                exercise("Overhead press"),
                exercise("Lat pulldown"),
                exercise("Biceps curl"),
                exercise("Triceps pushdown"),
            ),
        ),
        MavStrengthRoutine(
            name = "Lower body",
            exercises = listOf(
                exercise("Back squat", 4),
                exercise("Romanian deadlift"),
                exercise("Leg press"),
                exercise("Leg curl"),
                exercise("Calf raise"),
            ),
        ),
    )

    fun freshExercises(routine: MavStrengthRoutine?): List<MavStrengthExercise> =
        routine?.exercises?.map { exercise ->
            exercise.copy(
                id = UUID.randomUUID().toString(),
                sets = exercise.sets.map { set ->
                    set.copy(id = UUID.randomUUID().toString(), complete = false)
                },
            )
        } ?: emptyList()
}

class MavStrengthStore(context: Context) {
    private val prefs = context.getSharedPreferences("mav_strength_v2", Context.MODE_PRIVATE)

    fun routines(): List<MavStrengthRoutine> {
        val stored = runCatching {
            prefs.getString("routines", null)?.let(::decodeRoutines)
        }.getOrNull()
        return stored?.takeIf { it.isNotEmpty() } ?: MavStrengthLibrary.starterRoutines
    }

    fun saveRoutines(routines: List<MavStrengthRoutine>) {
        prefs.edit().putString("routines", encodeRoutines(routines).toString()).apply()
    }

    fun history(): List<MavStrengthWorkoutRecord> = runCatching {
        prefs.getString("history", null)?.let(::decodeHistory)
    }.getOrNull() ?: emptyList()

    fun finish(
        routineName: String,
        startedAt: Long,
        exercises: List<MavStrengthExercise>,
        saveRoutine: Boolean,
        routines: List<MavStrengthRoutine>,
    ): List<MavStrengthRoutine> {
        val nextRoutines = if (saveRoutine) {
            listOf(MavStrengthRoutine(name = routineName, exercises = exercises.map(::cleanExercise))) + routines
        } else {
            routines
        }
        saveRoutines(nextRoutines)
        val record = MavStrengthWorkoutRecord(
            routineName = routineName,
            durationSeconds = ((System.currentTimeMillis() - startedAt) / 1_000).toInt().coerceAtLeast(1),
            exercises = exercises,
        )
        val nextHistory = (listOf(record) + history()).take(100)
        prefs.edit().putString("history", encodeHistory(nextHistory).toString()).apply()
        return nextRoutines
    }

    private fun cleanExercise(exercise: MavStrengthExercise): MavStrengthExercise =
        exercise.copy(
            note = "",
            previous = "—",
            sets = exercise.sets.map { it.copy(complete = false) },
        )

    private fun encodeRoutines(values: List<MavStrengthRoutine>) =
        JSONArray().apply { values.forEach { put(encodeRoutine(it)) } }

    private fun decodeRoutines(text: String): List<MavStrengthRoutine> {
        val array = JSONArray(text)
        return List(array.length()) { decodeRoutine(array.getJSONObject(it)) }
    }

    private fun encodeRoutine(value: MavStrengthRoutine) = JSONObject()
        .put("id", value.id)
        .put("name", value.name)
        .put("exercises", encodeExercises(value.exercises))

    private fun decodeRoutine(value: JSONObject) = MavStrengthRoutine(
        id = value.optString("id", UUID.randomUUID().toString()),
        name = value.getString("name"),
        exercises = decodeExercises(value.getJSONArray("exercises")),
    )

    private fun encodeExercises(values: List<MavStrengthExercise>) =
        JSONArray().apply {
            values.forEach { exercise ->
                put(
                    JSONObject()
                        .put("id", exercise.id)
                        .put("name", exercise.name)
                        .put("category", exercise.category)
                        .put("note", exercise.note)
                        .put("previous", exercise.previous)
                        .put(
                            "sets",
                            JSONArray().apply {
                                exercise.sets.forEach { set ->
                                    put(
                                        JSONObject()
                                            .put("id", set.id)
                                            .put("kind", set.kind.name)
                                            .put("weight", set.weight)
                                            .put("reps", set.reps)
                                            .put("rir", set.rir)
                                            .put("complete", set.complete),
                                    )
                                }
                            },
                        ),
                )
            }
        }

    private fun decodeExercises(values: JSONArray): List<MavStrengthExercise> =
        List(values.length()) { index ->
            val exercise = values.getJSONObject(index)
            val sets = exercise.getJSONArray("sets")
            MavStrengthExercise(
                id = exercise.optString("id", UUID.randomUUID().toString()),
                name = exercise.getString("name"),
                category = exercise.optString("category", "Other"),
                note = exercise.optString("note"),
                previous = exercise.optString("previous", "—"),
                sets = List(sets.length()) { setIndex ->
                    val set = sets.getJSONObject(setIndex)
                    MavStrengthSet(
                        id = set.optString("id", UUID.randomUUID().toString()),
                        kind = runCatching {
                            MavStrengthSetKind.valueOf(set.optString("kind"))
                        }.getOrDefault(MavStrengthSetKind.WORKING),
                        weight = set.optString("weight"),
                        reps = set.optString("reps", "8"),
                        rir = set.optString("rir", "2"),
                        complete = set.optBoolean("complete"),
                    )
                },
            )
        }

    private fun encodeHistory(values: List<MavStrengthWorkoutRecord>) =
        JSONArray().apply {
            values.forEach { value ->
                put(
                    JSONObject()
                        .put("id", value.id)
                        .put("timestamp", value.timestamp)
                        .put("routineName", value.routineName)
                        .put("durationSeconds", value.durationSeconds)
                        .put("exercises", encodeExercises(value.exercises)),
                )
            }
        }

    private fun decodeHistory(text: String): List<MavStrengthWorkoutRecord> {
        val array = JSONArray(text)
        return List(array.length()) { index ->
            val value = array.getJSONObject(index)
            MavStrengthWorkoutRecord(
                id = value.optString("id", UUID.randomUUID().toString()),
                timestamp = value.optLong("timestamp"),
                routineName = value.getString("routineName"),
                durationSeconds = value.optInt("durationSeconds"),
                exercises = decodeExercises(value.getJSONArray("exercises")),
            )
        }
    }
}
