package com.example.subroutine_simple.ui.creator

import android.text.format.DateFormat
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.CalendarMonth
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api

import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.ui.components.ItemIdentityEditor
import com.example.subroutine_simple.ui.components.ProgressiveEditorSection
import com.example.subroutine_simple.ui.components.editorDatePickerColors
import com.example.subroutine_simple.ui.components.editorTimePickerColors

import com.example.subroutine_simple.ui.theme.subroutineButtonColors

import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

internal val LocalCreatorControlsEnabled = staticCompositionLocalOf { true }

private val dateFormatter: DateTimeFormatter = DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)
private val timeFormatter: DateTimeFormatter = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MaterialItemCreator(
    state: ItemCreatorState,
    onStateChange: (ItemCreatorState) -> Unit,
    onCreate: (CreateItemRequest) -> Unit,
    modifier: Modifier = Modifier,
    onCancel: (() -> Unit)? = null,
    isSubmitting: Boolean = false,
    errorMessage: String? = null,
) {
    var showValidation by rememberSaveable { mutableStateOf(false) }
    val validation = state.validate()

    fun submit() {
        when (val result = state.buildRequest()) {
            is CreateItemBuildResult.Invalid -> showValidation = true
            is CreateItemBuildResult.Success -> onCreate(result.request)
        }
    }

    Surface(
        modifier = modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        contentColor = MaterialTheme.colorScheme.onSurface,
        shape = MaterialTheme.shapes.extraLarge,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp, vertical = 24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .widthIn(max = 840.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Text(
                    text = "New ${state.selectedType.label.lowercase()}",
                    style = MaterialTheme.typography.titleLarge,
                )

                ItemIdentityEditor(
                    title = state.title,
                    onTitleChange = { onStateChange(state.copy(title = it)) },
                    notes = state.notes,
                    onNotesChange = { onStateChange(state.copy(notes = it)) },
                    enabled = !isSubmitting,
                    titlePlaceholder = when (state.selectedType) {
                        CreatorItemType.Action -> "What do you want to do?"
                        CreatorItemType.Event -> "What is happening?"
                        CreatorItemType.Marker -> "What should this time mean?"
                        CreatorItemType.Signal -> "What should you notice?"
                        CreatorItemType.Routine -> "Name this routine"
                    },
                    showNotes = false,
                    autoFocus = false,
                    titleImeAction = ImeAction.Done,
                    onTitleImeAction = { if (!isSubmitting) submit() },
                )
                FieldError(showValidation, validation.messageFor(CreatorField.Title))

                ProgressiveEditorSection(
                    title = "Details",
                    summary = creatorDetailsSummary(state),
                    enabled = !isSubmitting,
                    forceExpanded = showValidation && !validation.isValid,
                ) {
                    OutlinedTextField(
                        value = state.notes,
                        onValueChange = { onStateChange(state.copy(notes = it)) },
                        label = { Text("Notes") },
                        minLines = 3,
                        maxLines = 7,
                        enabled = !isSubmitting,
                        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                        modifier = Modifier.fillMaxWidth(),
                        shape = MaterialTheme.shapes.large,
                    )

                    when (state.selectedType) {
                        CreatorItemType.Action -> ActionFields(state, showValidation, onStateChange)
                        CreatorItemType.Event -> EventFields(state, showValidation, onStateChange)
                        CreatorItemType.Marker -> MarkerFields(state, showValidation, onStateChange)
                        CreatorItemType.Signal -> SignalFields(state, showValidation, onStateChange)
                        CreatorItemType.Routine -> RoutineFields(state, showValidation, onStateChange)
                    }
                }

                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)

                (errorMessage ?: validation.firstMessage?.takeIf { showValidation })?.let { message ->
                    Text(
                        text = message,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                    )
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    onCancel?.let { cancel ->
                        TextButton(onClick = cancel, enabled = !isSubmitting) {
                            Text("Cancel")
                        }
                    }
                    Button(
                        onClick = ::submit,
                        enabled = !isSubmitting,
                        colors = subroutineButtonColors(),
                    ) {
                        Text(if (isSubmitting) "Creating…" else "Create")
                    }
                }
            }
        }
    }
}

@Composable
internal fun ActionFields(
    state: ItemCreatorState,
    showValidation: Boolean,
    onStateChange: (ItemCreatorState) -> Unit,
) {
    ProgressiveEditorSection(
        title = "Plan",
        summary = when {
            state.actionPinned -> "${creatorScheduleSummary(state.schedule)} · pinned"
            state.actionQueued && state.schedule.hasDate -> creatorScheduleSummary(state.schedule)
            state.actionQueued -> "Any time in the queue"
            state.schedule.hasDate -> "Due ${creatorScheduleSummary(state.schedule)}"
            else -> "Unqueued, no date"
        },
        forceExpanded = showValidation && !state.validate().isValid,
    ) {
        SettingSwitch(
            label = "Add to queue",
            checked = state.actionQueued,
            onCheckedChange = { onStateChange(state.withActionQueued(it)) },
        )
        SettingSwitch(
            label = "Pin selected time",
            supportingText = if (state.canPinAction) {
                "The scheduler won’t move this action."
            } else {
                "Choose a date and time and add to the queue first."
            },
            checked = state.actionPinned,
            enabled = state.canPinAction && LocalCreatorControlsEnabled.current,
            onCheckedChange = { onStateChange(state.withActionPinned(it)) },
        )
        FieldError(showValidation, state.validate().messageFor(CreatorField.Queue))

        ScheduleFields(
            schedule = state.schedule,
            onDateChange = { onStateChange(state.withScheduleDate(it)) },
            onTimeChange = { onStateChange(state.withScheduleTime(it)) },
            dateRequired = false,
            timeRequired = false,
            showValidation = showValidation,
            validation = state.validate(),
        )
        DurationStepper(
            label = "Duration (optional)",
            duration = state.duration,
            stepMinutes = 15,
            onStep = { onStateChange(state.stepDuration(it)) },
            validationMessage = state.validate().messageFor(CreatorField.Duration).takeIf { showValidation },
        )
    }
}

@Composable
internal fun EventFields(
    state: ItemCreatorState,
    showValidation: Boolean,
    onStateChange: (ItemCreatorState) -> Unit,
) {
    ProgressiveEditorSection(
        title = "When",
        summary = "${creatorScheduleSummary(state.schedule)} · ${state.duration?.let(::formatDuration) ?: "set duration"}",
        forceExpanded = showValidation && !state.validate().isValid,
    ) {
        ScheduleFields(
            schedule = state.schedule,
            onDateChange = { onStateChange(state.withScheduleDate(it)) },
            onTimeChange = { onStateChange(state.withScheduleTime(it)) },
            dateRequired = true,
            timeRequired = true,
            showValidation = showValidation,
            validation = state.validate(),
        )
        DurationStepper(
            label = "Duration",
            duration = state.duration,
            stepMinutes = 15,
            onStep = { onStateChange(state.stepDuration(it)) },
            validationMessage = state.validate().messageFor(CreatorField.Duration).takeIf { showValidation },
        )
    }
}

@Composable
internal fun MarkerFields(
    state: ItemCreatorState,
    showValidation: Boolean,
    onStateChange: (ItemCreatorState) -> Unit,
) {
    ProgressiveEditorSection(
        title = "When",
        summary = markerRangeSummary(state),
        forceExpanded = showValidation && !state.validate().isValid,
    ) {
        DateField(
            label = "Start date",
            date = state.schedule.date,
            required = true,
            onDateChange = { onStateChange(state.withScheduleDate(it)) },
        )
        FieldError(showValidation, state.validate().messageFor(CreatorField.Date))
        DateField(
            label = "End date (inclusive)",
            date = state.markerEndDate,
            required = true,
            onDateChange = { onStateChange(state.copy(markerEndDate = it)) },
        )
        FieldError(showValidation, state.validate().messageFor(CreatorField.MarkerEndDate))
        val start = state.schedule.date
        val end = state.markerEndDate
        if (start != null && end != null && !end.isBefore(start)) {
            val days = java.time.temporal.ChronoUnit.DAYS.between(start, end) + 1
            Text(
                text = if (days == 1L) "One day" else "$days days, including both dates",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
internal fun SignalFields(
    state: ItemCreatorState,
    showValidation: Boolean,
    onStateChange: (ItemCreatorState) -> Unit,
) {
    ProgressiveEditorSection(
        title = "Fire at",
        summary = creatorScheduleSummary(state.schedule),
        forceExpanded = showValidation && !state.validate().isValid,
    ) {
        ScheduleFields(
            schedule = state.schedule,
            onDateChange = { onStateChange(state.withScheduleDate(it)) },
            onTimeChange = { onStateChange(state.withScheduleTime(it)) },
            dateRequired = true,
            timeRequired = true,
            showValidation = showValidation,
            validation = state.validate(),
        )
    }
}

@Composable
private fun RoutineFields(
    state: ItemCreatorState,
    showValidation: Boolean,
    onStateChange: (ItemCreatorState) -> Unit,
) {
    Text(
        text = "Steps",
        style = MaterialTheme.typography.titleMedium,
        color = MaterialTheme.colorScheme.onSurface,
    )
    var newStepTitle by rememberSaveable { mutableStateOf("") }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OutlinedTextField(
            value = newStepTitle,
            onValueChange = { newStepTitle = it },
            label = { Text("New step") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                capitalization = KeyboardCapitalization.Sentences,
                imeAction = ImeAction.Done,
            ),
            keyboardActions = KeyboardActions(
                onDone = {
                    if (newStepTitle.isNotBlank()) {
                        onStateChange(state.addRoutineStep(newStepTitle))
                        newStepTitle = ""
                    }
                },
            ),
            modifier = Modifier.weight(1f),
            shape = MaterialTheme.shapes.large,
        )
        IconButton(
            onClick = {
                onStateChange(state.addRoutineStep(newStepTitle))
                newStepTitle = ""
            },
            enabled = newStepTitle.isNotBlank(),
            modifier = Modifier.sizeIn(minWidth = 48.dp, minHeight = 48.dp),
        ) {
            Icon(Icons.Default.Add, contentDescription = "Add routine step")
        }
    }

    state.routineSteps.forEachIndexed { index, step ->
        key(step.id) {
            RoutineStepRow(
                index = index,
                count = state.routineSteps.size,
                step = step,
                onTitleChange = { onStateChange(state.updateRoutineStepTitle(step.id, it)) },
                onMove = { onStateChange(state.moveRoutineStep(step.id, it)) },
                onDurationStep = { onStateChange(state.stepRoutineDuration(step.id, it)) },
                onRemove = { onStateChange(state.removeRoutineStep(step.id)) },
            )
        }
    }
    FieldError(showValidation, state.validate().messageFor(CreatorField.RoutineSteps))

    ProgressiveEditorSection(
        title = "Target",
        summary = creatorScheduleSummary(state.schedule, empty = "No target"),
    ) {
        ScheduleFields(
            schedule = state.schedule,
            onDateChange = { onStateChange(state.withScheduleDate(it)) },
            onTimeChange = { onStateChange(state.withScheduleTime(it)) },
            dateRequired = false,
            timeRequired = false,
            showValidation = false,
            validation = CreatorValidation(emptyList()),
        )
    }
}

@Composable
internal fun RoutineStepRow(
    index: Int,
    count: Int,
    step: RoutineStepDraft,
    onTitleChange: (String) -> Unit,
    onMove: (Int) -> Unit,
    onDurationStep: (Int) -> Unit,
    onRemove: () -> Unit,
) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainer,
        contentColor = MaterialTheme.colorScheme.onSurface,
        shape = MaterialTheme.shapes.large,
    ) {
        Column(
            modifier = Modifier.padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(
                    text = "${index + 1}",
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = step.title,
                    onValueChange = onTitleChange,
                    label = { Text("Step ${index + 1}") },
                    enabled = LocalCreatorControlsEnabled.current,
                    singleLine = true,
                    modifier = Modifier.weight(1f),
                    shape = MaterialTheme.shapes.medium,
                )
                IconButton(onClick = { onMove(-1) }, enabled = LocalCreatorControlsEnabled.current && index > 0) {
                    Icon(Icons.Default.ArrowUpward, contentDescription = "Move step ${index + 1} up")
                }
                IconButton(
                    onClick = { onMove(1) },
                    enabled = LocalCreatorControlsEnabled.current && index < count - 1
                ) {
                    Icon(Icons.Default.ArrowDownward, contentDescription = "Move step ${index + 1} down")
                }
                IconButton(onClick = onRemove, enabled = LocalCreatorControlsEnabled.current) {
                    Icon(Icons.Default.Close, contentDescription = "Remove step ${index + 1}")
                }
            }
            DurationStepper(
                label = "Step duration",
                duration = step.duration,
                stepMinutes = 5,
                onStep = onDurationStep,
                compact = true,
            )
        }
    }
}

internal fun creatorDetailsSummary(state: ItemCreatorState): String {
    val notes = "notes".takeIf { state.notes.isNotBlank() }
    val plan = when (state.selectedType) {
        CreatorItemType.Action -> when {
            state.actionPinned -> "${creatorScheduleSummary(state.schedule)} · pinned"
            state.actionQueued -> creatorScheduleSummary(state.schedule, empty = "any time")
            state.schedule.hasDate -> "due ${creatorScheduleSummary(state.schedule)}"
            else -> null
        }

        CreatorItemType.Event -> creatorScheduleSummary(state.schedule)
        CreatorItemType.Marker -> markerRangeSummary(state)
        CreatorItemType.Signal -> creatorScheduleSummary(state.schedule)
        CreatorItemType.Routine -> "${state.routineSteps.size} steps".takeIf { state.routineSteps.isNotEmpty() }
    }
    return listOfNotNull(state.selectedType.label, plan, notes).joinToString(" · ")
}

internal fun creatorScheduleSummary(
    schedule: CreatorSchedule,
    empty: String = "Choose when",
): String = when {
    schedule.date == null -> empty
    schedule.time == null -> schedule.date.format(dateFormatter)
    else -> "${schedule.date.format(dateFormatter)}, ${schedule.time.format(timeFormatter)}"
}

internal fun markerRangeSummary(state: ItemCreatorState): String {
    val start = state.schedule.date ?: return "Choose dates"
    val end = state.markerEndDate ?: return start.format(dateFormatter)
    return if (start == end) {
        start.format(dateFormatter)
    } else {
        "${start.format(dateFormatter)} – ${end.format(dateFormatter)}"
    }
}

@Composable
internal fun ScheduleFields(
    schedule: CreatorSchedule,
    onDateChange: (LocalDate?) -> Unit,
    onTimeChange: (LocalTime?) -> Unit,
    dateRequired: Boolean,
    timeRequired: Boolean,
    showValidation: Boolean,
    validation: CreatorValidation,
) {
    DateField(
        label = if (dateRequired) "Date" else "Date (optional)",
        date = schedule.date,
        required = dateRequired,
        onDateChange = onDateChange,
    )
    FieldError(showValidation, validation.messageFor(CreatorField.Date))
    TimeField(
        label = if (timeRequired) "Time" else "Time (optional)",
        time = schedule.time,
        required = timeRequired,
        enabled = schedule.date != null && LocalCreatorControlsEnabled.current,
        onTimeChange = onTimeChange,
    )
    if (schedule.date == null && !timeRequired) {
        Text(
            text = "Choose a date before adding a time.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    FieldError(showValidation, validation.messageFor(CreatorField.Time))
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DateField(
    label: String,
    date: LocalDate?,
    required: Boolean,
    onDateChange: (LocalDate?) -> Unit,
    enabled: Boolean = LocalCreatorControlsEnabled.current,
) {
    var showPicker by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedButton(
                onClick = { showPicker = true },
                enabled = enabled,
                modifier = Modifier.weight(1f),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.onSurface),
            ) {
                Icon(
                    Icons.Default.CalendarMonth,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    text = date?.format(dateFormatter) ?: "Choose date",
                    modifier = Modifier.padding(start = 8.dp),
                )
            }
            if (!required && date != null) {
                IconButton(onClick = { onDateChange(null) }, enabled = enabled) {
                    Icon(Icons.Default.Close, contentDescription = "Clear $label")
                }
            }
        }
    }

    if (showPicker && enabled) {
        val initialMillis = date
            ?.atStartOfDay(ZoneOffset.UTC)
            ?.toInstant()
            ?.toEpochMilli()
        val pickerState = rememberDatePickerState(initialSelectedDateMillis = initialMillis)
        DatePickerDialog(
            onDismissRequest = { showPicker = false },
            confirmButton = {
                TextButton(
                    onClick = {
                        pickerState.selectedDateMillis?.let { millis ->
                            onDateChange(Instant.ofEpochMilli(millis).atZone(ZoneOffset.UTC).toLocalDate())
                        }
                        showPicker = false
                    },
                    enabled = pickerState.selectedDateMillis != null,
                ) {
                    Text("Use date")
                }
            },
            dismissButton = {
                TextButton(onClick = { showPicker = false }) {
                    Text("Cancel")
                }
            },
        ) {
            DatePicker(state = pickerState, colors = editorDatePickerColors())
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TimeField(
    label: String,
    time: LocalTime?,
    required: Boolean,
    enabled: Boolean,
    onTimeChange: (LocalTime?) -> Unit,
) {
    var showPicker by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedButton(
                onClick = { showPicker = true },
                enabled = enabled,
                modifier = Modifier.weight(1f),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.onSurface),
            ) {
                Icon(
                    Icons.Default.Schedule,
                    contentDescription = null,
                    tint = if (enabled) MaterialTheme.colorScheme.onSurfaceVariant else LocalContentColor.current,
                )
                Text(
                    text = time?.format(timeFormatter) ?: "Choose time",
                    modifier = Modifier.padding(start = 8.dp),
                )
            }
            if (!required && time != null) {
                IconButton(onClick = { onTimeChange(null) }, enabled = enabled) {
                    Icon(Icons.Default.Close, contentDescription = "Clear $label")
                }
            }
        }
    }

    if (showPicker && enabled) {
        val context = LocalContext.current
        val initial = time ?: LocalTime.NOON
        val pickerState = rememberTimePickerState(
            initialHour = initial.hour,
            initialMinute = initial.minute,
            is24Hour = DateFormat.is24HourFormat(context),
        )
        AlertDialog(
            onDismissRequest = { showPicker = false },
            title = { Text("Choose time") },
            text = { TimePicker(state = pickerState, colors = editorTimePickerColors()) },
            confirmButton = {
                TextButton(
                    onClick = {
                        onTimeChange(LocalTime.of(pickerState.hour, pickerState.minute))
                        showPicker = false
                    },
                ) {
                    Text("Use time")
                }
            },
            dismissButton = {
                TextButton(onClick = { showPicker = false }) {
                    Text("Cancel")
                }
            },
        )
    }
}

@Composable
private fun DurationStepper(
    label: String,
    duration: Duration?,
    stepMinutes: Int,
    onStep: (Int) -> Unit,
    validationMessage: String? = null,
    compact: Boolean = false,
    enabled: Boolean = LocalCreatorControlsEnabled.current,
) {
    val value = duration?.let(::formatDuration) ?: "No fixed duration"
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        if (!compact) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = if (compact) "$label: $value" else value,
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.weight(1f),
            )
            IconButton(
                onClick = { onStep(-1) },
                enabled = enabled && duration != null,
            ) {
                Icon(
                    Icons.Default.Remove,
                    contentDescription = "Decrease $label by $stepMinutes minutes",
                )
            }
            IconButton(onClick = { onStep(1) }, enabled = enabled) {
                Icon(
                    Icons.Default.Add,
                    contentDescription = "Increase $label by $stepMinutes minutes",
                )
            }
        }
        validationMessage?.let { message ->
            Text(
                text = message,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}

@Composable
private fun SettingSwitch(
    label: String,
    supportingText: String? = null,
    checked: Boolean,
    enabled: Boolean = LocalCreatorControlsEnabled.current,
    onCheckedChange: (Boolean) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .sizeIn(minHeight = 56.dp)
            .toggleable(
                value = checked,
                enabled = enabled,
                role = Role.Switch,
                onValueChange = onCheckedChange,
            )
            .padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(text = label, style = MaterialTheme.typography.bodyLarge)
            supportingText?.let {
                Text(
                    text = it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Switch(
            checked = checked,
            onCheckedChange = null,
            enabled = enabled,
        )
    }
}

@Composable
private fun SectionTitle(text: String) {
    Spacer(modifier = Modifier.height(4.dp))
    Text(
        text = text,
        style = MaterialTheme.typography.titleMedium,
        color = MaterialTheme.colorScheme.onSurface,
    )
}

@Composable
private fun FieldError(show: Boolean, message: String?) {
    if (show && message != null) {
        Text(
            text = message,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.error,
            modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
        )
    }
}

internal fun formatDuration(duration: Duration): String {
    if (duration.nano != 0) return duration.toString()
    val hours = duration.seconds / 3_600
    val minutes = duration.seconds / 60 % 60
    val seconds = duration.seconds % 60
    return listOfNotNull(
        "${hours}h".takeIf { hours != 0L },
        "${minutes}m".takeIf { minutes != 0L },
        "${seconds}s".takeIf { seconds != 0L || duration.isZero },
    ).joinToString(" ")
}
