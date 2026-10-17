package com.example.subroutine_simple.ui.focus

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.ui.theme.subroutineButtonColors
import kotlinx.coroutines.launch
import java.time.Duration
import java.time.format.DateTimeFormatter
import java.util.Locale

object FocusTestTags {
    const val Root = "focus"
    const val ModeActions = "focus:mode:actions"
    const val ModeEvents = "focus:mode:events"
    const val ActionPager = "focus:actions:pager"
    const val ActionPrevious = "focus:actions:previous"
    const val ActionComplete = "focus:actions:complete"
    const val ActionNext = "focus:actions:next"
    const val ActionPosition = "focus:actions:position"
    const val ActionEmpty = "focus:state:actions-empty"
    const val EventGroup = "focus:events:group"
    const val EventEmpty = "focus:state:events-empty"
    const val SignalNotice = "focus:signal:notice"
    const val SignalDismiss = "focus:signal:dismiss"
    const val SignalOpen = "focus:signal:open"

    fun actionCard(key: String): String = "focus:actions:card:$key"
    fun actionEdit(key: String): String = "focus:actions:edit:$key"
    fun eventCard(key: String): String = "focus:events:card:$key"
    fun eventEdit(key: String): String = "focus:events:edit:$key"
}

@Composable
fun FocusScreen(
    projection: FocusProjection,
    mode: FocusMode,
    selectedActionKey: String?,
    onModeChange: (FocusMode) -> Unit,
    onSelectedActionChange: (String) -> Unit,
    onCompleteAction: (Action) -> Unit,
    onEditAction: (Action) -> Unit,
    onEditEvent: (Event) -> Unit,
    onDismissSignal: (String) -> Unit,
    onOpenSignal: (Signal) -> Unit,
    modifier: Modifier = Modifier,
    contentPadding: PaddingValues = PaddingValues(),
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .padding(contentPadding)
            .testTag(FocusTestTags.Root),
    ) {
        FocusModeChoice(
            mode = mode,
            onModeChange = onModeChange,
            modifier = Modifier
                .align(Alignment.CenterHorizontally)
                .padding(horizontal = 16.dp, vertical = 12.dp),
        )

        projection.signalNotice?.let { notice ->
            SignalNotice(
                notice = notice,
                onDismiss = { onDismissSignal(notice.key) },
                onOpen = { onOpenSignal(notice.signal) },
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 4.dp),
            )
        }

        when (mode) {
            FocusMode.Actions -> ActionFocusContent(
                actions = projection.actions,
                selectedActionKey = selectedActionKey,
                onSelectedActionChange = onSelectedActionChange,
                onCompleteAction = onCompleteAction,
                onEditAction = onEditAction,
                modifier = Modifier.weight(1f),
            )

            FocusMode.Events -> EventFocusContent(
                events = projection.events,
                horizon = projection.horizon,
                onEditEvent = onEditEvent,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun FocusModeChoice(
    mode: FocusMode,
    onModeChange: (FocusMode) -> Unit,
    modifier: Modifier = Modifier,
) {
    val modes = FocusMode.entries
    SingleChoiceSegmentedButtonRow(modifier = modifier) {
        modes.forEachIndexed { index, candidate ->
            SegmentedButton(
                selected = mode == candidate,
                onClick = { onModeChange(candidate) },
                shape = SegmentedButtonDefaults.itemShape(index = index, count = modes.size),
                colors = SegmentedButtonDefaults.colors(
                    activeContainerColor = MaterialTheme.colorScheme.primaryContainer,
                    activeContentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                ),
                modifier = Modifier.testTag(
                    when (candidate) {
                        FocusMode.Actions -> FocusTestTags.ModeActions
                        FocusMode.Events -> FocusTestTags.ModeEvents
                    },
                ),
            ) {
                Text(if (candidate == FocusMode.Actions) "Actions" else "Events")
            }
        }
    }
}

@Composable
private fun ActionFocusContent(
    actions: List<FocusAction>,
    selectedActionKey: String?,
    onSelectedActionChange: (String) -> Unit,
    onCompleteAction: (Action) -> Unit,
    onEditAction: (Action) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (actions.isEmpty()) {
        FocusEmptyState(
            title = "No actions to focus on",
            message = "No scheduled, queued actions in the focus window.",
            modifier = modifier.testTag(FocusTestTags.ActionEmpty),
        )
        return
    }

    val selectedIndex = actions.indexOfFirst { it.key == selectedActionKey }.takeIf { it >= 0 } ?: 0
    val pagerState = rememberPagerState(
        initialPage = selectedIndex,
        pageCount = { actions.size },
    )
    val coroutineScope = rememberCoroutineScope()
    val actionKeys = actions.map(FocusAction::key)

    LaunchedEffect(selectedActionKey, actionKeys) {
        val requestedPage = actions.indexOfFirst { it.key == selectedActionKey }
        val targetPage = if (requestedPage >= 0) requestedPage else pagerState.currentPage.coerceIn(actions.indices)
        if (targetPage != pagerState.currentPage) pagerState.scrollToPage(targetPage)
    }
    LaunchedEffect(pagerState, actionKeys) {
        snapshotFlow { pagerState.settledPage }.collect { page ->
            actions.getOrNull(page)?.key?.let(onSelectedActionChange)
        }
    }

    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.Center,
    ) {
        HorizontalPager(
            state = pagerState,
            key = { page -> actions[page].key },
            userScrollEnabled = actions.size > 1,
            modifier = Modifier
                .fillMaxWidth()
                .testTag(FocusTestTags.ActionPager),
        ) { page ->
            ActionCard(
                item = actions[page],
                onEdit = { onEditAction(actions[page].action) },
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 24.dp),
            )
        }

        Spacer(Modifier.height(16.dp))

        val currentPage = pagerState.currentPage.coerceIn(actions.indices)
        val currentAction = actions[currentPage].action
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedButton(
                onClick = { coroutineScope.launch { pagerState.animateScrollToPage(currentPage - 1) } },
                enabled = currentPage > 0,
                modifier = Modifier.testTag(FocusTestTags.ActionPrevious),
            ) {
                Text("Previous")
            }
            Button(
                onClick = { onCompleteAction(currentAction) },
                colors = subroutineButtonColors(),
                modifier = Modifier.testTag(FocusTestTags.ActionComplete),
            ) {
                Text("Complete")
            }
            OutlinedButton(
                onClick = { coroutineScope.launch { pagerState.animateScrollToPage(currentPage + 1) } },
                enabled = currentPage < actions.lastIndex,
                modifier = Modifier.testTag(FocusTestTags.ActionNext),
            ) {
                Text("Next")
            }
        }
        Text(
            text = "${currentPage + 1} of ${actions.size}",
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 8.dp)
                .testTag(FocusTestTags.ActionPosition),
        )
    }
}

@Composable
private fun ActionCard(
    item: FocusAction,
    onEdit: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Card(
        modifier = modifier.testTag(FocusTestTags.actionCard(item.key)),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
            contentColor = MaterialTheme.colorScheme.onSurface,
        ),
    ) {
        Column(
            modifier = Modifier.padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = item.action.title,
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
                maxLines = 3,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = item.schedule.displayLabel(),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            item.action.content?.takeIf { it.isNotBlank() }?.let { content ->
                Text(
                    text = content,
                    style = MaterialTheme.typography.bodyLarge,
                    maxLines = 5,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            TextButton(
                onClick = onEdit,
                modifier = Modifier
                    .align(Alignment.End)
                    .testTag(FocusTestTags.actionEdit(item.key)),
            ) {
                Text("Edit")
            }
        }
    }
}

@Composable
private fun EventFocusContent(
    events: List<FocusEventMoment>,
    horizon: Duration,
    onEditEvent: (Event) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (events.isEmpty()) {
        FocusEmptyState(
            title = "No upcoming events",
            message = "In the next ${formatHorizon(horizon)}",
            modifier = modifier.testTag(FocusTestTags.EventEmpty),
        )
        return
    }

    LazyColumn(
        modifier = modifier
            .fillMaxWidth()
            .testTag(FocusTestTags.EventGroup),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        item {
            Text(
                text = if (events.first() is FocusEventMoment.InProgress) "In progress" else "Up next",
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier
                    .widthIn(max = 760.dp)
                    .fillMaxWidth(),
            )
        }
        items(
            items = events,
            key = FocusEventMoment::key,
        ) { moment ->
            EventCard(
                moment = moment,
                onEdit = { onEditEvent(moment.event) },
                modifier = Modifier
                    .widthIn(max = 760.dp)
                    .fillMaxWidth(),
            )
        }
    }
}

@Composable
private fun EventCard(
    moment: FocusEventMoment,
    onEdit: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val cue = focusCue(event = moment)
    Card(
        modifier = modifier.testTag(FocusTestTags.eventCard(moment.key)),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
            contentColor = MaterialTheme.colorScheme.onSurface,
        ),
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.Top,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = moment.event.title,
                        style = MaterialTheme.typography.titleLarge,
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = eventScheduleLabel(moment),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                TextButton(
                    onClick = onEdit,
                    modifier = Modifier.testTag(FocusTestTags.eventEdit(moment.key)),
                ) {
                    Text("Edit")
                }
            }

            when (moment) {
                is FocusEventMoment.InProgress -> {
                    Text(
                        text = "Ends in ${formatFocusCountdown(moment.remaining)}",
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    LinearProgressIndicator(
                        progress = { moment.progress },
                        color = focusCueColor(cue),
                        trackColor = MaterialTheme.colorScheme.surfaceContainerHighest,
                        modifier = Modifier.fillMaxWidth(),
                    )
                }

                is FocusEventMoment.Upcoming -> Text(
                    text = "Starts in ${formatFocusCountdown(moment.until)}",
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.focusNoticeCue(cue),
                )
            }

            moment.event.content?.takeIf { it.isNotBlank() }?.let { content ->
                Text(
                    text = content,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 4,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun SignalNotice(
    notice: FocusSignalNotice,
    onDismiss: () -> Unit,
    onOpen: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Card(
        modifier = modifier.testTag(FocusTestTags.SignalNotice),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
            contentColor = MaterialTheme.colorScheme.onSurface,
        ),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = notice.signal.title,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = when (notice) {
                    is FocusSignalNotice.Recent -> "${formatFocusCountdown(notice.elapsed)} ago"
                    is FocusSignalNotice.Upcoming -> "In ${formatFocusCountdown(notice.until)}"
                },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.focusNoticeCue(focusCue(signal = notice)),
            )
            Row(
                modifier = Modifier.align(Alignment.End),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                TextButton(
                    onClick = onDismiss,
                    modifier = Modifier.testTag(FocusTestTags.SignalDismiss),
                ) {
                    Text("Dismiss")
                }
                Button(
                    onClick = onOpen,
                    colors = subroutineButtonColors(),
                    modifier = Modifier.testTag(FocusTestTags.SignalOpen),
                ) {
                    Text("Open")
                }
            }
        }
    }
}

@Composable
private fun FocusEmptyState(
    title: String,
    message: String,
    modifier: Modifier = Modifier,
) {
    Box(modifier = modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
        Column(
            modifier = Modifier
                .widthIn(max = 480.dp)
                .padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
                textAlign = TextAlign.Center,
            )
            Text(
                text = message,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        }
    }
}

private val FocusDate = DateTimeFormatter.ofPattern("EEE, MMM d", Locale.getDefault())
private val FocusTime = DateTimeFormatter.ofPattern("h:mm a", Locale.getDefault())

private fun FocusActionSchedule.displayLabel(): String = when (this) {
    is FocusActionSchedule.DateOnly -> date.format(FocusDate)
    is FocusActionSchedule.Timed -> "${local.toLocalDate().format(FocusDate)} · ${
        local.toLocalTime().format(FocusTime)
    }"
}

private fun eventScheduleLabel(moment: FocusEventMoment): String {
    val start = moment.startsAt
    val end = moment.endsAt
    return if (start.toLocalDate() == end.toLocalDate()) {
        "${start.toLocalDate().format(FocusDate)} · ${start.toLocalTime().format(FocusTime)}–${
            end.toLocalTime().format(FocusTime)
        }"
    } else {
        "${start.toLocalDate().format(FocusDate)} ${start.toLocalTime().format(FocusTime)} – " +
                "${end.toLocalDate().format(FocusDate)} ${end.toLocalTime().format(FocusTime)}"
    }
}

private fun formatHorizon(horizon: Duration): String {
    val hours = horizon.toHours()
    return if (hours >= 24 && hours % 24 == 0L) {
        val days = hours / 24
        "$days ${if (days == 1L) "day" else "days"}"
    } else {
        "$hours ${if (hours == 1L) "hour" else "hours"}"
    }
}
