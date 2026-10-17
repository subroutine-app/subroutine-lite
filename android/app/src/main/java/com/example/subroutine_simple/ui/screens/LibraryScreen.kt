package com.example.subroutine_simple.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.data.models.ActionTemplate
import com.example.subroutine_simple.data.models.EventTemplate
import com.example.subroutine_simple.data.models.Routine
import com.example.subroutine_simple.ui.components.durationMinutes
import com.example.subroutine_simple.ui.theme.subroutineTonalButtonColors

@Composable
fun RoutinesScreen(
    routines: List<Routine>,
    onEditRoutine: (routineId: String) -> Unit,
    onStartRoutine: ((routineId: String) -> Unit)? = null,
    startActionsEnabled: Boolean = true,
    contentPadding: PaddingValues = PaddingValues(),
    modifier: Modifier = Modifier,
) {
    var query by rememberSaveable { mutableStateOf("") }
    val terms = query.searchTerms()
    val matchingRoutines = routines.filter { routine ->
        terms.matches(
            routine.title,
            routine.content.orEmpty(),
            routine.steps.joinToString("\n") { it.title },
        )
    }

    LazyColumn(
        state = rememberLazyListState(),
        contentPadding = contentPadding,
        verticalArrangement = Arrangement.spacedBy(12.dp),
        modifier = modifier.fillMaxSize(),
    ) {
        item(key = "routines-search", contentType = "search") {
            CollectionSearchField(
                query = query,
                onQueryChange = { query = it },
                label = "Search routines",
                modifier = Modifier.testTag("routines:search"),
            )
        }

        if (routines.isEmpty() || matchingRoutines.isEmpty()) {
            item(key = "routines-empty", contentType = "empty") {
                LibraryEmptyState(
                    title = if (routines.isEmpty()) "No routines yet" else "No matches",

                    modifier = Modifier.testTag(
                        if (routines.isEmpty()) "routines:empty" else "routines:no-matches",
                    ),
                )
            }
        } else {
            items(
                items = matchingRoutines,
                key = { "routine:${it.id}" },
                contentType = { "routine" },
            ) { routine ->
                RoutineLibraryRow(
                    routine = routine,
                    onEdit = { onEditRoutine(routine.id) },
                    onStart = onStartRoutine?.let { callback -> { callback(routine.id) } },
                    startEnabled = startActionsEnabled,
                    modifier = Modifier.testTag("routines:routine:${routine.id}"),
                )
            }
        }
    }
}

@Composable
fun TemplatesScreen(
    actionTemplates: List<ActionTemplate>,
    eventTemplates: List<EventTemplate>,
    onUseActionTemplate: (templateId: String) -> Unit,
    onUseEventTemplate: (templateId: String) -> Unit,
    onEditActionTemplate: (templateId: String) -> Unit,
    onEditEventTemplate: (templateId: String) -> Unit,
    contentPadding: PaddingValues = PaddingValues(),
    modifier: Modifier = Modifier,
) {
    var query by rememberSaveable { mutableStateOf("") }
    val terms = query.searchTerms()
    val matchingActionTemplates = actionTemplates
        .filter { template -> terms.matches(template.title, template.content.orEmpty()) }
        .sortedWith(compareBy<ActionTemplate> { it.sortOrder }.thenBy { it.id })
    val matchingEventTemplates = eventTemplates
        .filter { template -> terms.matches(template.title, template.content.orEmpty()) }
        .sortedWith(compareBy<EventTemplate> { it.sortOrder }.thenBy { it.id })
    val sourceIsEmpty = actionTemplates.isEmpty() && eventTemplates.isEmpty()
    val matchesAreEmpty = matchingActionTemplates.isEmpty() && matchingEventTemplates.isEmpty()

    LazyColumn(
        state = rememberLazyListState(),
        contentPadding = contentPadding,
        verticalArrangement = Arrangement.spacedBy(12.dp),
        modifier = modifier.fillMaxSize(),
    ) {
        item(key = "templates-search", contentType = "search") {
            CollectionSearchField(
                query = query,
                onQueryChange = { query = it },
                label = "Search templates",
                modifier = Modifier.testTag("templates:search"),
            )
        }

        if (sourceIsEmpty || matchesAreEmpty) {
            item(key = "templates-empty", contentType = "empty") {
                LibraryEmptyState(
                    title = if (sourceIsEmpty) "No templates yet" else "No matches",

                    modifier = Modifier.testTag(
                        if (sourceIsEmpty) "templates:empty" else "templates:no-matches",
                    ),
                )
            }
        } else {
            if (matchingActionTemplates.isNotEmpty()) {
                item(key = "action-templates-heading", contentType = "section-heading") {
                    LibrarySectionHeading(
                        title = "Action templates",
                        modifier = Modifier.testTag("templates:section:actions"),
                    )
                }
                items(
                    items = matchingActionTemplates,
                    key = { "action-template:${it.id}" },
                    contentType = { "action-template" },
                ) { template ->
                    TemplateLibraryRow(
                        title = template.title,
                        content = template.content,
                        metadata = listOfNotNull(
                            template.naiveTime?.let { "At ${it.take(5)}" },
                            template.duration.displayDuration(),
                        ).joinToString(" · ").ifEmpty { null },
                        onUse = { onUseActionTemplate(template.id) },
                        onEdit = { onEditActionTemplate(template.id) },
                        modifier = Modifier.testTag("templates:action:${template.id}"),
                    )
                }
            }

            if (matchingEventTemplates.isNotEmpty()) {
                item(key = "event-templates-heading", contentType = "section-heading") {
                    LibrarySectionHeading(
                        title = "Event templates",
                        modifier = Modifier.testTag("templates:section:events"),
                    )
                }
                items(
                    items = matchingEventTemplates,
                    key = { "event-template:${it.id}" },
                    contentType = { "event-template" },
                ) { template ->
                    TemplateLibraryRow(
                        title = template.title,
                        content = template.content,
                        metadata = template.duration.displayDuration(),
                        onUse = { onUseEventTemplate(template.id) },
                        onEdit = { onEditEventTemplate(template.id) },
                        modifier = Modifier.testTag("templates:event:${template.id}"),
                    )
                }
            }
        }
    }
}

@Composable
private fun CollectionSearchField(
    query: String,
    onQueryChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
) {
    OutlinedTextField(
        value = query,
        onValueChange = onQueryChange,
        label = { Text(label) },
        leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        shape = MaterialTheme.shapes.large,
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp),
    )
}

@Composable
private fun LibrarySectionHeading(
    title: String,
    modifier: Modifier = Modifier,
) {
    Text(
        text = title,
        style = MaterialTheme.typography.titleLarge,
        color = MaterialTheme.colorScheme.onSurface,
        modifier = modifier.padding(start = 24.dp, end = 24.dp, top = 12.dp),
    )
}

@Composable
internal fun RoutineLibraryRow(
    routine: Routine,
    onEdit: () -> Unit,
    onStart: (() -> Unit)?,
    startEnabled: Boolean,
    modifier: Modifier = Modifier,
) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
        ),
        shape = MaterialTheme.shapes.extraLarge,
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp),
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = routine.title,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            routine.content?.takeIf(String::isNotBlank)?.let { content ->
                Text(
                    text = content,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Text(
                text = when (routine.steps.size) {
                    0 -> "No steps"
                    1 -> "1 step"
                    else -> "${routine.steps.size} steps"
                },
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TextButton(
                    onClick = onEdit,
                    modifier = Modifier.testTag("library:routine:${routine.id}:edit"),
                ) {
                    Icon(Icons.Default.Edit, contentDescription = null)
                    Text("Edit", modifier = Modifier.padding(start = 8.dp))
                }
                if (onStart != null) {
                    FilledTonalButton(
                        onClick = onStart,
                        enabled = startEnabled,
                        colors = subroutineTonalButtonColors(),
                        modifier = Modifier.testTag("library:routine:${routine.id}:start"),
                    ) {
                        Icon(Icons.Default.PlayArrow, contentDescription = null)
                        Text("Start now", modifier = Modifier.padding(start = 8.dp))
                    }
                }
            }
        }
    }
}

@Composable
internal fun TemplateLibraryRow(
    title: String,
    content: String?,
    metadata: String?,
    onUse: () -> Unit,
    onEdit: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
        ),
        shape = MaterialTheme.shapes.large,
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp),
    ) {
        Column(
            modifier = Modifier.padding(18.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            content?.takeIf(String::isNotBlank)?.let { value ->
                Text(
                    text = value,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            metadata?.let { value ->
                Text(
                    text = value,
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
            ) {
                TextButton(onClick = onEdit) {
                    Icon(Icons.Default.Edit, contentDescription = null)
                    Text("Edit", modifier = Modifier.padding(start = 8.dp))
                }
                OutlinedButton(onClick = onUse) {
                    Icon(Icons.Default.Add, contentDescription = null)
                    Text("Use", modifier = Modifier.padding(start = 8.dp))
                }
            }
        }
    }
}

@Composable
private fun LibraryEmptyState(
    title: String,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 32.dp, vertical = 64.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(title, style = MaterialTheme.typography.titleLarge)
    }
}

private fun String.searchTerms(): List<String> =
    trim().lowercase().split(Regex("\\s+")).filter(String::isNotBlank)

private fun List<String>.matches(vararg values: String): Boolean {
    if (isEmpty()) return true
    val haystack = values.joinToString("\n").lowercase()
    return all(haystack::contains)
}

private fun String?.displayDuration(): String? {
    val minutes = durationMinutes(this) ?: return null
    val hours = minutes / 60
    val remainingMinutes = minutes % 60
    return when {
        hours == 0L -> "${remainingMinutes}m"
        remainingMinutes == 0L -> "${hours}h"
        else -> "${hours}h ${remainingMinutes}m"
    }
}
