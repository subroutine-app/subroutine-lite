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
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.ui.MainViewModel
import com.example.subroutine_simple.ui.components.DateEditorField
import com.example.subroutine_simple.ui.components.ItemActionsMenu
import com.example.subroutine_simple.ui.components.ItemIdentityEditor
import com.example.subroutine_simple.ui.components.ProgressiveEditorSection
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

private val MarkerDateFormatter = DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)

@Composable
fun EditMarkerScreen(
    markerId: String,
    viewModel: MainViewModel,
    onBack: () -> Unit,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val marker = viewModel.marker(markerId)
    if (uiState.content != null && marker == null && !saving) {
        onBack()
        return
    }
    if (marker == null) return

    EditMarkerContent(
        marker = marker,
        saving = saving,
        onBack = onBack,
        onSave = { title, content, date, endDate ->
            viewModel.saveMarker(markerId, title, content, date, endDate, onBack)
        },
        onDuplicate = { viewModel.duplicateMarker(markerId, onBack) },
        onDelete = { viewModel.deleteMarker(markerId, onBack) },
    )
}

@Composable
private fun EditMarkerContent(
    marker: Marker,
    saving: Boolean,
    onBack: () -> Unit,
    onSave: (String, String?, String, String?) -> Unit,
    onDuplicate: () -> Unit,
    onDelete: () -> Unit,
) {
    val initialStart = runCatching { LocalDate.parse(marker.date) }.getOrNull() ?: return
    val initialEnd = marker.endDate?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
        ?: initialStart
    var title by rememberSaveable(marker.id) { mutableStateOf(marker.title) }
    var content by rememberSaveable(marker.id) { mutableStateOf(marker.content.orEmpty()) }
    var startValue by rememberSaveable(marker.id) { mutableStateOf(initialStart.toString()) }
    var endValue by rememberSaveable(marker.id) { mutableStateOf(initialEnd.toString()) }
    var showDiscardDialog by rememberSaveable { mutableStateOf(false) }
    var showDeleteDialog by rememberSaveable { mutableStateOf(false) }

    val start = runCatching { LocalDate.parse(startValue) }.getOrNull()
    val end = runCatching { LocalDate.parse(endValue) }.getOrNull()
    val valid = title.isNotBlank() && start != null && end != null && !end.isBefore(start)
    val storedEnd = end?.takeIf { it != start }?.toString()
    val dirty = title != marker.title ||
        content.ifBlank { null } != marker.content ||
        start?.toString() != marker.date ||
        storedEnd != marker.endDate

    fun requestBack() {
        if (dirty && !saving) showDiscardDialog = true else onBack()
    }

    fun save() {
        onSave(
            title.trim(),
            content.trim().ifEmpty { null },
            requireNotNull(start).toString(),
            storedEnd,
        )
    }
    BackHandler(onBack = ::requestBack)

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Marker") },
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
                        itemKind = "marker",
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
                titlePlaceholder = "What should this time mean?",
            )
            ProgressiveEditorSection(
                title = "When",
                summary = markerDateSummary(start, end),
                enabled = !saving,
            ) {
                DateEditorField(
                label = "Start date",
                date = start,
                required = true,
                enabled = !saving,
                onDateChange = { selected ->
                    if (selected != null) {
                        startValue = selected.toString()
                        if (end?.isBefore(selected) != false) endValue = selected.toString()
                    }
                },
            )
            DateEditorField(
                label = "End date (inclusive)",
                date = end,
                required = true,
                enabled = !saving,
                onDateChange = { selected -> if (selected != null) endValue = selected.toString() },
            )
                if (start != null && end != null && end.isBefore(start)) {
                    Text(
                        "End date cannot be before start date.",
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
    }

    if (showDiscardDialog) {
        AlertDialog(
            onDismissRequest = { showDiscardDialog = false },
            title = { Text("Discard changes?") },
            text = { Text("Your unsaved marker changes will be lost.") },
            confirmButton = { TextButton(onClick = onBack) { Text("Discard") } },
            dismissButton = {
                TextButton(onClick = { showDiscardDialog = false }) { Text("Keep editing") }
            },
        )
    }
    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("Delete marker?") },
            text = { Text("\"${marker.title}\" will be permanently removed.") },
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

private fun markerDateSummary(start: LocalDate?, end: LocalDate?): String = when {
    start == null -> "Choose dates"
    end == null || end == start -> start.format(MarkerDateFormatter)
    else -> "${start.format(MarkerDateFormatter)} – ${end.format(MarkerDateFormatter)}"
}
