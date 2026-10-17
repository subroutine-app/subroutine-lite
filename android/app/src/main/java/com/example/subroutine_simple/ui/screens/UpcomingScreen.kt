package com.example.subroutine_simple.ui.screens

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.ui.MainViewModel
import com.example.subroutine_simple.ui.SyncStatus
import com.example.subroutine_simple.ui.agenda.AgendaEntry
import com.example.subroutine_simple.ui.agenda.UpcomingAgenda
import com.example.subroutine_simple.ui.agenda.projectAgenda
import com.example.subroutine_simple.ui.components.ItemCardMenuAction
import com.example.subroutine_simple.ui.components.PullToResyncBox
import com.example.subroutine_simple.ui.components.SyncStatusBanner
import com.example.subroutine_simple.ui.components.inlineActionRequest
import com.example.subroutine_simple.ui.creator.InlineCreatorSource
import kotlinx.coroutines.delay
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId


@Composable
fun UpcomingScreen(
    viewModel: MainViewModel,
    contentPadding: PaddingValues,
    onEditAction: (actionId: String) -> Unit,
    onEditEvent: (eventId: String) -> Unit,
    onEditMarker: (markerId: String) -> Unit,
    onEditSignal: (signalId: String) -> Unit,
    onCreateOnDate: (LocalDate) -> Unit,
    modifier: Modifier = Modifier,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val authentication by viewModel.authenticationState.collectAsStateWithLifecycle()
    val workspace by viewModel.workspaceDescriptor.collectAsStateWithLifecycle()
    val completing by viewModel.completing.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val isResyncing by viewModel.isResyncing.collectAsStateWithLifecycle()
    val now by produceState(Instant.now()) {
        while (true) {
            delay(60_000)
            value = Instant.now()
        }
    }
    val zoneId = ZoneId.systemDefault()
    val today = now.atZone(zoneId).toLocalDate()
    val content = uiState.content

    PullToResyncBox(
        isResyncing = isResyncing,
        onResync = if (authentication is AuthenticationState.SignedIn) viewModel::resync else null,
        contentPadding = contentPadding,
        modifier = modifier,
    ) {
        if (content == null) {
            Box(
                Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
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

        val sections = remember(content.actions, content.events, content.markers, content.signals, today, zoneId) {
            projectAgenda(content.actions, content.events, content.markers, content.signals, today, zoneId)
        }
        val stateKey = "${workspace?.workspaceId}:${content.datasetId}"
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            key(stateKey) {
                UpcomingAgenda(
                    sections = sections,
                    today = today,
                    stateKey = stateKey,
                    onEdit = { entry ->
                        when (entry) {
                            is AgendaEntry.ActionEntry -> onEditAction(entry.action.id)
                            is AgendaEntry.EventEntry -> onEditEvent(entry.event.id)
                            is AgendaEntry.MarkerEntry -> onEditMarker(entry.marker.id)
                            is AgendaEntry.SignalEntry -> onEditSignal(entry.signal.id)
                        }
                    },
                    onCreateOnDate = onCreateOnDate,
                    onMoreOptions = { date, source ->
                        viewModel.openInlineCreator(
                            InlineCreatorSource(source.key, source.creationId),
                            source.title,
                            date,
                            zoneId
                        )
                    },
                    onCreateAction = { date, title, onSuccess, creationId ->
                        inlineActionRequest(title, date)?.let { request ->
                            viewModel.createItem(request, onSuccess, creationId = creationId)
                        }
                    },
                    saving = saving,
                    createError = uiState.operationError,
                    onCompleteAction = if (saving) null else viewModel::completeAction,
                    completing = completing,
                    menuActions = { entry ->
                        upcomingItemActions(entry, viewModel, enabled = !saving)
                    },
                    zoneId = zoneId,
                    status = uiState.attentionMessage?.let { message ->
                        {
                            SyncStatusBanner(
                                message = message,
                                conflict = content.blockedCount > 0,
                                onUseRemote = viewModel::useRemoteConflict,
                                onKeepLocal = viewModel::keepLocalConflict,
                            )
                        }
                    },
                    modifier = Modifier.widthIn(max = 840.dp),
                )
            }
        }
    }
}

private fun upcomingItemActions(
    entry: AgendaEntry,
    viewModel: MainViewModel,
    enabled: Boolean,
): List<ItemCardMenuAction> {
    if (!enabled || entry !is AgendaEntry.ActionEntry) return emptyList()
    val action = entry.action
    return buildList {
        if (action.completion != null) {
            add(ItemCardMenuAction("restore", "Restore to backlog") {
                viewModel.restoreAction(action.id) {}
            })
        } else {
            if (action.queued && action.start?.contains('T') == true) {
                add(ItemCardMenuAction("pin", if (action.pinned) "Unpin from this time" else "Pin to this time") {
                    viewModel.action(action.id)?.let { latest ->
                        viewModel.savePlacement(latest.copy(pinned = !action.pinned))
                    }
                })
            }
            if (action.queued) {
                add(ItemCardMenuAction("backlog", "Move to backlog") {
                    viewModel.action(action.id)?.let { latest ->
                        viewModel.savePlacement(latest.copy(queued = false, pinned = false, start = null))
                    }
                })
            }
        }
        add(ItemCardMenuAction("duplicate", "Duplicate action") { viewModel.duplicateAction(action.id) {} })
    }
}

private fun MainViewModel.savePlacement(action: Action) = saveAction(
    actionId = action.id,
    title = action.title,
    content = action.content,
    start = action.start,
    duration = action.duration,
    queued = action.queued,
    pinned = action.pinned,
    onSuccess = {},
)
