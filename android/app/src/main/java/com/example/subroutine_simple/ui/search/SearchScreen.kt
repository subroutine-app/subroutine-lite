package com.example.subroutine_simple.ui.search

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
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

object SearchTestTags {
    const val Root = "search"
    const val Field = "search:field"
    const val Clear = "search:clear"
    const val TypeFilters = "search:filters:type"
    const val SmartFilters = "search:filters:smart"
    const val Sort = "search:sort"
    const val Results = "search:results"
    const val Prompt = "search:state:prompt"
    const val Empty = "search:state:empty"
    const val NoMatches = "search:state:no-matches"

    fun typeFilter(kind: SearchKind): String = "search:filter:type:${kind.keyPrefix}"
    fun smartFilter(filter: SearchSmartFilter): String = "search:filter:smart:${filter.key}"
    fun sortOption(sort: SearchSort): String = "search:sort:${sort.key}"
    fun result(key: String): String = "search:result:$key"
    fun resultAction(key: String): String = "search:result:$key:action"
}

@Composable
fun SearchScreen(
    request: SearchRequest,
    projection: SearchProjection,
    onRequestChange: (SearchRequest) -> Unit,
    onEditLiveItem: (SearchResource.Live) -> Unit,
    onUseSavedTemplate: (SearchResource.Saved) -> Unit,
    modifier: Modifier = Modifier,
    contentPadding: PaddingValues = PaddingValues(),
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .padding(contentPadding)
            .imePadding()
            .testTag(SearchTestTags.Root),
    ) {
        OutlinedTextField(
            value = request.query,
            onValueChange = { onRequestChange(request.copy(query = it)) },
            label = { Text("Search items") },
            placeholder = { Text("Title, note, or routine step") },
            leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
            trailingIcon = if (request.query.isNotEmpty()) {
                {
                    IconButton(
                        onClick = { onRequestChange(request.copy(query = "")) },
                        modifier = Modifier.testTag(SearchTestTags.Clear),
                    ) {
                        Icon(Icons.Default.Clear, contentDescription = "Clear search")
                    }
                }
            } else {
                null
            },
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            shape = MaterialTheme.shapes.large,
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 12.dp)
                .testTag(SearchTestTags.Field),
        )

        SearchFilterRow(
            label = "Type",
            modifier = Modifier.testTag(SearchTestTags.TypeFilters),
        ) {
            SearchKind.entries.forEach { kind ->
                FilterChip(
                    selected = kind in request.kinds,
                    onClick = {
                        onRequestChange(
                            request.copy(kinds = request.kinds.toggled(kind)),
                        )
                    },
                    label = { Text(kind.filterLabel) },
                    colors = FilterChipDefaults.filterChipColors(
                        selectedContainerColor = MaterialTheme.colorScheme.primaryContainer,
                        selectedLabelColor = MaterialTheme.colorScheme.onSurface,
                        selectedLeadingIconColor = MaterialTheme.colorScheme.onSurface,
                        selectedTrailingIconColor = MaterialTheme.colorScheme.onSurface,
                    ),
                    modifier = Modifier.testTag(SearchTestTags.typeFilter(kind)),
                )
            }
        }

        SearchFilterRow(
            label = "Smart lists",
            modifier = Modifier.testTag(SearchTestTags.SmartFilters),
        ) {
            SearchSmartFilter.entries.forEach { filter ->
                FilterChip(
                    selected = filter in request.smartFilters,
                    onClick = {
                        onRequestChange(
                            request.copy(smartFilters = request.smartFilters.toggled(filter)),
                        )
                    },
                    label = { Text(filter.label) },
                    colors = FilterChipDefaults.filterChipColors(
                        selectedContainerColor = MaterialTheme.colorScheme.primaryContainer,
                        selectedLabelColor = MaterialTheme.colorScheme.onSurface,
                        selectedLeadingIconColor = MaterialTheme.colorScheme.onSurface,
                        selectedTrailingIconColor = MaterialTheme.colorScheme.onSurface,
                    ),
                    modifier = Modifier.testTag(SearchTestTags.smartFilter(filter)),
                )
            }
        }

        SearchSortSelector(
            selected = request.sort,
            onSelected = { onRequestChange(request.copy(sort = it)) },
        )

        when (projection) {
            SearchProjection.Empty -> SearchMessage(
                title = "Nothing to search yet",

                modifier = Modifier.weight(1f).testTag(SearchTestTags.Empty),
            )
            SearchProjection.Prompt -> SearchMessage(
                title = "Search your items",

                modifier = Modifier.weight(1f).testTag(SearchTestTags.Prompt),
            )
            SearchProjection.NoMatches -> SearchMessage(
                title = "No matches",
                message = "Try another search or filter.",
                modifier = Modifier.weight(1f).testTag(SearchTestTags.NoMatches),
            )
            is SearchProjection.Results -> SearchResults(
                results = projection.items,
                onEditLiveItem = onEditLiveItem,
                onUseSavedTemplate = onUseSavedTemplate,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun SearchFilterRow(
    label: String,
    modifier: Modifier = Modifier,
    content: @Composable RowScope.() -> Unit,
) {
    Column(modifier = modifier.fillMaxWidth()) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp),
        )
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            content = content,
        )
    }
}

@Composable
private fun SearchSortSelector(
    selected: SearchSort,
    onSelected: (SearchSort) -> Unit,
) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.End,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = "Sort",
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(end = 8.dp),
        )
        Box {
            OutlinedButton(
                onClick = { expanded = true },
                modifier = Modifier.testTag(SearchTestTags.Sort),
            ) {
                Text(selected.label)
            }
            DropdownMenu(
                expanded = expanded,
                onDismissRequest = { expanded = false },
            ) {
                SearchSort.entries.forEach { sort ->
                    DropdownMenuItem(
                        text = { Text(sort.label) },
                        onClick = {
                            expanded = false
                            onSelected(sort)
                        },
                        modifier = Modifier.testTag(SearchTestTags.sortOption(sort)),
                    )
                }
            }
        }
    }
}

@Composable
private fun SearchResults(
    results: List<SearchResult>,
    onEditLiveItem: (SearchResource.Live) -> Unit,
    onUseSavedTemplate: (SearchResource.Saved) -> Unit,
    modifier: Modifier = Modifier,
) {
    LazyColumn(
        modifier = modifier
            .fillMaxWidth()
            .testTag(SearchTestTags.Results),
        verticalArrangement = Arrangement.spacedBy(8.dp),
        contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
    ) {
        items(
            items = results,
            key = SearchResult::key,
            contentType = { it.resource.kind },
        ) { result ->
            SearchResultRow(
                result = result,
                onEditLiveItem = onEditLiveItem,
                onUseSavedTemplate = onUseSavedTemplate,
            )
        }
    }
}

@Composable
private fun SearchResultRow(
    result: SearchResult,
    onEditLiveItem: (SearchResource.Live) -> Unit,
    onUseSavedTemplate: (SearchResource.Saved) -> Unit,
) {
    Surface(
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.surfaceContainerLow,
    ) {
        ListItem(
            headlineContent = {
                Text(
                    text = result.displayTitle,
                    style = MaterialTheme.typography.titleMedium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            },
            supportingContent = {
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(
                        text = result.typeLabel,
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    result.supportingText?.let { supporting ->
                        Text(
                            text = supporting,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 3,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            },
            trailingContent = {
                when (val resource = result.resource) {
                    is SearchResource.Live -> TextButton(
                        onClick = { onEditLiveItem(resource) },
                        modifier = Modifier.testTag(SearchTestTags.resultAction(result.key)),
                    ) {
                        Text("Edit")
                    }
                    is SearchResource.Saved -> OutlinedButton(
                        onClick = { onUseSavedTemplate(resource) },
                        modifier = Modifier.testTag(SearchTestTags.resultAction(result.key)),
                    ) {
                        Text("Use")
                    }
                }
            },
            colors = ListItemDefaults.colors(containerColor = androidx.compose.ui.graphics.Color.Transparent),
            modifier = Modifier.testTag(SearchTestTags.result(result.key)),
        )
    }
}

@Composable
private fun SearchMessage(
    title: String,
    modifier: Modifier = Modifier,
    message: String? = null,
) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 32.dp, vertical = 48.dp),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Surface(
                color = MaterialTheme.colorScheme.surfaceContainerLow,
                contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                shape = MaterialTheme.shapes.extraLarge,
                modifier = Modifier.size(64.dp),
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Icon(Icons.Default.Search, contentDescription = null)
                }
            }
            Text(text = title, style = MaterialTheme.typography.titleLarge)
            message?.let {
                Text(
                    text = it,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

private fun <T> Set<T>.toggled(value: T): Set<T> = if (value in this) this - value else this + value
