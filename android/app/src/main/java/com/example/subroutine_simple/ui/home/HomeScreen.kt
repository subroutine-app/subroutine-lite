package com.example.subroutine_simple.ui.home

import android.text.format.DateFormat
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height

import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CenterFocusStrong
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.ui.ActionsUiState
import com.example.subroutine_simple.ui.MainViewModel
import com.example.subroutine_simple.ui.SyncStatus
import com.example.subroutine_simple.ui.agenda.AgendaEntry
import com.example.subroutine_simple.ui.components.CompoundItemCard
import com.example.subroutine_simple.ui.components.InlineActionCreator
import com.example.subroutine_simple.ui.components.ItemCardMenuAction
import com.example.subroutine_simple.ui.components.PullToResyncBox
import com.example.subroutine_simple.ui.components.inlineActionRequest
import com.example.subroutine_simple.ui.components.rememberInlineActionCreatorState
import com.example.subroutine_simple.ui.creator.CreatorSchedule
import com.example.subroutine_simple.ui.focus.FocusEventMoment
import com.example.subroutine_simple.ui.focus.FocusMode
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import java.time.Instant
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter

object HomeTestTags {
    const val Root = "home"
    const val Clock = "home:clock"
    const val Focus = "home:focus"
    const val Status = "home:status"
    const val Creator = "home:creator"
    fun action(id: String) = "home:action:$id"
    fun event(id: String) = "home:event:$id"
}

@Composable
fun HomeScreen(
    viewModel: MainViewModel,
    contentPadding: PaddingValues,
    onEditAction: (String) -> Unit,
    onEditEvent: (String) -> Unit,
    onOpenFocus: (FocusMode) -> Unit,
    modifier: Modifier = Modifier,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val authentication by viewModel.authenticationState.collectAsStateWithLifecycle()
    val workspace by viewModel.workspaceDescriptor.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val completing by viewModel.completing.collectAsStateWithLifecycle()
    val resyncing by viewModel.isResyncing.collectAsStateWithLifecycle()
    val now by rememberHomeClock()
    val zoneId = ZoneId.systemDefault()
    val localNow = now.atZone(zoneId)
    val content = uiState.content
    val writable = content != null && homeWorkspaceWritable(authentication, workspace, content.isLocalOnly)
    val mutationEnabled = writable && !saving
    val projection = remember(content?.actions, content?.events, now, zoneId) {
        content?.let { projectHome(it.actions, it.events, now, zoneId) }
    }
    val focusMode =
        if (projection?.nextAction == null && projection?.event != null) FocusMode.Events else FocusMode.Actions

    fun canMutateNow(): Boolean {
        val current = viewModel.uiState.value.content ?: return false
        return !viewModel.saving.value && viewModel.workspaceDescriptor.value == workspace &&
                current.datasetId == content?.datasetId &&
                homeWorkspaceWritable(viewModel.authenticationState.value, workspace, current.isLocalOnly)
    }

    PullToResyncBox(
        isResyncing = resyncing,
        onResync = if (authentication is AuthenticationState.SignedIn) viewModel::resync else null,
        contentPadding = contentPadding,
        modifier = modifier.testTag(HomeTestTags.Root),
    ) {
        key(workspace, content?.datasetId) {
            val draft = rememberInlineActionCreatorState("home:${workspace?.workspaceId}:${content?.datasetId}")
            Column(
                modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Column(
                    modifier = Modifier.widthIn(max = 560.dp).fillMaxWidth()
                        .padding(start = 24.dp, end = 24.dp, top = 32.dp, bottom = 96.dp),
                    verticalArrangement = Arrangement.spacedBy(24.dp),
                ) {
                    HomeClock(localNow, onOpenFocus = if (content == null) null else ({ onOpenFocus(focusMode) }))
                    HomeStatus(
                        uiState = uiState,
                        canResolveConflict = mutationEnabled && authentication is AuthenticationState.SignedIn,
                        onUseRemote = viewModel::useRemoteConflict,
                        onKeepLocal = viewModel::keepLocalConflict,
                    )
                    if (content != null && projection != null) {
                        when {
                            draft.expanded || projection.isEmpty -> {
                                if (writable) {
                                    InlineActionCreator(
                                        state = draft,
                                        saving = saving,
                                        error = uiState.operationError,
                                        onCreate = { title, onSuccess ->
                                            if (canMutateNow()) {
                                                // Home has no selected date: its new action is queued, not scheduled.
                                                inlineActionRequest(title, localNow.toLocalDate())
                                                    ?.copy(start = CreatorSchedule(zoneId = zoneId))
                                                    ?.let {
                                                        viewModel.createItem(
                                                            it,
                                                            onSuccess,
                                                            creationId = draft.creationId
                                                        )
                                                    }
                                            }
                                        },
                                        modifier = Modifier.testTag(HomeTestTags.Creator),
                                    )
                                } else if (draft.expanded) {
                                    Surface(
                                        color = MaterialTheme.colorScheme.surfaceContainerLow,
                                        shape = MaterialTheme.shapes.medium
                                    ) {
                                        Column(
                                            Modifier.fillMaxWidth().padding(16.dp),
                                            verticalArrangement = Arrangement.spacedBy(8.dp)
                                        ) {
                                            Text(draft.title, style = MaterialTheme.typography.bodyLarge)
                                            HomeCaption("Editing paused. Your draft is preserved.")
                                            TextButton(
                                                onClick = draft::cancel,
                                                enabled = !saving
                                            ) { Text("Cancel new action") }
                                        }
                                    }
                                } else {
                                    HomeCaption("Creation is unavailable in this workspace.")
                                }
                            }

                            else -> {
                                projection.nextAction?.let { action ->
                                    key(action.id) {
                                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                            HomeCaption("Next action")
                                            CompoundItemCard(
                                                entry = AgendaEntry.ActionEntry(action),
                                                dateContext = localNow.toLocalDate(),
                                                isCompleting = action.id in completing,
                                                onEdit = if (mutationEnabled) ({ onEditAction(action.id) }) else null,
                                                onComplete = if (mutationEnabled && homeCompletionAvailable(
                                                        action,
                                                        content.isLocalOnly
                                                    )
                                                ) ({
                                                    if (canMutateNow() && action.id !in viewModel.completing.value) {
                                                        viewModel.completeAction(action.id)
                                                    }
                                                }) else null,
                                                zoneId = zoneId,
                                                modifier = Modifier.testTag(HomeTestTags.action(action.id)),
                                            )
                                            if (!homeCompletionAvailable(action, content.isLocalOnly)) {
                                                HomeCaption("Recurring actions cannot be completed in local-only mode.")
                                            }
                                        }
                                    }
                                }
                                projection.event?.let { moment ->
                                    key(moment.key) {
                                        HomeEventPreview(
                                            moment = moment,
                                            now = localNow,
                                            onEdit = if (mutationEnabled) ({ onEditEvent(moment.event.id) }) else null,
                                            onOpenFocus = { onOpenFocus(FocusMode.Events) },
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun HomeClock(now: ZonedDateTime, onOpenFocus: (() -> Unit)?) {
    val locale = LocalConfiguration.current.locales[0]
    val use24Hours = DateFormat.is24HourFormat(LocalContext.current)
    val timeFormat = remember(locale, use24Hours) {
        DateTimeFormatter.ofPattern(if (use24Hours) "HH:mm" else "h:mm a", locale)
    }
    val dateFormat = remember(locale) { DateTimeFormatter.ofPattern("EEEE, MMMM d", locale) }
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        Column(
            modifier = Modifier.weight(1f).testTag(HomeTestTags.Clock)
                .semantics(mergeDescendants = true) { heading() }) {
            Text(
                now.format(dateFormat),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            Text(now.format(timeFormat), style = MaterialTheme.typography.displaySmall)
        }
        if (onOpenFocus != null) {
            IconButton(onClick = onOpenFocus, modifier = Modifier.testTag(HomeTestTags.Focus)) {
                Icon(Icons.Outlined.CenterFocusStrong, contentDescription = "Open Focus")
            }
        }
    }
}

@Composable
private fun HomeEventPreview(
    moment: FocusEventMoment,
    now: ZonedDateTime,
    onEdit: (() -> Unit)?,
    onOpenFocus: () -> Unit,
) {
    val locale = LocalConfiguration.current.locales[0]
    val use24Hours = DateFormat.is24HourFormat(LocalContext.current)
    val timeFormat = remember(locale, use24Hours) {
        DateTimeFormatter.ofPattern(if (use24Hours) "HH:mm" else "h:mm a", locale)
    }
    Column(
        verticalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.testTag(HomeTestTags.event(moment.event.id))
    ) {
        val caption = when (moment) {
            is FocusEventMoment.InProgress -> "Now · Ends in ${homeDuration(moment.remaining)}"
            is FocusEventMoment.Upcoming -> {
                val startDate = moment.startsAt.toLocalDate()
                val date = when (startDate) {
                    now.toLocalDate() -> null
                    now.toLocalDate().plusDays(1) -> "Tomorrow"
                    else -> startDate.format(DateTimeFormatter.ofPattern("MMM d", locale))
                }
                listOfNotNull(
                    date,
                    moment.startsAt.format(timeFormat),
                    "In ${homeDuration(moment.until)}"
                ).joinToString(" · ")
            }
        }
        HomeCaption(caption)
        CompoundItemCard(
            entry = AgendaEntry.EventEntry(moment.event),
            dateContext = now.toLocalDate(),
            onEdit = onEdit,
            menuActions = listOf(ItemCardMenuAction("focus-events", "Open in Focus", onOpenFocus)),
            zoneId = now.zone,
        )
        if (moment is FocusEventMoment.InProgress) {
            LinearProgressIndicator(
                progress = { moment.progress },
                modifier = Modifier.fillMaxWidth().height(4.dp).semantics { contentDescription = "Event progress" },
                color = MaterialTheme.colorScheme.primary,
                trackColor = MaterialTheme.colorScheme.surfaceContainerHighest,
            )
        }
    }
}

@Composable
private fun HomeCaption(text: String) {
    Text(text, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

@Composable
private fun HomeStatus(
    uiState: ActionsUiState,
    canResolveConflict: Boolean,
    onUseRemote: () -> Unit,
    onKeepLocal: () -> Unit,
) {
    val content = uiState.content
    val fallback = when (uiState.syncStatus) {
        SyncStatus.InitialLoading -> if (content == null) "Loading…" else null
        SyncStatus.Refreshing -> "Refreshing…"
        SyncStatus.Offline -> if (content == null) "Workspace unavailable. Connect to load your items." else "Offline. Showing saved items."
        SyncStatus.AuthenticationRequired -> "Sign in again to synchronize."
        SyncStatus.Ready -> if (content == null) "Workspace unavailable." else null
    }
    val messages = listOfNotNull(uiState.attentionMessage, uiState.operationError, uiState.message ?: fallback)
        .filter { it.isNotBlank() }.distinct()
    if (messages.isEmpty()) return
    Column(
        verticalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.fillMaxWidth().testTag(HomeTestTags.Status)
            .semantics { liveRegion = LiveRegionMode.Polite },
    ) {
        if (content == null && uiState.syncStatus == SyncStatus.InitialLoading) CircularProgressIndicator()
        messages.forEach { message ->
            Text(
                message,
                style = MaterialTheme.typography.bodySmall,
                color = if (message == uiState.operationError || content?.blockedCount?.let { it > 0 } == true) {
                    MaterialTheme.colorScheme.error
                } else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (content?.blockedMutationId != null && content.blockedCount > 0) {
            Column(horizontalAlignment = Alignment.End, modifier = Modifier.fillMaxWidth()) {
                TextButton(onClick = onUseRemote, enabled = canResolveConflict) { Text("Use server version") }
                TextButton(onClick = onKeepLocal, enabled = canResolveConflict) { Text("Keep my changes") }
            }
        }
    }
}

@Composable
private fun rememberHomeClock(): State<Instant> {
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val now = remember { mutableStateOf(Instant.now()) }
    LaunchedEffect(lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (isActive) {
                now.value = Instant.now()
                delay(1_000)
            }
        }
    }
    return now
}
