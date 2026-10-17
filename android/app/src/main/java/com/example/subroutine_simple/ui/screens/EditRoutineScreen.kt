package com.example.subroutine_simple.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.data.models.Routine
import com.example.subroutine_simple.data.models.RoutineStep
import com.example.subroutine_simple.ui.components.EditorSchedule
import com.example.subroutine_simple.ui.components.ItemActionsMenu
import com.example.subroutine_simple.ui.components.ItemIdentityEditor
import com.example.subroutine_simple.ui.components.ProgressiveEditorSection
import com.example.subroutine_simple.ui.components.ScheduleEditor
import com.example.subroutine_simple.ui.components.durationIso
import com.example.subroutine_simple.ui.components.durationMinutes
import com.example.subroutine_simple.ui.components.formatSchedulePoint
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId

@Composable
fun EditRoutineScreen(
    routine: Routine,
    saving: Boolean,
    onBack: () -> Unit,
    onSave: (
        title: String,
        content: String?,
        target: String?,
        steps: List<RoutineStep>,
    ) -> Unit,
    onDuplicate: () -> Unit,
    onDelete: () -> Unit,
    destructiveActionsEnabled: Boolean = true,
    modifier: Modifier = Modifier,
) {
    val initialSchedule = remember(routine.id, routine.target) {
        EditorSchedule.fromSchedulePoint(routine.target)
    }
    var title by rememberSaveable(routine.id) { mutableStateOf(routine.title) }
    var content by rememberSaveable(routine.id) { mutableStateOf(routine.content.orEmpty()) }
    var dateValue by rememberSaveable(routine.id) {
        mutableStateOf(initialSchedule.date?.toString())
    }
    var timeValue by rememberSaveable(routine.id) {
        mutableStateOf(initialSchedule.time?.toString())
    }
    var steps by remember(routine.id) {
        mutableStateOf(
            routine.steps.mapIndexed { index, step ->
                EditableRoutineStep(key = index.toLong(), title = step.title, duration = step.duration)
            },
        )
    }
    var nextStepKey by remember(routine.id) { mutableLongStateOf(routine.steps.size.toLong()) }
    var newStepTitle by rememberSaveable(routine.id) { mutableStateOf("") }
    var showDiscardDialog by rememberSaveable(routine.id) { mutableStateOf(false) }
    var showDeleteDialog by rememberSaveable(routine.id) { mutableStateOf(false) }

    val schedule = EditorSchedule(
        date = dateValue?.let { runCatching { LocalDate.parse(it) }.getOrNull() },
        time = timeValue?.let { runCatching { LocalTime.parse(it) }.getOrNull() },
        zoneId = ZoneId.systemDefault(),
    )
    val scheduleChanged = dateValue != initialSchedule.date?.toString() ||
        timeValue != initialSchedule.time?.toString()
    val target = if (scheduleChanged) schedule.toSchedulePoint() else routine.target
    val editedSteps = steps.map { step ->
        RoutineStep(title = step.title.trim(), duration = step.duration)
    }
    val dirty = title != routine.title ||
        content.ifBlank { null } != routine.content ||
        scheduleChanged ||
        editedSteps != routine.steps
    val valid = title.isNotBlank() && editedSteps.all { it.title.isNotBlank() }

    fun requestBack() {
        when {
            saving -> Unit
            dirty -> showDiscardDialog = true
            else -> onBack()
        }
    }

    fun save() {
        onSave(
            title.trim(),
            content.trim().ifEmpty { null },
            target,
            editedSteps,
        )
    }

    fun addStep() {
        val stepTitle = newStepTitle.trim()
        if (stepTitle.isEmpty()) return
        steps = steps + EditableRoutineStep(
            key = nextStepKey,
            title = stepTitle,
            duration = null,
        )
        nextStepKey += 1
        newStepTitle = ""
    }

    BackHandler(onBack = ::requestBack)
    val scrollBehavior = TopAppBarDefaults.pinnedScrollBehavior()

    Scaffold(
        modifier = modifier
            .fillMaxSize()
            .nestedScroll(scrollBehavior.nestedScrollConnection),
        topBar = {
            TopAppBar(
                title = { Text("Routine") },
                navigationIcon = {
                    IconButton(onClick = ::requestBack, enabled = !saving) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    if (saving) {
                        CircularProgressIndicator(modifier = Modifier.padding(12.dp))
                    } else if (dirty) {
                        TextButton(
                            onClick = ::save,
                            enabled = valid,
                            modifier = Modifier.testTag("edit-routine:save"),
                        ) { Text("Save") }
                    }
                    ItemActionsMenu(
                        itemKind = "routine",
                        enabled = destructiveActionsEnabled && !saving && !dirty,
                        onDuplicate = onDuplicate.takeIf { destructiveActionsEnabled },
                        onDelete = { showDeleteDialog = true }.takeIf { destructiveActionsEnabled },
                    )
                },
                scrollBehavior = scrollBehavior,
            )
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(innerPadding)
                .padding(horizontal = 24.dp, vertical = 16.dp)
                .imePadding()
                .testTag("edit-routine:${routine.id}"),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            ItemIdentityEditor(
                title = title,
                onTitleChange = { title = it },
                notes = content,
                onNotesChange = { content = it },
                enabled = !saving,
                titlePlaceholder = "Name this routine",
                modifier = Modifier.testTag("edit-routine:identity"),
            )

            ProgressiveEditorSection(
                title = "Target",
                summary = target?.let(::formatSchedulePoint) ?: "No target",
                enabled = !saving,
                modifier = Modifier.testTag("edit-routine:target"),
            ) {
                ScheduleEditor(
                    schedule = schedule,
                    durationMinutes = null,
                    onScheduleChange = { next ->
                        dateValue = next.date?.toString()
                        timeValue = next.time?.toString()
                    },
                    onDurationChange = {},
                    requireDateAndTime = false,
                    requireDuration = false,
                    showDuration = false,
                    showHeading = false,
                    enabled = !saving,
                )
            }

            HorizontalDivider()
            Text(
                text = "Steps",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurface,
            )
            if (steps.isEmpty()) {
                Text(
                    text = "No steps yet",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.testTag("edit-routine:steps:empty"),
                )
            }
            steps.forEachIndexed { index, step ->
                key(step.key) {
                    RoutineStepEditor(
                        step = step,
                        index = index,
                        lastIndex = steps.lastIndex,
                        enabled = !saving,
                        onChange = { updated ->
                            steps = steps.toMutableList().also { it[index] = updated }
                        },
                        onRemove = {
                            steps = steps.toMutableList().also { it.removeAt(index) }
                        },
                        onMoveUp = {
                            steps = steps.toMutableList().also {
                                val previous = it[index - 1]
                                it[index - 1] = it[index]
                                it[index] = previous
                            }
                        },
                        onMoveDown = {
                            steps = steps.toMutableList().also {
                                val next = it[index + 1]
                                it[index + 1] = it[index]
                                it[index] = next
                            }
                        },
                    )
                }
            }
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
                    enabled = !saving,
                    keyboardOptions = KeyboardOptions(
                        capitalization = KeyboardCapitalization.Sentences,
                        imeAction = ImeAction.Done,
                    ),
                    shape = MaterialTheme.shapes.large,
                    modifier = Modifier
                        .weight(1f)
                        .testTag("edit-routine:step:new"),
                )
                OutlinedButton(
                    onClick = ::addStep,
                    enabled = !saving && newStepTitle.isNotBlank(),
                    modifier = Modifier.testTag("edit-routine:step:add"),
                ) {
                    Icon(Icons.Default.Add, contentDescription = null)
                    Text("Add", modifier = Modifier.padding(start = 8.dp))
                }
            }

        }
    }

    if (showDiscardDialog) {
        AlertDialog(
            onDismissRequest = { showDiscardDialog = false },
            title = { Text("Discard changes?") },
            text = { Text("Your unsaved routine changes will be lost.") },
            confirmButton = {
                TextButton(onClick = onBack) { Text("Discard") }
            },
            dismissButton = {
                TextButton(onClick = { showDiscardDialog = false }) { Text("Keep editing") }
            },
        )
    }
    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("Delete routine?") },
            text = { Text("\"${routine.title}\" will be permanently removed.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteDialog = false
                        onDelete()
                    },
                    enabled = destructiveActionsEnabled && !saving,
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) {
                    Text("Delete")
                }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteDialog = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun RoutineStepEditor(
    step: EditableRoutineStep,
    index: Int,
    lastIndex: Int,
    enabled: Boolean,
    onChange: (EditableRoutineStep) -> Unit,
    onRemove: () -> Unit,
    onMoveUp: () -> Unit,
    onMoveDown: () -> Unit,
) {
    val minutes = durationMinutes(step.duration)
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .testTag("edit-routine:step:${step.key}"),
    ) {
        Column(
            modifier = Modifier.padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            OutlinedTextField(
                value = step.title,
                onValueChange = { onChange(step.copy(title = it)) },
                label = { Text("Step ${index + 1}") },
                singleLine = true,
                enabled = enabled,
                keyboardOptions = KeyboardOptions(
                    capitalization = KeyboardCapitalization.Sentences,
                    imeAction = ImeAction.Next,
                ),
                shape = MaterialTheme.shapes.large,
                modifier = Modifier.fillMaxWidth(),
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text("Duration", style = MaterialTheme.typography.labelLarge)
                    Text(
                        text = minutes?.let(::formatStepDuration) ?: "No fixed duration",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                IconButton(
                    onClick = {
                        val nextMinutes = requireNotNull(minutes) - 5L
                        onChange(step.copy(duration = durationIso(nextMinutes.takeIf { it > 0 })))
                    },
                    enabled = enabled && minutes != null,
                ) {
                    Icon(Icons.Default.Remove, contentDescription = "Decrease step duration by 5 minutes")
                }
                IconButton(
                    onClick = {
                        onChange(step.copy(duration = durationIso((minutes ?: 0L) + 5L)))
                    },
                    enabled = enabled,
                ) {
                    Icon(Icons.Default.Add, contentDescription = "Increase step duration by 5 minutes")
                }
                IconButton(
                    onClick = onMoveUp,
                    enabled = enabled && index > 0,
                ) {
                    Icon(Icons.Default.ArrowUpward, contentDescription = "Move step up")
                }
                IconButton(
                    onClick = onMoveDown,
                    enabled = enabled && index < lastIndex,
                ) {
                    Icon(Icons.Default.ArrowDownward, contentDescription = "Move step down")
                }
                IconButton(onClick = onRemove, enabled = enabled) {
                    Icon(
                        Icons.Default.Delete,
                        contentDescription = "Remove step",
                        tint = if (enabled) {
                            MaterialTheme.colorScheme.error
                        } else {
                            MaterialTheme.colorScheme.onSurface.copy(alpha = 0.38f)
                        },
                    )
                }
            }
        }
    }
}

private data class EditableRoutineStep(
    val key: Long,
    val title: String,
    val duration: String?,
)

private fun formatStepDuration(minutes: Long): String {
    val hours = minutes / 60
    val remainingMinutes = minutes % 60
    return when {
        hours == 0L -> "${remainingMinutes}m"
        remainingMinutes == 0L -> "${hours}h"
        else -> "${hours}h ${remainingMinutes}m"
    }
}
