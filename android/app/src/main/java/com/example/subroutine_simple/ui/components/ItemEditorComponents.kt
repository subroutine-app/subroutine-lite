package com.example.subroutine_simple.ui.components

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp

@Composable
fun ItemIdentityEditor(
    title: String,
    onTitleChange: (String) -> Unit,
    notes: String,
    onNotesChange: (String) -> Unit,
    enabled: Boolean,
    titlePlaceholder: String,
    modifier: Modifier = Modifier,
    showNotes: Boolean = true,
    autoFocus: Boolean = false,
    titleImeAction: ImeAction = ImeAction.Next,
    onTitleImeAction: (() -> Unit)? = null,
) {
    var notesExpanded by rememberSaveable { mutableStateOf(notes.isNotBlank()) }
    val titleFocusRequester = remember { FocusRequester() }
    LaunchedEffect(autoFocus) {
        if (autoFocus) titleFocusRequester.requestFocus()
    }
    val textColor = if (enabled) {
        MaterialTheme.colorScheme.onSurface
    } else {
        MaterialTheme.colorScheme.onSurface.copy(alpha = 0.38f)
    }

    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        BasicTextField(
            value = title,
            onValueChange = onTitleChange,
            enabled = enabled,
            singleLine = true,
            textStyle = MaterialTheme.typography.headlineMedium.copy(color = textColor),
            cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
            keyboardOptions = KeyboardOptions(
                capitalization = KeyboardCapitalization.Sentences,
                imeAction = titleImeAction,
            ),
            keyboardActions = if (onTitleImeAction == null) {
                KeyboardActions()
            } else {
                KeyboardActions(
                    onDone = { onTitleImeAction() },
                    onGo = { onTitleImeAction() },
                    onNext = { onTitleImeAction() },
                )
            },
            modifier = Modifier
                .fillMaxWidth()
                .focusRequester(titleFocusRequester)
                .sizeIn(minHeight = 56.dp)
                .padding(vertical = 10.dp),
            decorationBox = { innerTextField ->
                Box(contentAlignment = Alignment.CenterStart) {
                    if (title.isEmpty()) {
                        Text(
                            text = titlePlaceholder,
                            style = MaterialTheme.typography.headlineMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    innerTextField()
                }
            },
        )

        if (showNotes && (notesExpanded || notes.isNotBlank())) {
            Surface(
                color = MaterialTheme.colorScheme.surfaceContainerLow,
                shape = MaterialTheme.shapes.large,
            ) {
                BasicTextField(
                    value = notes,
                    onValueChange = onNotesChange,
                    enabled = enabled,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(color = textColor),
                    cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    modifier = Modifier
                        .fillMaxWidth()
                        .sizeIn(minHeight = 112.dp)
                        .padding(16.dp),
                    decorationBox = { innerTextField ->
                        Box {
                            if (notes.isEmpty()) {
                                Text(
                                    text = "Notes",
                                    style = MaterialTheme.typography.bodyLarge,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            innerTextField()
                        }
                    },
                )
            }
            if (notes.isBlank()) {
                TextButton(
                    onClick = { notesExpanded = false },
                    enabled = enabled,
                    modifier = Modifier.align(Alignment.End),
                ) {
                    Text("Hide notes")
                }
            }
        } else if (showNotes) {
            TextButton(
                onClick = { notesExpanded = true },
                enabled = enabled,
            ) {
                Icon(Icons.Default.Add, contentDescription = null)
                Text("Add notes", modifier = Modifier.padding(start = 8.dp))
            }
        }
    }
}

@Composable
fun ProgressiveEditorSection(
    title: String,
    summary: String,
    modifier: Modifier = Modifier,
    initiallyExpanded: Boolean = false,
    forceExpanded: Boolean = false,
    enabled: Boolean = true,
    content: @Composable ColumnScope.() -> Unit,
) {
    var expanded by rememberSaveable(title) { mutableStateOf(initiallyExpanded) }
    val visible = expanded || forceExpanded

    Surface(
        modifier = modifier
            .fillMaxWidth()
            .animateContentSize(),
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = MaterialTheme.shapes.extraLarge,
    ) {
        Column {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .sizeIn(minHeight = 64.dp)
                    .clickable(
                        enabled = enabled && !forceExpanded,
                        role = Role.Button,
                        onClickLabel = if (visible) "Hide $title controls" else "Edit $title",
                    ) { expanded = !expanded }
                    .padding(horizontal = 20.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = title,
                        style = MaterialTheme.typography.titleMedium,
                        modifier = Modifier.semantics { heading() },
                    )
                    Text(
                        text = summary,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Icon(
                    imageVector = if (visible) Icons.Default.KeyboardArrowUp else Icons.Default.KeyboardArrowDown,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            AnimatedVisibility(visible = visible) {
                Column(
                    modifier = Modifier.padding(start = 20.dp, end = 20.dp, bottom = 20.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                    content = content,
                )
            }
        }
    }
}

@Composable
fun ItemActionsMenu(
    itemKind: String,
    enabled: Boolean,
    onDuplicate: (() -> Unit)? = null,
    onSaveForReuse: (() -> Unit)? = null,
    onRestore: (() -> Unit)? = null,
    onDelete: (() -> Unit)? = null,
) {
    var expanded by rememberSaveable { mutableStateOf(false) }

    IconButton(
        onClick = { expanded = true },
        enabled = enabled,
    ) {
        Icon(Icons.Default.MoreVert, contentDescription = "More $itemKind actions")
    }
    DropdownMenu(
        expanded = expanded,
        onDismissRequest = { expanded = false },
    ) {
        onRestore?.let { action ->
            DropdownMenuItem(
                text = { Text("Restore") },
                onClick = {
                    expanded = false
                    action()
                },
            )
        }
        onSaveForReuse?.let { action ->
            DropdownMenuItem(
                text = { Text("Save for reuse") },
                onClick = {
                    expanded = false
                    action()
                },
            )
        }
        onDuplicate?.let { action ->
            DropdownMenuItem(
                text = { Text("Duplicate") },
                onClick = {
                    expanded = false
                    action()
                },
            )
        }
        onDelete?.let { action ->
            DropdownMenuItem(
                text = { Text("Delete") },
                leadingIcon = {
                    Icon(
                        Icons.Default.Delete,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.error,
                    )
                },
                onClick = {
                    expanded = false
                    action()
                },
            )
        }
    }
}
