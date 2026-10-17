package com.example.subroutine_simple.ui.screens

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.ui.ActionsContent
import com.example.subroutine_simple.ui.MainViewModel
import com.example.subroutine_simple.ui.SyncStatus
import com.example.subroutine_simple.ui.agenda.AgendaEntry
import com.example.subroutine_simple.ui.components.CompoundItemCard
import com.example.subroutine_simple.ui.components.ItemCardMenuAction
import com.example.subroutine_simple.ui.components.PullToResyncBox
import com.example.subroutine_simple.ui.components.SyncStatusBanner
import com.example.subroutine_simple.ui.components.durationMinutes
import com.example.subroutine_simple.ui.components.formatDuration
import com.example.subroutine_simple.ui.search.BrowseFilter
import com.example.subroutine_simple.ui.search.SearchCorpus
import com.example.subroutine_simple.ui.search.SearchProjection
import com.example.subroutine_simple.ui.search.SearchResource
import com.example.subroutine_simple.ui.search.SearchResult
import com.example.subroutine_simple.ui.search.projectBrowse

@Composable
fun BrowseScreen(
    viewModel: MainViewModel,
    onEditItem: (SearchResource) -> Unit,
    contentPadding: PaddingValues,
    modifier: Modifier = Modifier,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val authentication by viewModel.authenticationState.collectAsStateWithLifecycle()
    val workspace by viewModel.workspaceDescriptor.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val completing by viewModel.completing.collectAsStateWithLifecycle()
    val isResyncing by viewModel.isResyncing.collectAsStateWithLifecycle()
    val signedIn = authentication is AuthenticationState.SignedIn
    val content = uiState.content

    PullToResyncBox(
        isResyncing = isResyncing,
        onResync = if (signedIn) viewModel::resync else null,
        contentPadding = contentPadding,
        modifier = modifier.testTag("browse"),
    ) {
        if (content == null) {
            Box(Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
                if (uiState.syncStatus == SyncStatus.InitialLoading) {
                    LoadingIndicator()
                } else {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(uiState.operationError ?: uiState.message ?: "Could not load items.")
                        TextButton(onClick = viewModel::loadActions) { Text("Retry") }
                    }
                }
            }
            return@PullToResyncBox
        }

        key(workspace?.workspaceId, content.datasetId) {
            Column(Modifier.widthIn(max = 840.dp).fillMaxSize().align(Alignment.TopCenter)) {
                uiState.attentionMessage?.let { message ->
                    SyncStatusBanner(
                        message = message,
                        conflict = content.blockedCount > 0,
                        onUseRemote = viewModel::useRemoteConflict,
                        onKeepLocal = viewModel::keepLocalConflict,
                        actionsEnabled = signedIn && !saving,
                    )
                }
                BrowseCollection(
                    content = content,
                    viewModel = viewModel,
                    saving = saving,
                    completing = completing,
                    routineStartUnavailableReason = when {
                        content.isLocalOnly -> "Starting routines requires a connected account."
                        !signedIn || uiState.syncStatus !in setOf(SyncStatus.Ready, SyncStatus.Refreshing) ->
                            "Connect to start routines."

                        content.pendingCount > 0 || content.blockedCount > 0 ->
                            "Sync changes before starting routines."

                        else -> null
                    },
                    onEditItem = onEditItem,
                )
            }
        }
    }
}

@Composable
private fun BrowseCollection(
    content: ActionsContent,
    viewModel: MainViewModel,
    saving: Boolean,
    completing: Set<String>,
    routineStartUnavailableReason: String?,
    onEditItem: (SearchResource) -> Unit,
) {
    var query by rememberSaveable { mutableStateOf("") }
    var filter by rememberSaveable { mutableStateOf(BrowseFilter.All) }
    var deletingActionId by rememberSaveable { mutableStateOf<String?>(null) }
    val keyboard = LocalSoftwareKeyboardController.current
    val listState = rememberLazyListState()
    val corpus = remember(content) {
        SearchCorpus(
            actions = content.actions,
            routines = content.routines,
            actionTemplates = content.actionTemplates,
            eventTemplates = content.eventTemplates,
        )
    }
    val projection = remember(corpus, query, filter) { projectBrowse(corpus, query, filter) }

    OutlinedTextField(
        value = query,
        onValueChange = { query = it },
        label = { Text("Search collection") },
        leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
        trailingIcon = if (query.isNotEmpty()) {
            {
                IconButton(onClick = { query = "" }, modifier = Modifier.testTag("browse:clear")) {
                    Icon(Icons.Default.Clear, contentDescription = "Clear search")
                }
            }
        } else null,
        singleLine = true,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        keyboardActions = KeyboardActions(onSearch = { keyboard?.hide() }),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)
            .testTag("browse:search"),
    )
    Row(
        modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())
            .padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        BrowseFilter.entries.forEach { choice ->
            FilterChip(
                selected = choice == filter,
                onClick = { filter = choice },
                label = { Text(choice.label) },
                modifier = Modifier.testTag("browse:filter:${choice.key}"),
            )
        }
    }

    LazyColumn(
        state = listState,
        contentPadding = PaddingValues(top = 8.dp, bottom = 96.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.fillMaxSize().testTag("browse:results"),
    ) {
        when (projection) {
            is SearchProjection.Results -> {
                if (routineStartUnavailableReason != null &&
                    projection.items.any { it.resource is SearchResource.LiveRoutine }
                ) {
                    item(key = "routine-availability", contentType = "status") {
                        Text(
                            text = routineStartUnavailableReason,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                        )
                    }
                }
                items(projection.items, key = SearchResult::key, contentType = { it.resource.kind }) { result ->
                    BrowseResult(
                        result = result,
                        viewModel = viewModel,
                        saving = saving,
                        completing = completing,
                        canStartRoutine = routineStartUnavailableReason == null,
                        onEdit = { onEditItem(result.resource) },
                        onDeleteAction = { deletingActionId = it },
                    )
                }
            }

            SearchProjection.Empty, SearchProjection.Prompt, SearchProjection.NoMatches -> {
                item(key = "empty", contentType = "empty") {
                    Text(
                        text = if (query.isNotBlank()) "No matches" else when (filter) {
                            BrowseFilter.All -> "Nothing to browse"
                            BrowseFilter.Unscheduled -> "Nothing unscheduled"
                            BrowseFilter.Unqueued -> "Nothing unqueued"
                            BrowseFilter.Routines -> "No routines yet"
                            BrowseFilter.Saved -> "No saved items yet"
                        },
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(24.dp).testTag("browse:empty"),
                    )
                }
            }
        }
    }

    content.actions.firstOrNull { it.id == deletingActionId }?.let { action ->
        AlertDialog(
            onDismissRequest = { if (!saving) deletingActionId = null },
            title = { Text("Delete action?") },
            text = { Text("\"${action.title}\" will be permanently removed.") },
            confirmButton = {
                TextButton(
                    enabled = !saving,
                    onClick = {
                        deletingActionId = null
                        viewModel.deleteAction(action.id) {}
                    },
                    modifier = Modifier.testTag("browse:delete:confirm"),
                ) { Text("Delete", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(enabled = !saving, onClick = { deletingActionId = null }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun BrowseResult(
    result: SearchResult,
    viewModel: MainViewModel,
    saving: Boolean,
    completing: Set<String>,
    canStartRoutine: Boolean,
    onEdit: () -> Unit,
    onDeleteAction: (String) -> Unit,
) {
    val modifier = Modifier.testTag("browse:result:${result.key}")
    when (val resource = result.resource) {
        is SearchResource.LiveAction -> CompoundItemCard(
            entry = AgendaEntry.ActionEntry(resource.action),
            isCompleting = resource.id in completing,
            onComplete = if (saving) null else {
                { viewModel.completeAction(resource.id) }
            },
            onEdit = onEdit,
            menuActions = if (saving) emptyList() else buildList {
                if (!resource.action.queued) {
                    add(ItemCardMenuAction("queue", "Queue any time") {
                        viewModel.queueActionsAnyTime(setOf(resource.id)) {}
                    })
                }
                if (resource.action.templateId == null) {
                    add(ItemCardMenuAction("save", "Save for reuse") { viewModel.saveActionForReuse(resource.id) })
                }
                add(ItemCardMenuAction("duplicate", "Duplicate") { viewModel.duplicateAction(resource.id) {} })
                add(ItemCardMenuAction("delete", "Delete") { onDeleteAction(resource.id) })
            },
            modifier = modifier.padding(horizontal = 16.dp),
        )

        is SearchResource.LiveRoutine -> RoutineLibraryRow(
            routine = resource.routine,
            onEdit = onEdit,
            onStart = if (canStartRoutine) {
                { viewModel.instantiateRoutine(resource.id) }
            } else null,
            startEnabled = !saving && resource.routine.steps.isNotEmpty(),
            modifier = modifier,
        )

        is SearchResource.SavedAction -> TemplateLibraryRow(
            title = resource.template.title,
            content = resource.template.content,
            metadata = listOfNotNull(
                "Saved action",
                resource.template.naiveTime?.let { "At ${it.take(5)}" },
                durationMinutes(resource.template.duration)?.let(::formatDuration),
            ).joinToString(" · "),
            onUse = { viewModel.useActionTemplate(resource.id) },
            onEdit = onEdit,
            modifier = modifier,
        )

        is SearchResource.SavedEvent -> TemplateLibraryRow(
            title = resource.template.title,
            content = resource.template.content,
            metadata = listOfNotNull(
                "Saved event", durationMinutes(resource.template.duration)?.let(::formatDuration),
            ).joinToString(" · "),
            onUse = { viewModel.useEventTemplate(resource.id) },
            onEdit = onEdit,
            modifier = modifier,
        )

        is SearchResource.LiveEvent -> CompoundItemCard(
            entry = AgendaEntry.EventEntry(resource.event), onEdit = onEdit,
            modifier = modifier.padding(horizontal = 16.dp),
        )

        is SearchResource.LiveMarker -> CompoundItemCard(
            entry = AgendaEntry.MarkerEntry(resource.marker), onEdit = onEdit,
            modifier = modifier.padding(horizontal = 16.dp),
        )

        is SearchResource.LiveSignal -> CompoundItemCard(
            entry = AgendaEntry.SignalEntry(resource.signal), onEdit = onEdit,
            modifier = modifier.padding(horizontal = 16.dp),
        )
    }
}
