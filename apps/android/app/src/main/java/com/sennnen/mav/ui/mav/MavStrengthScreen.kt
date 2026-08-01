package com.sennnen.mav.ui.mav

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.SwapHoriz
import androidx.compose.material.icons.outlined.RadioButtonUnchecked
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import java.text.DateFormat
import java.util.Date
import kotlin.math.roundToInt

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MavStrengthScreen(usingFixture: Boolean, onBack: () -> Unit) {
    val context = LocalContext.current
    val store = remember { MavStrengthStore(context) }
    var routines by remember { mutableStateOf(store.routines()) }
    var history by remember { mutableStateOf(store.history()) }
    var logging by remember { mutableStateOf(false) }
    var sessionName by remember { mutableStateOf("New workout") }
    var exercises by remember { mutableStateOf<List<MavStrengthExercise>>(emptyList()) }
    var startedAt by remember { mutableLongStateOf(System.currentTimeMillis()) }
    var showBuilder by remember { mutableStateOf(false) }

    fun begin(routine: MavStrengthRoutine?) {
        sessionName = routine?.name ?: "New workout"
        exercises = MavStrengthLibrary.freshExercises(routine)
        if (usingFixture && routine == null) {
            sessionName = "Push day"
            exercises = MavStrengthLibrary.starterRoutines[1].exercises.take(3).map { exercise ->
                exercise.copy(
                    previous = if (exercise.name == "Bench press") "60 × 8" else "—",
                    sets = exercise.sets.mapIndexed { index, set ->
                        if (exercise.name == "Bench press") {
                            set.copy(
                                weight = if (index < 2) "60" else "62.5",
                                reps = if (index < 2) "8" else "6",
                                complete = index < 2,
                            )
                        } else {
                            set
                        }
                    },
                )
            }
        }
        startedAt = System.currentTimeMillis()
        logging = true
    }

    MavDetailScaffold(
        if (logging) sessionName else "Strength",
        onBack = {
            if (logging) logging = false else onBack()
        },
    ) {
        if (logging) {
            MavStrengthLogger(
                sessionName = sessionName,
                exercises = exercises,
                startedAt = startedAt,
                onExercises = { exercises = it },
                onFinish = { saveRoutine ->
                    routines = store.finish(
                        routineName = sessionName,
                        startedAt = startedAt,
                        exercises = exercises,
                        saveRoutine = saveRoutine,
                        routines = routines,
                    )
                    history = store.history()
                    onBack()
                },
            )
        } else {
            Text(
                "Start with a routine",
                style = MavType.display,
                color = MavTheme.palette.ink,
                modifier = Modifier.padding(top = 12.dp),
            )
            Text(
                "Your exercises, set types and targets are ready before the first set.",
                style = MavType.body,
                color = MavTheme.palette.inkSecondary,
            )
            Button(
                onClick = { begin(null) },
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) {
                Icon(Icons.Filled.PlayArrow, contentDescription = null)
                Text("Start empty workout", modifier = Modifier.padding(start = 8.dp))
            }

            MavSectionHeader("Routines")
            MavTile(padded = false) {
                routines.forEachIndexed { index, routine ->
                    if (index > 0) MavDivider()
                    MavRow(
                        routine.name,
                        routineSummary(routine),
                        modifier = Modifier.clickable { begin(routine) },
                        trailing = { Icon(MavIcons.chevronRight, contentDescription = null) },
                    )
                }
            }

            OutlinedButton(
                onClick = { showBuilder = true },
                modifier = Modifier.fillMaxWidth().height(50.dp),
            ) {
                Icon(Icons.Filled.Add, contentDescription = null)
                Text("Create routine", modifier = Modifier.padding(start = 8.dp))
            }

            if (history.isNotEmpty()) {
                MavSectionHeader("Recent strength")
                MavTile(padded = false) {
                    history.take(4).forEachIndexed { index, record ->
                        if (index > 0) MavDivider()
                        MavRow(
                            record.routineName,
                            "${DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(record.timestamp))} · " +
                                "${record.completedSets} sets",
                        )
                    }
                }
            }
        }
    }

    if (showBuilder) {
        MavRoutineBuilderSheet(
            onDismiss = { showBuilder = false },
            onSave = { name, selected ->
                val routine = MavStrengthRoutine(
                    name = name,
                    exercises = selected.map { MavStrengthLibrary.exercise(it.name) },
                )
                routines = listOf(routine) + routines
                store.saveRoutines(routines)
                showBuilder = false
            },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MavStrengthLogger(
    sessionName: String,
    exercises: List<MavStrengthExercise>,
    startedAt: Long,
    onExercises: (List<MavStrengthExercise>) -> Unit,
    onFinish: (Boolean) -> Unit,
) {
    var elapsed by remember { mutableIntStateOf(0) }
    var restSeconds by remember { mutableStateOf<Int?>(null) }
    var showPicker by remember { mutableStateOf(false) }
    var replacementIndex by remember { mutableStateOf<Int?>(null) }
    var confirmFinish by remember { mutableStateOf(false) }
    var saveRoutine by remember { mutableStateOf(false) }

    LaunchedEffect(startedAt) {
        while (true) {
            elapsed = ((System.currentTimeMillis() - startedAt) / 1_000).toInt().coerceAtLeast(0)
            delay(1_000)
        }
    }
    LaunchedEffect(restSeconds) {
        val remaining = restSeconds ?: return@LaunchedEffect
        if (remaining > 0) {
            delay(1_000)
            restSeconds = remaining - 1
        }
    }

    MavTile {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            MavStrengthStat("Elapsed", "%02d:%02d".format(elapsed / 60, elapsed % 60))
            MavStrengthStat(
                "Completed",
                "${exercises.sumOf { exercise -> exercise.sets.count { it.complete } }} sets",
                Alignment.CenterHorizontally,
            )
            val volume = exercises.flatMap { it.sets }.filter { it.complete }.sumOf { set ->
                (set.weight.toDoubleOrNull() ?: 0.0) * (set.reps.toIntOrNull() ?: 0)
            }
            MavStrengthStat(
                "Volume",
                if (volume >= 1_000) "%.1ft".format(volume / 1_000) else "${volume.roundToInt()}kg",
                Alignment.End,
            )
        }
    }

    restSeconds?.let { remaining ->
        MavTile {
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(if (remaining > 0) "Rest" else "Rest complete", style = MavType.caption)
                    Text(
                        "${remaining / 60}:${"%02d".format(remaining % 60)}",
                        style = MavType.numeralSmall,
                    )
                }
                if (remaining > 0) {
                    OutlinedButton(onClick = { restSeconds = remaining + 30 }) { Text("+30s") }
                    TextButton(onClick = { restSeconds = null }) { Text("Skip") }
                } else {
                    TextButton(onClick = { restSeconds = null }) { Text("Dismiss") }
                }
            }
        }
    }

    if (exercises.isEmpty()) {
        MavTile {
            Text("Add your first exercise", style = MavType.title)
            Text(
                "Build the workout as you go, or save it as a routine when you finish.",
                style = MavType.body,
                color = MavTheme.palette.inkSecondary,
                modifier = Modifier.padding(top = 6.dp),
            )
        }
    }

    exercises.forEachIndexed { exerciseIndex, exercise ->
        MavStrengthExerciseCard(
            exercise = exercise,
            exerciseIndex = exerciseIndex,
            exerciseCount = exercises.size,
            onExercise = { updated ->
                onExercises(exercises.mapIndexed { index, item ->
                    if (index == exerciseIndex) updated else item
                })
            },
            onMove = { delta ->
                val target = exerciseIndex + delta
                if (target in exercises.indices) {
                    val mutable = exercises.toMutableList()
                    val item = mutable.removeAt(exerciseIndex)
                    mutable.add(target, item)
                    onExercises(mutable)
                }
            },
            onReplace = {
                replacementIndex = exerciseIndex
                showPicker = true
            },
            onRemove = { onExercises(exercises.filterIndexed { index, _ -> index != exerciseIndex }) },
            onComplete = { restSeconds = 90 },
        )
    }

    OutlinedButton(
        onClick = {
            replacementIndex = null
            showPicker = true
        },
        modifier = Modifier.fillMaxWidth().height(50.dp),
    ) {
        Icon(Icons.Filled.Add, contentDescription = null)
        Text("Add exercise", modifier = Modifier.padding(start = 8.dp))
    }

    Button(
        onClick = { confirmFinish = true },
        modifier = Modifier.fillMaxWidth().height(52.dp),
    ) {
        Text("Finish workout")
    }

    if (showPicker) {
        MavExercisePickerSheet(
            excluding = if (replacementIndex == null) exercises.map { it.name }.toSet() else emptySet(),
            onDismiss = { showPicker = false },
            onSelect = { definition ->
                val item = MavStrengthLibrary.exercise(definition.name)
                onExercises(
                    if (replacementIndex != null) {
                        exercises.mapIndexed { index, current ->
                            if (index == replacementIndex) item else current
                        }
                    } else {
                        exercises + item
                    },
                )
                showPicker = false
            },
        )
    }

    if (confirmFinish) {
        AlertDialog(
            onDismissRequest = { confirmFinish = false },
            title = { Text("Finish workout?") },
            text = {
                Column {
                    Text(
                        "${exercises.sumOf { it.sets.count { set -> set.complete } }} completed sets · " +
                            "${exercises.size} exercises.",
                    )
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .padding(top = 16.dp)
                            .clickable { saveRoutine = !saveRoutine },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        androidx.compose.material3.Checkbox(
                            checked = saveRoutine,
                            onCheckedChange = { saveRoutine = it },
                        )
                        Text("Save $sessionName as a routine")
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { onFinish(saveRoutine) }) { Text("Finish") }
            },
            dismissButton = {
                TextButton(onClick = { confirmFinish = false }) { Text("Keep logging") }
            },
        )
    }
}

@Composable
private fun MavStrengthExerciseCard(
    exercise: MavStrengthExercise,
    exerciseIndex: Int,
    exerciseCount: Int,
    onExercise: (MavStrengthExercise) -> Unit,
    onMove: (Int) -> Unit,
    onReplace: () -> Unit,
    onRemove: () -> Unit,
    onComplete: () -> Unit,
) {
    var actionsOpen by remember { mutableStateOf(false) }
    MavTile {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Top) {
            Column(Modifier.weight(1f)) {
                Text(exercise.name, style = MavType.title)
                Text(exercise.category, style = MavType.sub, color = MavTheme.palette.inkSecondary)
            }
            Box {
                IconButton(onClick = { actionsOpen = true }) {
                    Icon(Icons.Filled.MoreVert, contentDescription = "Actions for ${exercise.name}")
                }
                DropdownMenu(expanded = actionsOpen, onDismissRequest = { actionsOpen = false }) {
                    DropdownMenuItem(
                        text = { Text("Replace exercise") },
                        leadingIcon = { Icon(Icons.Filled.SwapHoriz, null) },
                        onClick = { actionsOpen = false; onReplace() },
                    )
                    DropdownMenuItem(
                        text = { Text("Move up") },
                        leadingIcon = { Icon(Icons.Filled.KeyboardArrowUp, null) },
                        enabled = exerciseIndex > 0,
                        onClick = { actionsOpen = false; onMove(-1) },
                    )
                    DropdownMenuItem(
                        text = { Text("Move down") },
                        leadingIcon = { Icon(Icons.Filled.KeyboardArrowDown, null) },
                        enabled = exerciseIndex < exerciseCount - 1,
                        onClick = { actionsOpen = false; onMove(1) },
                    )
                    HorizontalDivider()
                    DropdownMenuItem(
                        text = { Text("Remove exercise") },
                        leadingIcon = { Icon(Icons.Filled.Delete, null) },
                        onClick = { actionsOpen = false; onRemove() },
                    )
                }
            }
        }

        MavStrengthColumnsHeader()
        exercise.sets.forEachIndexed { setIndex, set ->
            MavStrengthSetRow(
                set = set,
                number = setIndex + 1,
                previous = exercise.previous,
                onSet = { updated ->
                    onExercise(
                        exercise.copy(
                            sets = exercise.sets.mapIndexed { index, item ->
                                if (index == setIndex) updated else item
                            },
                        ),
                    )
                },
                onRemove = {
                    onExercise(exercise.copy(sets = exercise.sets.filterIndexed { index, _ -> index != setIndex }))
                },
                onComplete = onComplete,
            )
        }

        TextButton(
            onClick = {
                val last = exercise.sets.lastOrNull() ?: MavStrengthSet()
                onExercise(
                    exercise.copy(
                        sets = exercise.sets + MavStrengthSet(
                            weight = last.weight,
                            reps = last.reps,
                            rir = last.rir,
                        ),
                    ),
                )
            },
            modifier = Modifier.fillMaxWidth().padding(top = 6.dp),
        ) {
            Icon(Icons.Filled.Add, contentDescription = null, modifier = Modifier.size(18.dp))
            Text("Add set", modifier = Modifier.padding(start = 6.dp))
        }

        OutlinedTextField(
            value = exercise.note,
            onValueChange = { onExercise(exercise.copy(note = it)) },
            modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
            label = { Text("Exercise note") },
            minLines = 1,
            maxLines = 3,
        )
    }
}

@Composable
private fun MavStrengthColumnsHeader() {
    Row(
        Modifier.fillMaxWidth().padding(top = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        Text("Set", style = MavType.caption, textAlign = TextAlign.Center, modifier = Modifier.width(42.dp))
        Text("Prev", style = MavType.caption, textAlign = TextAlign.Center, modifier = Modifier.width(48.dp))
        Text("kg", style = MavType.caption, textAlign = TextAlign.Center, modifier = Modifier.weight(1f))
        Text("Reps", style = MavType.caption, textAlign = TextAlign.Center, modifier = Modifier.width(52.dp))
        Text("RIR", style = MavType.caption, textAlign = TextAlign.Center, modifier = Modifier.width(44.dp))
        Spacer(Modifier.width(48.dp))
    }
}

@Composable
private fun MavStrengthSetRow(
    set: MavStrengthSet,
    number: Int,
    previous: String,
    onSet: (MavStrengthSet) -> Unit,
    onRemove: () -> Unit,
    onComplete: () -> Unit,
) {
    var typeOpen by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().padding(top = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        Box(Modifier.width(42.dp)) {
            Surface(
                onClick = { typeOpen = true },
                modifier = Modifier.size(42.dp),
                shape = RoundedCornerShape(12.dp),
                color = MavTheme.palette.raised,
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Text(
                        if (set.kind == MavStrengthSetKind.WORKING) "$number" else set.kind.shortLabel,
                        style = MavType.label,
                    )
                }
            }
            DropdownMenu(expanded = typeOpen, onDismissRequest = { typeOpen = false }) {
                MavStrengthSetKind.entries.forEach { kind ->
                    DropdownMenuItem(
                        text = { Text(kind.label) },
                        onClick = { onSet(set.copy(kind = kind)); typeOpen = false },
                    )
                }
                HorizontalDivider()
                DropdownMenuItem(text = { Text("Remove set") }, onClick = {
                    typeOpen = false
                    onRemove()
                })
            }
        }

        Text(
            previous,
            style = MavType.caption,
            color = MavTheme.palette.inkSecondary,
            textAlign = TextAlign.Center,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.width(48.dp),
        )
        MavCompactStrengthField(
            set.weight,
            { onSet(set.copy(weight = it)) },
            "Weight in kilograms",
            decimal = true,
            modifier = Modifier.weight(1f),
        )
        MavCompactStrengthField(
            set.reps,
            { onSet(set.copy(reps = it)) },
            "Repetitions",
            modifier = Modifier.width(52.dp),
        )
        MavCompactStrengthField(
            set.rir,
            { onSet(set.copy(rir = it)) },
            "Reps in reserve",
            modifier = Modifier.width(44.dp),
        )
        IconButton(
            onClick = {
                onSet(set.copy(complete = !set.complete))
                if (!set.complete) onComplete()
            },
            modifier = Modifier.size(48.dp),
        ) {
            Icon(
                if (set.complete) Icons.Filled.CheckCircle else Icons.Outlined.RadioButtonUnchecked,
                contentDescription = if (set.complete) "Set completed" else "Complete set",
                tint = if (set.complete) MavTheme.palette.ink else MavTheme.palette.inkSecondary,
            )
        }
    }
}

@Composable
private fun MavCompactStrengthField(
    value: String,
    onValueChange: (String) -> Unit,
    spoken: String,
    modifier: Modifier,
    decimal: Boolean = false,
) {
    Surface(
        modifier = modifier.height(44.dp),
        shape = RoundedCornerShape(12.dp),
        color = MavTheme.palette.sunken,
    ) {
        Box(contentAlignment = Alignment.Center) {
            BasicTextField(
                value = value,
                onValueChange = { candidate ->
                    val valid = candidate.length <= 6 &&
                        candidate.count { it == '.' } <= (if (decimal) 1 else 0) &&
                        candidate.all { it.isDigit() || decimal && it == '.' }
                    if (valid) onValueChange(candidate)
                },
                textStyle = MavType.label.copy(
                    color = MavTheme.palette.ink,
                    textAlign = TextAlign.Center,
                ),
                singleLine = true,
                keyboardOptions = KeyboardOptions(
                    keyboardType = if (decimal) KeyboardType.Decimal else KeyboardType.Number,
                ),
                modifier = Modifier
                    .fillMaxWidth()
                    .semantics { contentDescription = spoken },
            )
        }
    }
}

@Composable
private fun MavStrengthStat(
    label: String,
    value: String,
    alignment: Alignment.Horizontal = Alignment.Start,
) {
    Column(horizontalAlignment = alignment) {
        Text(label, style = MavType.caption, color = MavTheme.palette.inkSecondary)
        Text(value, style = MavType.numeralSmall, modifier = Modifier.padding(top = 4.dp))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MavExercisePickerSheet(
    excluding: Set<String>,
    onDismiss: () -> Unit,
    onSelect: (MavExerciseDefinition) -> Unit,
) {
    var query by remember { mutableStateOf("") }
    var category by remember { mutableStateOf("All") }
    val filtered = MavStrengthLibrary.exercises.filter { exercise ->
        exercise.name !in excluding &&
            (category == "All" || exercise.category == category) &&
            (query.isBlank() || exercise.name.contains(query, ignoreCase = true))
    }
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp).padding(bottom = 24.dp)) {
            Text("Add exercise", style = MavType.title)
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                label = { Text("Search exercises") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
            )
            Row(
                Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState())
                    .padding(vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                (listOf("All") + MavStrengthLibrary.categories).forEach { item ->
                    FilterChip(
                        selected = category == item,
                        onClick = { category = item },
                        label = { Text(item) },
                    )
                }
            }
            filtered.take(12).forEach { exercise ->
                MavRow(
                    exercise.name,
                    exercise.category,
                    modifier = Modifier.clickable { onSelect(exercise) },
                    trailing = { Icon(Icons.Filled.Add, contentDescription = null) },
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MavRoutineBuilderSheet(
    onDismiss: () -> Unit,
    onSave: (String, List<MavExerciseDefinition>) -> Unit,
) {
    var name by remember { mutableStateOf("") }
    var query by remember { mutableStateOf("") }
    var selected by remember { mutableStateOf<List<MavExerciseDefinition>>(emptyList()) }
    val filtered = MavStrengthLibrary.exercises.filter {
        query.isBlank() || it.name.contains(query, ignoreCase = true)
    }
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp).padding(bottom = 28.dp)) {
            Text("Create routine", style = MavType.title)
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("Routine name") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
            )
            if (selected.isNotEmpty()) {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .horizontalScroll(rememberScrollState())
                        .padding(top = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    selected.forEach { exercise ->
                        AssistChip(
                            onClick = { selected = selected - exercise },
                            label = { Text(exercise.name) },
                        )
                    }
                }
            }
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                label = { Text("Find exercises") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
            )
            filtered.take(7).forEach { exercise ->
                MavRow(
                    exercise.name,
                    exercise.category,
                    modifier = Modifier.clickable {
                        if (exercise !in selected) selected = selected + exercise
                    },
                    trailing = {
                        Text(
                            if (exercise in selected) "Added" else "Add",
                            style = MavType.label,
                            color = MavTheme.palette.inkSecondary,
                        )
                    },
                )
            }
            Button(
                onClick = { onSave(name.trim(), selected) },
                enabled = name.isNotBlank() && selected.isNotEmpty(),
                modifier = Modifier.fillMaxWidth().height(52.dp).padding(top = 10.dp),
            ) {
                Text("Save routine")
            }
        }
    }
}

private fun routineSummary(routine: MavStrengthRoutine): String {
    val visible = routine.exercises.take(3).joinToString(" · ") { it.name }
    return if (routine.exercises.size > 3) "$visible +${routine.exercises.size - 3}" else visible
}
