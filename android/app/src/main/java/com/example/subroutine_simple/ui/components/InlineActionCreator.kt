package com.example.subroutine_simple.ui.components

import android.os.Bundle
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme

import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember

import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.error
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.data.models.UuidV7

@Stable
class InlineActionCreatorState internal constructor(
    val key: String,
    saved: Bundle?,
    private val persist: (Bundle) -> Unit,
) {
    var title by mutableStateOf(saved?.getString("title").orEmpty())
        private set
    var expanded by mutableStateOf(saved?.getBoolean("expanded") ?: false)
        private set
    var creationId by mutableStateOf(saved?.getString("creationId") ?: UuidV7.newString())
        private set
    internal var attempted by mutableStateOf(saved?.getBoolean("attempted") ?: false)
        private set
    internal var focusRequested by mutableStateOf(false)
    private var revision = saved?.getLong("revision") ?: 0L

    init {
        save()
    }

    fun expand() {
        if (expanded) return
        expanded = true
        focusRequested = true
        save()
    }

    fun cancel() {
        title = ""
        expanded = false
        attempted = false
        focusRequested = false
        creationId = UuidV7.newString()
        revision += 1
        save()
    }

    fun complete(acceptedCreationId: String) {
        if (creationId == acceptedCreationId) cancel()
    }

    internal fun edit(title: String) {
        if (this.title == title) return
        this.title = title
        attempted = false
        revision += 1
        save()
    }

    internal fun submit(onCreate: (String, () -> Unit) -> Unit) {
        val title = title.trim().takeIf { it.isNotEmpty() } ?: return
        val submittedId = creationId
        val submittedRevision = revision
        attempted = true
        save()
        onCreate(title) {
            if (creationId == submittedId) {
                if (revision == submittedRevision) {
                    cancel()
                } else {
                    // The accepted item is finished, but edits made since submission remain a new draft.
                    creationId = UuidV7.newString()
                    attempted = false
                    save()
                }
            }
        }
    }

    private fun save() = persist(Bundle().apply {
        putString("title", title)
        putBoolean("expanded", expanded)
        putString("creationId", creationId)
        putBoolean("attempted", attempted)
        putLong("revision", revision)
    })
}

@Composable
fun rememberInlineActionCreatorState(key: String): InlineActionCreatorState =
    LocalInlineActionDrafts.current.draft(key)

@Composable
fun InlineActionCreator(
    state: InlineActionCreatorState,
    saving: Boolean,
    onCreate: (title: String, onSuccess: () -> Unit) -> Unit,
    modifier: Modifier = Modifier,
    error: String? = null,
    onMoreOptions: (() -> Unit)? = null,
) {
    val focusRequester = remember(state) { FocusRequester() }
    val focusManager = LocalFocusManager.current
    var hasFocus by remember(state) { mutableStateOf(false) }
    val tag = "inline-creator:${state.key}"

    fun cancel() {
        if (saving) return
        if (hasFocus) focusManager.clearFocus()
        state.cancel()
    }

    fun submit() {
        if (!saving) state.submit(onCreate)
    }

    BackHandler(enabled = state.expanded && hasFocus) { cancel() }
    LaunchedEffect(state, state.focusRequested) {
        if (state.expanded && state.focusRequested) {
            focusRequester.requestFocus()
            state.focusRequested = false
        }
    }

    val failure = error.takeIf { state.attempted && !saving }
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = CardAppearance.shape(ItemCardPosition.Standalone),
        modifier = modifier.fillMaxWidth().testTag(tag)
            .itemCardBorder(ItemCardPosition.Standalone, CardAppearance.borderColor(MaterialTheme.colorScheme)),
    ) {
        if (!state.expanded) {
            ItemCardRow(
                modifier = Modifier.testTag("$tag:expand")
                    .clickable(enabled = !saving, role = Role.Button, onClick = state::expand),
                leading = {
                    Icon(Icons.Default.Add, contentDescription = null, modifier = Modifier.size(20.dp))
                },
                title = {
                    Text(
                        "Add item",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.testTag("$tag:prompt"),
                    )
                },
            )
        } else {
            Column(
                modifier = Modifier.fillMaxWidth()
                    .onFocusChanged { hasFocus = it.hasFocus }
                    .focusGroup()
                    .onPreviewKeyEvent { event ->
                        if (event.key == Key.Escape) {
                            if (event.type == KeyEventType.KeyDown) cancel()
                            true
                        } else {
                            false
                        }
                    },
            ) {
                ItemCardRow(
                    modifier = Modifier.testTag("$tag:row"),
                    leading = {
                        IconButton(
                            onClick = ::submit,
                            enabled = !saving && state.title.isNotBlank(),
                            modifier = Modifier.size(CardAppearance.controlSize).testTag("$tag:submit"),
                        ) {
                            if (saving && state.attempted) {
                                CircularProgressIndicator(
                                    strokeWidth = 2.dp,
                                    modifier = Modifier.size(24.dp).semantics { contentDescription = "Saving action" },
                                )
                            } else {
                                Icon(Icons.Default.Check, contentDescription = "Create action")
                            }
                        }
                    },
                    title = {
                        BasicTextField(
                            value = state.title,
                            onValueChange = state::edit,
                            singleLine = true,
                            readOnly = saving,
                            textStyle = MaterialTheme.typography.titleMedium.copy(color = MaterialTheme.colorScheme.onSurface),
                            cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                            keyboardOptions = KeyboardOptions(
                                capitalization = KeyboardCapitalization.Sentences,
                                imeAction = ImeAction.Done,
                            ),
                            keyboardActions = KeyboardActions(onDone = { submit() }),
                            decorationBox = { input ->
                                Box(contentAlignment = Alignment.CenterStart) {
                                    if (state.title.isEmpty()) {
                                        Text(
                                            "Add item",
                                            style = MaterialTheme.typography.titleMedium,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                    input()
                                }
                            },
                            modifier = Modifier.fillMaxWidth().focusRequester(focusRequester)
                                .testTag("$tag:title").semantics {
                                    contentDescription = "Action title"
                                    if (failure != null) error(failure)
                                },
                        )
                    },
                    trailing = {
                        IconButton(
                            onClick = ::cancel,
                            enabled = !saving,
                            modifier = Modifier.size(CardAppearance.controlSize).testTag("$tag:cancel"),
                        ) {
                            Icon(Icons.Default.Close, contentDescription = "Cancel new action")
                        }
                    },
                )
                if (failure != null) {
                    Text(
                        text = failure,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.padding(
                            start = CardAppearance.padding + CardAppearance.controlSize,
                            end = CardAppearance.padding,
                            bottom = CardAppearance.padding,
                        ).testTag("$tag:error").semantics { liveRegion = LiveRegionMode.Polite },
                    )
                }
                if (onMoreOptions != null) {
                    TextButton(
                        onClick = onMoreOptions,
                        enabled = !saving,
                        contentPadding = PaddingValues(horizontal = 0.dp),
                        modifier = Modifier.padding(
                            start = CardAppearance.padding + CardAppearance.controlSize,
                            end = CardAppearance.padding,
                        ).heightIn(min = CardAppearance.controlSize).testTag("$tag:more-options"),
                    ) { Text("More options") }
                }
            }
        }
    }
}
