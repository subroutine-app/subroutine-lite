package com.example.subroutine_simple.ui.components

import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.data.models.QueueItem
import com.example.subroutine_simple.ui.agenda.AgendaEntry

@Composable
fun QueueItemRow(
    item: QueueItem,
    isCompleting: Boolean,
    onComplete: () -> Unit,
    onClickAction: (QueueItem.ActionItem) -> Unit,
    onClickEvent: (QueueItem.EventItem) -> Unit,
    modifier: Modifier = Modifier,
) {
    when (item) {
        is QueueItem.ActionItem -> CompoundItemCard(
            entry = AgendaEntry.ActionEntry(item.action),
            isCompleting = isCompleting,
            onComplete = onComplete,
            onEdit = { onClickAction(item) },
            modifier = modifier.padding(horizontal = 12.dp, vertical = 4.dp),
        )

        is QueueItem.EventItem -> CompoundItemCard(
            entry = AgendaEntry.EventEntry(item.event),
            onEdit = { onClickEvent(item) },
            modifier = modifier.padding(horizontal = 12.dp, vertical = 4.dp),
        )
    }
}
