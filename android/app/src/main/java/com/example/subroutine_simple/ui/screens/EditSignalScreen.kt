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
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.ui.MainViewModel
import com.example.subroutine_simple.ui.components.EditorSchedule
import com.example.subroutine_simple.ui.components.ItemActionsMenu
import com.example.subroutine_simple.ui.components.ItemIdentityEditor
import com.example.subroutine_simple.ui.components.ProgressiveEditorSection
import com.example.subroutine_simple.ui.components.ScheduleEditor
import com.example.subroutine_simple.ui.components.formatSchedulePoint
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId

@Composable
fun EditSignalScreen(
    signalId: String,
    viewModel: MainViewModel,
    onBack: () -> Unit,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val signal = viewModel.signal(signalId)
    if (uiState.content != null && signal == null && !saving) {
        onBack()
        return
    }
    if (signal == null) return

    EditSignalContent(
        signal = signal,
        saving = saving,
        onBack = onBack,
        onSave = { title, content, datetime ->
            viewModel.saveSignal(signalId, title, content, datetime, onBack)
        },
        onDuplicate = { viewModel.duplicateSignal(signalId, onBack) },
        onDelete = { viewModel.deleteSignal(signalId, onBack) },
    )
}

@Composable
private fun EditSignalContent(
    signal: Signal,
    saving: Boolean,
    onBack: () -> Unit,
    onSave: (String, String?, String) -> Unit,
    onDuplicate: () -> Unit,
    onDelete: () -> Unit,
) {
    val initialSchedule = EditorSchedule.fromSchedulePoint(signal.datetime)
    var title by rememberSaveable(signal.id) { mutableStateOf(signal.title) }
    var content by rememberSaveable(signal.id) { mutableStateOf(signal.content.orEmpty()) }
    var dateValue by rememberSaveable(signal.id) { mutableStateOf(initialSchedule.date?.toString()) }
    var timeValue by rememberSaveable(signal.id) { mutableStateOf(initialSchedule.time?.toString()) }
    var showDiscardDialog by rememberSaveable { mutableStateOf(false) }
    var showDeleteDialog by rememberSaveable { mutableStateOf(false) }

    val schedule = EditorSchedule(
        date = dateValue?.let { runCatching { LocalDate.parse(it) }.getOrNull() },
        time = timeValue?.let { runCatching { LocalTime.parse(it) }.getOrNull() },
        zoneId = ZoneId.systemDefault(),
    )
    val scheduleChanged = dateValue != initialSchedule.date?.toString() ||
        timeValue != initialSchedule.time?.toString()
    val datetime = if (scheduleChanged) schedule.toSchedulePoint() else signal.datetime
    val valid = title.isNotBlank() && schedule.date != null && schedule.time != null
    val dirty = title != signal.title ||
        content.ifBlank { null } != signal.content ||
        scheduleChanged

    fun requestBack() {
        if (dirty && !saving) showDiscardDialog = true else onBack()
    }

    fun save() {
        onSave(
            title.trim(),
            content.trim().ifEmpty { null },
            requireNotNull(datetime),
        )
    }
    BackHandler(onBack = ::requestBack)

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Signal") },
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
                        itemKind = "signal",
                        enabled = !saving && !dirty,
                        onDuplicate = onDuplicate,
                        onDelete = { showDeleteDialog = true },
                    )
                },
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
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            ItemIdentityEditor(
                title = title,
                onTitleChange = { title = it },
                notes = content,
                onNotesChange = { content = it },
                enabled = !saving,
                titlePlaceholder = "What should you notice?",
            )
            ProgressiveEditorSection(
                title = "Fire at",
                summary = datetime?.let(::formatSchedulePoint) ?: "Choose when",
                enabled = !saving,
            ) {
                ScheduleEditor(
                    schedule = schedule,
                    durationMinutes = null,
                    onScheduleChange = { next ->
                        dateValue = next.date?.toString()
                        timeValue = next.time?.toString()
                    },
                    onDurationChange = {},
                    requireDateAndTime = true,
                    requireDuration = false,
                    showDuration = false,
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
            text = { Text("Your unsaved signal changes will be lost.") },
            confirmButton = { TextButton(onClick = onBack) { Text("Discard") } },
            dismissButton = {
                TextButton(onClick = { showDiscardDialog = false }) { Text("Keep editing") }
            },
        )
    }
    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("Delete signal?") },
            text = { Text("\"${signal.title}\" will be permanently removed.") },
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
