package com.example.subroutine_simple.ui.components

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.ui.agenda.AgendaEntry
import java.time.LocalDate
import java.time.ZoneId

// No filtering: calendar/timeline inspectors own which occurrences belong to their date or range.
@Composable
fun DateInspectorList(
    date: LocalDate,
    entries: List<AgendaEntry>,
    modifier: Modifier = Modifier,
    onEdit: ((AgendaEntry) -> Unit)? = null,
    onCompleteAction: ((String) -> Unit)? = null,
    completing: Set<String> = emptySet(),
    menuActions: (AgendaEntry) -> List<ItemCardMenuAction> = { emptyList() },
    contentPadding: PaddingValues = PaddingValues(16.dp),
    state: LazyListState = rememberLazyListState(),
    zoneId: ZoneId = ZoneId.systemDefault(),
) {
    val notesState = rememberItemCardNotesState()
    LazyColumn(
        state = state,
        contentPadding = contentPadding,
        modifier = modifier.testTag("date-inspector:$date"),
    ) {
        compoundItemCards(
            entries = entries,
            keyPrefix = "date-inspector:$date",
            dateContext = date,
            onEdit = onEdit,
            onCompleteAction = onCompleteAction,
            completing = completing,
            menuActions = menuActions,
            zoneId = zoneId,
            notesState = notesState,
        )
        if (entries.isEmpty()) {
            item(key = "date-inspector:$date:empty") {
                Text(
                    "No items",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                )
            }
        }
    }
}

fun LazyListScope.compoundItemCards(
    entries: List<AgendaEntry>,
    keyPrefix: String,
    dateContext: LocalDate? = null,
    onEdit: ((AgendaEntry) -> Unit)? = null,
    onCompleteAction: ((String) -> Unit)? = null,
    completing: Set<String> = emptySet(),
    menuActions: (AgendaEntry) -> List<ItemCardMenuAction> = { emptyList() },
    itemModifier: Modifier = Modifier,
    zoneId: ZoneId = ZoneId.systemDefault(),
    notesState: ItemCardNotesState? = null,
    itemKey: (AgendaEntry) -> String = { "$keyPrefix:${it.key}" },
) {
    itemsIndexed(
        items = entries,
        key = { _, entry -> itemKey(entry) },
        contentType = { _, _ -> "compound-item" },
    ) { index, entry ->
        val actionId = (entry as? AgendaEntry.ActionEntry)?.action?.id
        CompoundItemCard(
            entry = entry,
            position = ItemCardPosition.fromNeighbors(index > 0, index < entries.lastIndex),
            dateContext = dateContext,
            isCompleting = actionId != null && actionId in completing,
            onEdit = onEdit?.let { edit -> { edit(entry) } },
            onComplete = if (actionId != null && onCompleteAction != null) {
                { onCompleteAction(actionId) }
            } else {
                null
            },
            menuActions = menuActions(entry),
            zoneId = zoneId,
            modifier = Modifier.animateItem().then(itemModifier).testTag("$keyPrefix:${entry.key}"),
            notesState = notesState,
        )
    }
}
