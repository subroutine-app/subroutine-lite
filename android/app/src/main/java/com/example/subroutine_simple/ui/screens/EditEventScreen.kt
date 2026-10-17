package com.example.subroutine_simple.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.BookmarkAdd
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
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
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.example.subroutine_simple.data.models.Event
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
fun EditEventScreen(
    eventId: String,
    viewModel: MainViewModel,
    onBack: () -> Unit,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val event = viewModel.event(eventId)

    if (uiState.content != null && event == null && !saving) {
        onBack()
        return
    }
    if (event == null) return

    EditEventContent(
        event = event,
        saving = saving,
        onBack = onBack,
        onSave = { title, content, start, duration ->
            viewModel.saveEvent(eventId, title, content, start, duration, onBack)
        },
        onSaveForReuse = { viewModel.saveEventForReuse(eventId) },
        onDuplicate = { viewModel.duplicateEvent(eventId, onBack) },
        onDelete = { viewModel.deleteEvent(eventId, onBack) },
    )
}

@Composable
private fun EditEventContent(
    event: Event,
    saving: Boolean,
    onBack: () -> Unit,
    onSave: (title: String, content: String?, start: String, duration: String) -> Unit,
    onSaveForReuse: () -> Unit,
    onDuplicate: () -> Unit,
    onDelete: () -> Unit,
) {
    val initialSchedule = EditorSchedule.fromSchedulePoint(event.start)
    var title by rememberSaveable(event.id) { mutableStateOf(event.title) }
    var content by rememberSaveable(event.id) { mutableStateOf(event.content.orEmpty()) }
    var dateValue by rememberSaveable(event.id) { mutableStateOf(initialSchedule.date?.toString()) }
    var timeValue by rememberSaveable(event.id) { mutableStateOf(initialSchedule.time?.toString()) }
    val initialDurationMinutes = durationMinutes(event.duration) ?: 60L
    var durationValue by rememberSaveable(event.id) {
        mutableLongStateOf(initialDurationMinutes)
    }
    var showDiscardDialog by rememberSaveable { mutableStateOf(false) }
    var showDeleteDialog by rememberSaveable { mutableStateOf(false) }

    val schedule = EditorSchedule(
        date = dateValue?.let { runCatching { LocalDate.parse(it) }.getOrNull() },
        time = timeValue?.let { runCatching { LocalTime.parse(it) }.getOrNull() },
        zoneId = ZoneId.systemDefault(),
    )
    val scheduleChanged = dateValue != initialSchedule.date?.toString() ||
        timeValue != initialSchedule.time?.toString()
    val durationChanged = durationValue != initialDurationMinutes
    val startValue = if (scheduleChanged) schedule.toSchedulePoint() else event.start
    val durationValueIso = if (durationChanged) durationIso(durationValue) else event.duration
    val isValid = title.isNotBlank() && schedule.hasTime() && durationValue > 0L
    val dirty = title != event.title ||
        content.ifBlank { null } != event.content ||
        scheduleChanged ||
        durationChanged

    fun requestBack() {
        if (dirty && !saving) showDiscardDialog = true else onBack()
    }

    fun save() {
        onSave(
            title.trim(),
            content.trim().ifEmpty { null },
            requireNotNull(startValue),
            requireNotNull(durationValueIso),
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
                title = { Text("Event") },
                navigationIcon = {
                    IconButton(onClick = ::requestBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    if (saving) {
                        CircularProgressIndicator(modifier = Modifier.padding(12.dp))
                    } else if (dirty) {
                        TextButton(onClick = ::save, enabled = isValid) { Text("Save") }
                    }
                    ItemActionsMenu(
                        itemKind = "event",
                        enabled = !saving && !dirty,
                        onSaveForReuse = onSaveForReuse.takeIf { event.templateId == null },
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
                titlePlaceholder = "What is happening?",
            )
            ProgressiveEditorSection(
                title = "When",
                summary = "${formatSchedulePoint(startValue ?: event.start)} · ${formatDuration(durationValue)}",
                enabled = !saving,
            ) {
                ScheduleEditor(
                    schedule = schedule,
                    durationMinutes = durationValue,
                    onScheduleChange = { next ->
                        dateValue = next.date?.toString()
                        timeValue = next.time?.toString()
                    },
                    onDurationChange = { durationValue = it ?: 15L },
                    requireDateAndTime = true,
                    requireDuration = true,
                    showHeading = false,
                    enabled = !saving,
                )
            }
        }
    }

    if (showDiscardDialog) {
        AlertDialog(
            onDismissRequest = { showDiscardDialog = false },
            title = { Text("Discard changes?") },
            text = { Text("Your unsaved event changes will be lost.") },
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
            title = { Text("Delete event?") },
            text = { Text("\"${event.title}\" will be permanently removed.") },
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

private fun EditorSchedule.hasTime(): Boolean = date != null && time != null
