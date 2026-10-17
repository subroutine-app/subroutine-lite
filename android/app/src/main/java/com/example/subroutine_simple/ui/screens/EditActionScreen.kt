package com.example.subroutine_simple.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.BookmarkAdd
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Restore
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.isCompleted
import com.example.subroutine_simple.ui.MainViewModel
import com.example.subroutine_simple.ui.components.EditorSchedule
import com.example.subroutine_simple.ui.components.ItemActionsMenu
import com.example.subroutine_simple.ui.components.ItemIdentityEditor
import com.example.subroutine_simple.ui.components.ProgressiveEditorSection
import com.example.subroutine_simple.ui.components.ScheduleEditor
import com.example.subroutine_simple.ui.components.durationIso
import com.example.subroutine_simple.ui.components.durationMinutes
import com.example.subroutine_simple.ui.components.formatDuration
import com.example.subroutine_simple.ui.components.formatSchedulePoint
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId

@Composable
fun EditActionScreen(
    actionId: String,
    viewModel: MainViewModel,
    onBack: () -> Unit,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val action = viewModel.action(actionId)

    if (uiState.content != null && action == null && !saving) {
        onBack()
        return
    }
    if (action == null) return

    EditActionContent(
        action = action,
        saving = saving,
        onBack = onBack,
        onSave = { title, content, start, duration, queued, pinned ->
            viewModel.saveAction(
                actionId = actionId,
                title = title,
                content = content,
                start = start,
                duration = duration,
                queued = queued,
                pinned = pinned,
                onSuccess = onBack,
            )
        },
        onRestore = { viewModel.restoreAction(actionId, onBack) },
        onSaveForReuse = { viewModel.saveActionForReuse(actionId) },
        onDuplicate = { viewModel.duplicateAction(actionId, onBack) },
        onDelete = { viewModel.deleteAction(actionId, onBack) },
    )
}

@Composable
private fun EditActionContent(
    action: Action,
    saving: Boolean,
    onBack: () -> Unit,
    onSave: (
        title: String,
        content: String?,
        start: String?,
        duration: String?,
        queued: Boolean,
        pinned: Boolean,
    ) -> Unit,
    onRestore: () -> Unit,
    onSaveForReuse: () -> Unit,
    onDuplicate: () -> Unit,
    onDelete: () -> Unit,
) {
    val initialSchedule = EditorSchedule.fromSchedulePoint(action.start)
    var title by rememberSaveable(action.id) { mutableStateOf(action.title) }
    var content by rememberSaveable(action.id) { mutableStateOf(action.content.orEmpty()) }
    var dateValue by rememberSaveable(action.id) { mutableStateOf(initialSchedule.date?.toString()) }
    var timeValue by rememberSaveable(action.id) { mutableStateOf(initialSchedule.time?.toString()) }
    val initialDurationMinutes = durationMinutes(action.duration)
    var durationValue by rememberSaveable(action.id) {
        mutableLongStateOf(initialDurationMinutes ?: -1L)
    }
    var placement by rememberSaveable(action.id) {
        mutableStateOf(
            when {
                !action.queued -> ActionPlacement.Unqueued
                action.start == null -> ActionPlacement.AnyTime
                else -> ActionPlacement.Scheduled
            },
        )
    }
    var pinned by rememberSaveable(action.id) { mutableStateOf(action.pinned) }
    var showDiscardDialog by rememberSaveable { mutableStateOf(false) }
    var showDeleteDialog by rememberSaveable { mutableStateOf(false) }

    val schedule = EditorSchedule(
        date = dateValue?.let { runCatching { LocalDate.parse(it) }.getOrNull() },
        time = timeValue?.let { runCatching { LocalTime.parse(it) }.getOrNull() },
        zoneId = ZoneId.systemDefault(),
    )
    val currentDuration = durationValue.takeIf { it >= 0 }
    val scheduleChanged = dateValue != initialSchedule.date?.toString() ||
        timeValue != initialSchedule.time?.toString()
    val durationChanged = currentDuration != initialDurationMinutes
    val editedStart = if (scheduleChanged) schedule.toSchedulePoint() else action.start
    val startValue = if (placement == ActionPlacement.AnyTime) null else editedStart
    val queued = placement != ActionPlacement.Unqueued
    val durationValueIso = if (durationChanged) durationIso(currentDuration) else action.duration
    val dirty = title != action.title ||
        content.ifBlank { null } != action.content ||
        startValue != action.start ||
        durationChanged ||
        queued != action.queued ||
        pinned != action.pinned
    val valid = title.isNotBlank() &&
        (placement != ActionPlacement.Scheduled || schedule.date != null)

    fun requestBack() {
        if (dirty && !saving) showDiscardDialog = true else onBack()
    }

    fun save() {
        onSave(
            title.trim(),
            content.trim().ifEmpty { null },
            startValue,
            durationValueIso,
            queued,
            pinned,
        )
    }

    BackHandler(onBack = ::requestBack)
    val scrollBehavior = TopAppBarDefaults.pinnedScrollBehavior()

    Scaffold(
        modifier = Modifier
            .fillMaxSize()
            .nestedScroll(scrollBehavior.nestedScrollConnection),
        topBar = {
            TopAppBar(
                title = { Text("Action") },
                navigationIcon = {
                    IconButton(onClick = ::requestBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    if (saving) {
                        CircularProgressIndicator(modifier = Modifier.padding(12.dp))
                    } else if (dirty) {
                        TextButton(onClick = ::save, enabled = valid) { Text("Save") }
                    }
                    ItemActionsMenu(
                        itemKind = "action",
                        enabled = !saving && !dirty,
                        onRestore = onRestore.takeIf { action.isCompleted },
                        onSaveForReuse = onSaveForReuse.takeIf { action.templateId == null },
                        onDuplicate = onDuplicate,
                        onDelete = { showDeleteDialog = true },
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
                .imePadding(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            ItemIdentityEditor(
                title = title,
                onTitleChange = { title = it },
                notes = content,
                onNotesChange = { content = it },
                enabled = !saving,
                titlePlaceholder = "What do you want to do?",
            )

            ProgressiveEditorSection(
                title = "Plan",
                summary = actionPlanSummary(placement, startValue, currentDuration, pinned),
                initiallyExpanded = false,
                enabled = !saving,
            ) {
                if (!action.isCompleted) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        ActionPlacement.entries.forEach { option ->
                            FilterChip(
                                selected = placement == option,
                                onClick = {
                                    placement = option
                                    if (option != ActionPlacement.Scheduled) pinned = false
                                },
                                label = { Text(option.label) },
                                enabled = !saving,
                                colors = FilterChipDefaults.filterChipColors(
                                    selectedContainerColor = MaterialTheme.colorScheme.primaryContainer,
                                    selectedLabelColor = MaterialTheme.colorScheme.onPrimaryContainer,
                                ),
                            )
                        }
                    }
                }

                if (placement != ActionPlacement.AnyTime) {
                    ScheduleEditor(
                        schedule = schedule,
                        durationMinutes = currentDuration,
                        onScheduleChange = { next ->
                            dateValue = next.date?.toString()
                            timeValue = next.time?.toString()
                            if (!next.hasTime()) pinned = false
                        },
                        onDurationChange = { durationValue = it ?: -1L },
                        requireDateAndTime = placement == ActionPlacement.Scheduled,
                        requireDuration = false,
                        showHeading = false,
                        enabled = !saving,
                    )
                } else {
                    Text(
                        text = "The scheduler can place this whenever there is room.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    ScheduleEditor(
                        schedule = EditorSchedule(),
                        durationMinutes = currentDuration,
                        onScheduleChange = {},
                        onDurationChange = { durationValue = it ?: -1L },
                        requireDateAndTime = false,
                        requireDuration = false,
                        showHeading = false,
                        showDateTime = false,
                        enabled = !saving,
                    )
                }

                if (!action.isCompleted && placement == ActionPlacement.Scheduled) {
                    Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .toggleable(
                            value = pinned,
                            enabled = !saving && schedule.hasTime(),
                            role = Role.Switch,
                            onValueChange = { pinned = it },
                        )
                        .padding(vertical = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text("Pin selected time", style = MaterialTheme.typography.bodyLarge)
                        Text(
                            "Pinned actions are not moved by the scheduler.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                        Switch(checked = pinned, onCheckedChange = null, enabled = schedule.hasTime())
                    }
                }
            }
        }
    }

    if (showDiscardDialog) {
        AlertDialog(
            onDismissRequest = { showDiscardDialog = false },
            title = { Text("Discard changes?") },
            text = { Text("Your unsaved action changes will be lost.") },
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
            title = { Text("Delete action?") },
            text = { Text("\"${action.title}\" will be permanently removed.") },
            confirmButton = {
                TextButton(onClick = {
                    showDeleteDialog = false
                    onDelete()
                }) { Text("Delete", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteDialog = false }) { Text("Cancel") }
            },
        )
    }
}

private fun actionPlanSummary(
    placement: ActionPlacement,
    start: String?,
    durationMinutes: Long?,
    pinned: Boolean,
): String {
    val placementSummary = when (placement) {
        ActionPlacement.Unqueued -> start?.let { "Due ${formatSchedulePoint(it)}" } ?: "Unqueued"
        ActionPlacement.AnyTime -> "Any time"
        ActionPlacement.Scheduled -> start?.let(::formatSchedulePoint) ?: "Choose a time"
    }
    val duration = durationMinutes?.let(::formatDuration)
    return listOfNotNull(placementSummary, duration, "pinned".takeIf { pinned }).joinToString(" · ")
}

private enum class ActionPlacement(val label: String) {
    Unqueued("Unqueued"),
    AnyTime("Any time"),
    Scheduled("Scheduled"),
}

private fun EditorSchedule.hasTime(): Boolean = date != null && time != null
