package com.example.subroutine_simple.ui.agenda

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons


import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Flag
import androidx.compose.material.icons.filled.UnfoldLess
import androidx.compose.material.icons.filled.UnfoldMore

import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.launch
import androidx.compose.runtime.saveable.mapSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription

import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.example.subroutine_simple.ui.components.InlineActionCreator
import com.example.subroutine_simple.ui.components.InlineActionCreatorState
import com.example.subroutine_simple.ui.components.ItemCardMenuAction
import com.example.subroutine_simple.ui.components.rememberInlineActionCreatorState
import com.example.subroutine_simple.ui.components.rememberItemCardNotesState
import com.example.subroutine_simple.ui.components.compoundItemCards
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

private val ExpansionSaver = mapSaver<Map<String, Boolean>>(
    save = { it },
    restore = { values -> values.mapValues { it.value as Boolean } },
)

internal fun AgendaSection.expandedIn(choices: Map<String, Boolean>, today: LocalDate): Boolean =
    choices[key] ?: (date == today || key == "any-time")

internal fun toggleUpcomingSections(
    sections: List<AgendaSection>,
    choices: Map<String, Boolean>,
    today: LocalDate,
): Map<String, Boolean> {
    val expand = sections.none { it.expandedIn(choices, today) }
    return choices + sections.associate { it.key to expand }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun UpcomingAgenda(
    sections: List<AgendaSection>,
    today: LocalDate,
    stateKey: String,
    onEdit: (AgendaEntry) -> Unit,
    onCreateOnDate: (LocalDate) -> Unit,
    modifier: Modifier = Modifier,
    onCompleteAction: ((String) -> Unit)? = null,
    completing: Set<String> = emptySet(),
    menuActions: (AgendaEntry) -> List<ItemCardMenuAction> = { emptyList() },
    zoneId: ZoneId = ZoneId.systemDefault(),
    status: (@Composable () -> Unit)? = null,
    onCreateAction: ((date: LocalDate, title: String, onSuccess: () -> Unit, creationId: String) -> Unit)? = null,
    saving: Boolean = false,
    createError: String? = null,
    onMoreOptions: ((LocalDate, InlineActionCreatorState) -> Unit)? = null,
) {
    var expansion by rememberSaveable(stateKey, stateSaver = ExpansionSaver) {
        mutableStateOf(emptyMap<String, Boolean>())
    }
    val state = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val focusManager = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    val notesState = rememberItemCardNotesState(stateKey)
    val creators = mutableMapOf<String, InlineActionCreatorState>()
    for (section in sections) {
        if (section.date != null && onCreateAction != null) {
            creators[section.key] = key(stateKey, section.key) {
                rememberInlineActionCreatorState("upcoming:$stateKey:${section.key}")
            }
        }
    }
    val collapsible = sections.filter { it.items.isNotEmpty() || creators[it.key]?.expanded == true }
    val collapseAll = collapsible.any { it.expandedIn(expansion, today) }
    Column(modifier.fillMaxSize().testTag("upcoming")) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), horizontalArrangement = Arrangement.End) {
            TextButton(
                onClick = {
                    if (collapseAll) {
                        focusManager.clearFocus(); keyboard?.hide()
                    }
                    expansion = toggleUpcomingSections(collapsible, expansion, today)
                },
                enabled = collapsible.isNotEmpty(),
                modifier = Modifier.heightIn(min = 48.dp).testTag("upcoming:toggle-all"),
            ) {
                Icon(
                    if (collapseAll) Icons.Default.UnfoldLess else Icons.Default.UnfoldMore,
                    null,
                    Modifier.size(18.dp)
                )
                Text(if (collapseAll) "Collapse all" else "Expand all", Modifier.padding(start = 8.dp))
            }
        }
        LazyColumn(
            state = state,
            contentPadding = PaddingValues(bottom = 96.dp),
            modifier = Modifier.fillMaxWidth().weight(1f),
        ) {
            if (status != null) item(key = "upcoming:status", contentType = "status") {
                Column(Modifier.animateItem()) { status() }
            }
            sections.forEach { section ->
                val entries = section.items
                val expanded = section.expandedIn(expansion, today)
                val creator = creators[section.key]
                stickyHeader(key = "header:${section.key}", contentType = "header") {
                    UpcomingHeader(
                        section = section,
                        today = today,
                        expanded = expanded,
                        onToggle = if (entries.isNotEmpty() || creator?.expanded == true) {
                            { expansion = expansion + (section.key to !expanded) }
                        } else {
                            null
                        },
                        onEdit = onEdit,
                        onCreate = creator?.takeUnless { it.expanded }?.let { draft ->
                            {
                                val headerIndex = state.layoutInfo.visibleItemsInfo
                                    .firstOrNull { it.key == "header:${section.key}" }?.index
                                expansion = expansion + (section.key to true)
                                draft.expand()
                                headerIndex?.let { index -> scope.launch { state.animateScrollToItem(index) } }
                            }
                        },
                        creationEnabled = !saving,
                        modifier = Modifier.animateItem(),
                    )
                }
                // Remove collapsed rows from the lazy provider; zero-height exit rows would eagerly compose a large day.
                if (expanded) {
                    if (section.date != null && creator?.expanded == true && onCreateAction != null) {
                        item(key = "create:${section.key}", contentType = "create") {
                            InlineActionCreator(
                                state = creator,
                                saving = saving,
                                error = createError,
                                onCreate = { title, onSuccess ->
                                    onCreateAction(section.date, title, onSuccess, creator.creationId)
                                },
                                onMoreOptions = {
                                    if (onMoreOptions != null) onMoreOptions(section.date, creator)
                                    else onCreateOnDate(section.date)
                                },
                                modifier = Modifier.animateItem().padding(horizontal = 16.dp, vertical = 4.dp)
                                    .testTag("agenda:create:${section.date}"),
                            )
                        }
                    }
                    compoundItemCards(
                        entries = entries,
                        keyPrefix = "agenda:${section.key}",
                        dateContext = section.date,
                        onEdit = onEdit,
                        onCompleteAction = onCompleteAction,
                        completing = completing,
                        menuActions = menuActions,
                        itemModifier = Modifier.padding(horizontal = 16.dp),
                        zoneId = zoneId,
                        notesState = notesState,
                        itemKey = { "agenda:${it.key}" },
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun UpcomingHeader(
    section: AgendaSection,
    today: LocalDate,
    expanded: Boolean,
    onToggle: (() -> Unit)?,
    onEdit: (AgendaEntry) -> Unit,
    onCreate: (() -> Unit)?,
    creationEnabled: Boolean,
    modifier: Modifier = Modifier,
) {
    val rotation by animateFloatAsState(if (expanded) 180f else 0f, label = "Day expansion")
    val label = when {
        section.key == "any-time" -> "Any time"
        section.date == null -> "Missed"
        section.date == today -> "Today"
        section.date == today.plusDays(1) -> "Tomorrow"
        else -> section.date.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM))
    }
    Surface(color = MaterialTheme.colorScheme.surface, modifier = modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("agenda:section:${section.key}")) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.weight(1f).heightIn(min = 48.dp)
                        .then(
                            if (onToggle == null) Modifier else Modifier.clickable(
                                role = Role.Button,
                                onClickLabel = "${if (expanded) "Collapse" else "Expand"} $label",
                                onClick = onToggle,
                            ),
                        )
                        .semantics(mergeDescendants = true) {
                            heading()
                            if (onToggle != null) {
                                val count = section.items.size
                                stateDescription =
                                    "${if (expanded) "Expanded" else "Collapsed"}, $count ${if (count == 1) "item" else "items"}"
                            }
                        },
                ) {
                    if (onToggle != null) {
                        Icon(
                            Icons.Default.ExpandMore,
                            contentDescription = null,
                            modifier = Modifier.size(20.dp).rotate(rotation),
                        )
                    }
                    Column(Modifier.weight(1f)) {
                        Text(
                            label,
                            style = MaterialTheme.typography.titleLarge.copy(fontSize = 20.sp, lineHeight = 26.sp)
                        )
                        section.date?.let { date ->
                            Text(
                                date.format(DateTimeFormatter.ofPattern("EEEE")),
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    if (!expanded && section.items.isNotEmpty()) {
                        Text(
                            section.items.size.toString(),
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(end = 8.dp),
                        )
                    }
                }
                if (onCreate != null) {
                    IconButton(
                        onClick = onCreate,
                        enabled = creationEnabled,
                        modifier = Modifier.size(48.dp).testTag("agenda:add:${section.date}"),
                    ) {
                        Icon(
                            Icons.Default.Add,
                            contentDescription = "Add item on ${section.date}",
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.size(18.dp),
                        )
                    }
                }
            }
            if (section.markers.isNotEmpty()) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    section.markers.forEach { entry ->
                        key(entry.key) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(4.dp),
                                modifier = Modifier.widthIn(min = 48.dp).heightIn(min = 48.dp)
                                    .clickable(role = Role.Button, onClickLabel = "Edit marker") { onEdit(entry) }
                                    .testTag("agenda:${section.occurrenceKey(entry)}")
                                    .semantics(mergeDescendants = true) {
                                        contentDescription = buildString {
                                            append("Marker ${entry.title}")
                                            entry.marker.endDate?.let { append(", ${entry.marker.date} through $it") }
                                        }
                                    },
                            ) {
                                Icon(
                                    Icons.Default.Flag,
                                    contentDescription = null,
                                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.size(12.dp),
                                )
                                Text(
                                    entry.title,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.weight(1f, fill = false).clearAndSetSemantics {},
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}
