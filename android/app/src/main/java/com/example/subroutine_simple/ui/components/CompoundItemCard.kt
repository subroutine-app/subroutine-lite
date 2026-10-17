package com.example.subroutine_simple.ui.components

import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth

import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.toggleable

import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Event
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.Flag

import androidx.compose.material.icons.filled.NotificationsNone
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.filled.RadioButtonUnchecked
import androidx.compose.material.icons.filled.Repeat
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem

import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow

import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.ui.agenda.AgendaEntry
import com.example.subroutine_simple.ui.theme.subroutineCompletionCheckboxColors
import com.example.subroutine_simple.ui.theme.SubroutineTheme
import java.time.LocalDate
import java.time.ZoneId

// Include only operations the host can persist; absence means unavailable, not a no-op.
data class ItemCardMenuAction(val id: String, val label: String, val onClick: () -> Unit)

data class ItemCardSelection(val selected: Boolean, val onToggle: () -> Unit)

@Stable
class ItemCardNotesState internal constructor(initial: Set<String> = emptySet()) {
    private var expanded by mutableStateOf(initial)

    fun isExpanded(key: String): Boolean = key in expanded

    fun toggle(key: String) {
        expanded = if (key in expanded) expanded - key else expanded + key
    }

    internal companion object {
        val Saver = listSaver<ItemCardNotesState, String>(
            save = { it.expanded.toList() },
            restore = { ItemCardNotesState(it.toSet()) },
        )
    }
}

@Composable
fun rememberItemCardNotesState(stateKey: Any? = null): ItemCardNotesState =
    rememberSaveable(stateKey, saver = ItemCardNotesState.Saver) { ItemCardNotesState() }

@Composable
fun CompoundItemCard(
    entry: AgendaEntry,
    modifier: Modifier = Modifier,
    position: ItemCardPosition = ItemCardPosition.Standalone,
    dateContext: LocalDate? = null,
    isCompleting: Boolean = false,
    onEdit: (() -> Unit)? = null,
    onComplete: (() -> Unit)? = null,
    menuActions: List<ItemCardMenuAction> = emptyList(),
    selection: ItemCardSelection? = null,
    zoneId: ZoneId = ZoneId.systemDefault(),
    notesState: ItemCardNotesState? = null,
) {
    val content = projectCompoundItem(entry, dateContext, zoneId)
    val tag = "item-card:${dateContext ?: "undated"}:${entry.key}"
    val details = notesState ?: rememberItemCardNotesState(entry.key)
    val notesExpanded = details.isExpanded(entry.key)
    val notesRotation by animateFloatAsState(if (notesExpanded) 180f else 0f, label = "Card notes")
    val foreground = if (selection?.selected == true) {
        MaterialTheme.colorScheme.onSecondaryContainer
    } else {
        MaterialTheme.colorScheme.onSurface
    }
    val muted = if (selection?.selected == true) foreground else MaterialTheme.colorScheme.onSurfaceVariant
    val edit = onEdit.takeIf { selection == null && !isCompleting }
    val actions = if (selection != null || isCompleting) emptyList() else buildList {
        if (edit != null) add(ItemCardMenuAction("edit", "Edit ${content.kind.name.lowercase()}", edit))
        addAll(menuActions)
    }
    var menuOpen by remember(tag, selection != null, isCompleting, actions.isNotEmpty()) { mutableStateOf(false) }
    val interaction = when {
        selection != null -> Modifier.toggleable(
            value = selection.selected,
            role = Role.Checkbox,
            onValueChange = { selection.onToggle() },
        )

        actions.isNotEmpty() -> Modifier
            .onPreviewKeyEvent { event ->
                if (event.key == Key.Menu || (event.key == Key.F10 && event.isShiftPressed)) {
                    if (event.type == KeyEventType.KeyDown) menuOpen = true
                    true
                } else {
                    false
                }
            }
            .combinedClickable(
                role = Role.Button,
                onClickLabel = if (edit != null) "Edit ${content.kind.name.lowercase()}" else "Actions for ${entry.title}",
                onClick = edit ?: { menuOpen = true },
                onLongClickLabel = "Actions for ${entry.title}",
                onLongClick = { menuOpen = true },
            )

        else -> Modifier
    }

    Surface(
        color = if (selection?.selected == true) {
            MaterialTheme.colorScheme.secondaryContainer
        } else {
            MaterialTheme.colorScheme.surfaceContainerLow
        },
        contentColor = foreground,
        shape = CardAppearance.shape(position),
        modifier = modifier.fillMaxWidth()
            .itemCardBorder(position, CardAppearance.borderColor(MaterialTheme.colorScheme)),
    ) {
        Column(Modifier.animateContentSize().testTag(tag)) {
            Box {
                ItemCardRow(
                    modifier = Modifier.then(interaction).testTag("$tag:identity")
                        .semantics(mergeDescendants = true) {
                            if (content.completed) stateDescription = "Completed"
                            if (actions.isNotEmpty()) {
                                customActions = actions.map { action ->
                                    CustomAccessibilityAction(action.label) {
                                        menuOpen = false
                                        action.onClick()
                                        true
                                    }
                                }
                            }
                        },
                    leading = {
                        when {
                            selection != null -> Checkbox(
                                checked = selection.selected,
                                onCheckedChange = { selection.onToggle() },
                                modifier = Modifier.testTag("$tag:select").semantics {
                                    contentDescription = "Select ${entry.title}"
                                },
                            )

                            isCompleting -> CircularProgressIndicator(
                                modifier = Modifier.size(24.dp).testTag("$tag:completing").semantics {
                                    contentDescription = "Completing ${entry.title}"
                                },
                                strokeWidth = 2.dp,
                            )

                            content.kind == ItemCardKind.Action && !content.completed && onComplete != null -> Checkbox(
                                checked = false,
                                onCheckedChange = { if (it) onComplete() },
                                colors = subroutineCompletionCheckboxColors(),
                                modifier = Modifier.testTag("$tag:complete").semantics {
                                    contentDescription = "Complete ${entry.title}"
                                },
                            )

                            else -> Icon(
                                imageVector = when (content.kind) {
                                    ItemCardKind.Action -> if (content.completed) Icons.Default.Check else Icons.Default.RadioButtonUnchecked
                                    ItemCardKind.Event -> Icons.Default.Event
                                    ItemCardKind.Marker -> Icons.Default.Flag
                                    ItemCardKind.Signal -> Icons.Default.NotificationsNone
                                },
                                contentDescription = content.kind.name,
                                tint = if (content.completed) SubroutineTheme.colors.stable else muted,
                                modifier = Modifier.size(20.dp),
                            )
                        }
                    },
                    title = {
                        Text(
                            text = entry.title,
                            style = MaterialTheme.typography.titleMedium,
                            textDecoration = if (content.completed) TextDecoration.LineThrough else null,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.testTag("$tag:title"),
                        )
                    },
                    metadata = {
                        val timing = content.timing?.takeUnless { it == "Any time" && dateContext != null }
                        if (timing != null || content.repeating || content.pinned) {
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                if (timing != null) {
                                    Text(
                                        timing, style = MaterialTheme.typography.bodySmall, color = muted,
                                        maxLines = 1, overflow = TextOverflow.Ellipsis,
                                        modifier = Modifier.weight(1f, fill = false),
                                    )
                                }
                                if (content.repeating) {
                                    Icon(Icons.Default.Repeat, "Repeats", Modifier.size(14.dp), tint = muted)
                                }
                                if (content.pinned) {
                                    Icon(Icons.Default.PushPin, "Pinned", Modifier.size(14.dp), tint = muted)
                                }
                            }
                        }
                    },
                    trailing = {
                        if (content.notes != null) {
                            IconButton(
                                onClick = { details.toggle(entry.key) },
                                modifier = Modifier.size(CardAppearance.controlSize).testTag("$tag:notes-toggle")
                                    .semantics {
                                        stateDescription = if (notesExpanded) "Expanded" else "Collapsed"
                                    },
                            ) {
                                Icon(
                                    Icons.Default.ExpandMore,
                                    contentDescription = "${if (notesExpanded) "Collapse" else "Expand"} notes for ${entry.title}",
                                    tint = muted,
                                    modifier = Modifier.size(20.dp).rotate(notesRotation),
                                )
                            }
                        }
                    },
                )
                CardActions(actions, tag, expanded = menuOpen, onDismiss = { menuOpen = false })
            }
            if (notesExpanded) {
                content.notes?.let { notes ->
                    CardNotes(notes, tag, selectable = selection == null)
                }
            }
        }
    }
}

@Composable
private fun CardActions(
    actions: List<ItemCardMenuAction>,
    tag: String,
    expanded: Boolean,
    onDismiss: () -> Unit,
) {
    DropdownMenu(expanded = expanded && actions.isNotEmpty(), onDismissRequest = onDismiss) {
        actions.forEach { action ->
            DropdownMenuItem(
                text = { Text(action.label) },
                onClick = {
                    onDismiss()
                    action.onClick()
                },
                modifier = Modifier.testTag("$tag:${action.id}"),
            )
        }
    }
}

@Composable
private fun CardNotes(notes: String, tag: String, selectable: Boolean) {
    val text: @Composable () -> Unit = {
        Text(
            text = notes,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.fillMaxWidth()
                .padding(start = CardAppearance.padding, end = CardAppearance.padding, bottom = CardAppearance.padding)
                .testTag("$tag:notes"),
        )
    }
    if (selectable) SelectionContainer { text() } else text()
}
