package com.example.subroutine_simple.ui.screens

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Inbox
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.parseSchedulePoint
import com.example.subroutine_simple.ui.MainViewModel
import com.example.subroutine_simple.ui.SyncStatus
import com.example.subroutine_simple.ui.components.ActionListItem
import com.example.subroutine_simple.ui.components.ItemCardPosition
import com.example.subroutine_simple.ui.components.PullToResyncBox
import com.example.subroutine_simple.ui.components.SyncStatusBanner
import com.example.subroutine_simple.ui.theme.subroutineButtonColors

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun BacklogScreen(
    viewModel: MainViewModel,
    contentPadding: PaddingValues,
    onEditAction: (actionId: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val authentication by viewModel.authenticationState.collectAsStateWithLifecycle()
    val completing by viewModel.completing.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val isResyncing by viewModel.isResyncing.collectAsStateWithLifecycle()
    val listState = rememberLazyListState()
    var query by rememberSaveable { mutableStateOf("") }
    var selectionMode by rememberSaveable { mutableStateOf(false) }
    var selectedIds by remember { mutableStateOf(emptySet<String>()) }
    var showBulkDelete by rememberSaveable { mutableStateOf(false) }
    val content = uiState.content

    PullToResyncBox(
        isResyncing = isResyncing,
        onResync = if (authentication is AuthenticationState.SignedIn) viewModel::resync else null,
        contentPadding = contentPadding,
        modifier = modifier,
    ) {
        if (content == null) {
            Box(
                Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(24.dp),
                contentAlignment = Alignment.Center,
            ) {
                if (uiState.syncStatus == SyncStatus.InitialLoading) {
                    LoadingIndicator()
                } else {
                    Text(uiState.operationError ?: uiState.message ?: "Could not load items.")
                }
            }
            return@PullToResyncBox
        }

        val terms = query.trim().lowercase().split(Regex("\\s+")).filter(String::isNotBlank)
        val filtered = content.backlogged
            .asSequence()
            .filter { action ->
                val haystack = "${action.title}\n${action.content.orEmpty()}".lowercase()
                terms.all(haystack::contains)
            }
            .sortedWith(
                compareBy<Action> { it.start == null }
                    .thenBy { parseSchedulePoint(it.start) }
                    .thenBy { it.id },
            )
            .toList()
        LaunchedEffect(filtered.map { it.id }) {
            selectedIds = selectedIds.intersect(filtered.map { it.id }.toSet())
        }

        LazyColumn(
            state = listState,
            modifier = Modifier.fillMaxSize(),
        ) {
            uiState.attentionMessage?.let { message ->
                item(key = "sync-status", contentType = "status") {
                    SyncStatusBanner(
                        message = message,
                        conflict = content.blockedCount > 0,
                        onUseRemote = viewModel::useRemoteConflict,
                        onKeepLocal = viewModel::keepLocalConflict,
                    )
                }
            }
            item(key = "search", contentType = "search") {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    label = { Text("Search unqueued") },
                    leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    shape = MaterialTheme.shapes.large,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp, vertical = 12.dp)
                        .testTag("unqueued:search"),
                )
            }
            item(key = "selection-actions", contentType = "selection-actions") {
                if (!selectionMode) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp),
                        horizontalArrangement = Arrangement.End,
                    ) {
                        TextButton(
                            onClick = { selectionMode = true },
                            enabled = filtered.isNotEmpty(),
                            modifier = Modifier.testTag("unqueued:select"),
                        ) { Text("Select") }
                    }
                } else {
                    androidx.compose.foundation.layout.Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp, vertical = 8.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text("${selectedIds.size} selected")
                            Row {
                                TextButton(onClick = {
                                    selectedIds = filtered.map { it.id }.toSet()
                                }) { Text("Select all") }
                                TextButton(onClick = {
                                    selectedIds = emptySet()
                                    selectionMode = false
                                }) { Text("Done") }
                            }
                        }
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Button(
                                onClick = {
                                    viewModel.queueActionsAnyTime(selectedIds) {
                                        selectedIds = emptySet()
                                        selectionMode = false
                                    }
                                },
                                enabled = selectedIds.isNotEmpty() && !saving,
                                colors = subroutineButtonColors(),
                                modifier = Modifier.weight(1f),
                            ) { Text("Queue any time") }
                            TextButton(
                                onClick = {
                                    viewModel.duplicateActions(selectedIds) {
                                        selectedIds = emptySet()
                                        selectionMode = false
                                    }
                                },
                                enabled = selectedIds.isNotEmpty() && !saving,
                                modifier = Modifier.weight(1f),
                            ) { Text("Duplicate") }
                            TextButton(
                                onClick = { showBulkDelete = true },
                                enabled = selectedIds.isNotEmpty() && !saving,
                                modifier = Modifier.weight(1f),
                            ) { Text("Delete") }
                        }
                    }
                }
            }
            itemsIndexed(
                items = filtered,
                key = { _, action -> "action:${action.id}" },
                contentType = { _, _ -> "unqueued_action" },
            ) { index, action ->
                ActionListItem(
                    action = action,
                    position = ItemCardPosition.fromNeighbors(index > 0, index < filtered.lastIndex),
                    isCompleting = action.id in completing,
                    onComplete = { viewModel.completeAction(action.id) },
                    onClick = { onEditAction(action.id) },
                    selectionMode = selectionMode,
                    selected = action.id in selectedIds,
                    onToggleSelection = {
                        selectedIds = if (action.id in selectedIds) {
                            selectedIds - action.id
                        } else {
                            selectedIds + action.id
                        }
                    },
                    modifier = Modifier.padding(horizontal = 4.dp).testTag("unqueued:action:${action.id}"),
                )
            }
            if (filtered.isEmpty()) {
                item(key = "empty", contentType = "empty") {
                    Surface(
                        color = MaterialTheme.colorScheme.surfaceContainerLow,
                        shape = MaterialTheme.shapes.extraLarge,
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp, vertical = 24.dp),
                    ) {
                        Column(
                            modifier = Modifier.padding(horizontal = 24.dp, vertical = 28.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Icon(
                                imageVector = Icons.Default.Inbox,
                                contentDescription = null,
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.size(40.dp),
                            )
                            Text(
                                text = if (query.isBlank()) "Nothing unqueued" else "No matches",
                                style = MaterialTheme.typography.titleLarge,
                            )

                        }
                    }
                }
            }
        }
    }

    if (showBulkDelete) {
        AlertDialog(
            onDismissRequest = { showBulkDelete = false },
            title = { Text("Delete selected actions?") },
            text = {
                Text("${selectedIds.size} action${if (selectedIds.size == 1) "" else "s"} will be permanently removed.")
            },
            confirmButton = {
                TextButton(onClick = {
                    showBulkDelete = false
                    viewModel.deleteActions(selectedIds) {
                        selectedIds = emptySet()
                        selectionMode = false
                    }
                }) { Text("Delete", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { showBulkDelete = false }) { Text("Cancel") }
            },
        )
    }
}
