package com.example.subroutine_simple.ui.components

import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.ui.agenda.AgendaEntry

@Composable
fun ActionListItem(
    action: Action,
    isCompleting: Boolean,
    onComplete: () -> Unit,
    onClick: () -> Unit,
    selectionMode: Boolean = false,
    selected: Boolean = false,
    onToggleSelection: () -> Unit = {},
    modifier: Modifier = Modifier,
    position: ItemCardPosition = ItemCardPosition.Standalone,
) {
    CompoundItemCard(
        entry = AgendaEntry.ActionEntry(action),
        position = position,
        isCompleting = isCompleting,
        onComplete = onComplete,
        onEdit = onClick,
        selection = if (selectionMode) ItemCardSelection(selected, onToggleSelection) else null,
        modifier = modifier.padding(
            horizontal = 12.dp,
            vertical = if (position == ItemCardPosition.Standalone) 4.dp else 0.dp,
        ),
    )
}
