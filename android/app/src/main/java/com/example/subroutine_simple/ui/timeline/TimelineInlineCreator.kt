package com.example.subroutine_simple.ui.timeline

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.SaverScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.error
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import androidx.core.os.BundleCompat
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import com.example.subroutine_simple.data.models.UuidV7
import com.example.subroutine_simple.ui.components.CardAppearance
import com.example.subroutine_simple.ui.components.ItemCardPosition
import com.example.subroutine_simple.ui.components.itemCardBorder
import com.example.subroutine_simple.ui.creator.CreateItemBuildResult
import com.example.subroutine_simple.ui.creator.CreateItemRequest
import com.example.subroutine_simple.ui.creator.CreatorItemType
import com.example.subroutine_simple.ui.creator.CreatorSchedule
import com.example.subroutine_simple.ui.creator.ItemCreatorState
import com.example.subroutine_simple.ui.creator.ItemParseRequest
import com.example.subroutine_simple.ui.creator.NativeItemParser
import com.example.subroutine_simple.ui.creator.ParsedCreatorDraft
import com.example.subroutine_simple.ui.creator.ParsedCreatorDraftSaver
import com.example.subroutine_simple.ui.creator.ParserOutcome
import com.example.subroutine_simple.ui.creator.ParserTextField
import com.example.subroutine_simple.ui.creator.creatorScheduleSummary
import com.example.subroutine_simple.ui.creator.formatDuration
import com.example.subroutine_simple.ui.creator.markerRangeSummary
import com.example.subroutine_simple.ui.creator.parseInBackground
import com.example.subroutine_simple.ui.creator.recurrenceSummary
import com.example.subroutine_simple.ui.theme.SubroutineTheme
import java.time.Clock
import java.time.Instant
import java.time.ZoneId
import java.util.UUID

private data class TimelineDraft(
    val anchor: Instant,
    val creationId: String,
    val parsed: ParsedCreatorDraft,
    val attempted: Boolean = false,
) {
    fun save(): Bundle = Bundle().apply {
        putInt("version", 1)
        putString("anchor", anchor.toString())
        putString("creationId", creationId)
        putBoolean("attempted", attempted)
        val payload = requireNotNull(with(ParsedCreatorDraftSaver) { DraftSaverScope.save(parsed) })
        putSerializable("draft", ArrayList(payload as List<*>))
    }

    companion object {
        fun restore(saved: Bundle?): TimelineDraft? = saved?.let {
            runCatching {
                require(it.getInt("version") == 1)
                val id = requireNotNull(it.getString("creationId"))
                require(UUID.fromString(id).toString() == id && UUID.fromString(id) != UUID(0, 0))
                val payload = requireNotNull(BundleCompat.getSerializable(it, "draft", ArrayList::class.java))
                val parsed = requireNotNull(ParsedCreatorDraftSaver.restore(payload))
                require(parsed.fields.selectedType != CreatorItemType.Routine)
                TimelineDraft(Instant.parse(it.getString("anchor")), id, parsed, it.getBoolean("attempted"))
            }.getOrNull()
        }
    }
}

private object DraftSaverScope : SaverScope {
    override fun canBeSaved(value: Any): Boolean = when (value) {
        is String, is Boolean -> true
        is List<*> -> value.all { it == null || canBeSaved(it) }
        else -> false
    }
}

@Stable
class TimelineCreatorState internal constructor(
    val scopeKey: String,
    saved: Bundle?,
    private val persist: (Bundle?) -> Unit,
) {
    private var session by mutableStateOf(TimelineDraft.restore(saved))
    val draft: ParsedCreatorDraft? get() = session?.parsed
    val anchor: Instant? get() = session?.anchor
    val creationId: String? get() = session?.creationId
    internal val attempted: Boolean get() = session?.attempted == true

    // An occupied slot is not replaced implicitly; its input belongs to the user.
    fun begin(
        instant: Instant,
        type: CreatorItemType = CreatorItemType.Action,
        zoneId: ZoneId = ZoneId.systemDefault(),
        now: Instant = Instant.now(),
    ): Boolean {
        require(type != CreatorItemType.Routine) { "Timeline creation supports actions, events, markers and signals." }
        if (session != null) return false
        val local = instant.atZone(zoneId)
        val isMarker = type == CreatorItemType.Marker
        val fields = ItemCreatorState(
            schedule = CreatorSchedule(
                local.toLocalDate(), local.toLocalTime().takeUnless { isMarker }, zoneId,
                local.offset.takeUnless { isMarker },
            ),
            actionQueued = type == CreatorItemType.Action,
            actionPinned = type == CreatorItemType.Action,
        ).selectType(type, Clock.fixed(now, zoneId))
        update(TimelineDraft(instant, UuidV7.newString(), ParsedCreatorDraft.initial(fields, now)))
        return true
    }

    fun cancel() = update(null)

    internal fun edit(value: TextFieldValue) {
        val current = session ?: return
        update(
            current.copy(
                parsed = current.parsed.withInput(value),
                attempted = current.attempted && value.text == current.parsed.input.text,
            )
        )
    }

    internal fun accept(id: String, request: ItemParseRequest, revision: Long, outcome: ParserOutcome) {
        val current = session?.takeIf { it.creationId == id } ?: return
        update(current.copy(parsed = current.parsed.accept(request, revision, outcome)))
    }

    fun submit(onCreate: (CreateItemRequest, String, () -> Unit) -> Unit) {
        val submitted = session ?: return
        val result = submitted.parsed.buildRequest() as? CreateItemBuildResult.Success ?: return
        update(submitted.copy(attempted = true))
        onCreate(result.request, submitted.creationId) {
            val current = session
            if (current?.creationId == submitted.creationId) {
                if (current.parsed.revision == submitted.parsed.revision) {
                    cancel()
                } else {
                    // The accepted identity is spent; preserve later edits as a new creation.
                    update(current.copy(creationId = UuidV7.newString(), attempted = false))
                }
            }
        }
    }

    private fun update(next: TimelineDraft?) {
        if (session == next) return
        session = next
        persist(next?.save())
    }
}

internal class TimelineCreatorViewModel(private val savedState: SavedStateHandle) : ViewModel() {
    private val states = mutableMapOf<String, TimelineCreatorState>()

    fun state(scopeKey: String): TimelineCreatorState {
        require(scopeKey.isNotBlank()) { "Timeline drafts need a workspace/dataset scope key." }
        return states.getOrPut(scopeKey) {
            val key = "timeline-inline-draft:$scopeKey"
            TimelineCreatorState(scopeKey, savedState[key]) { value ->
                if (value == null) savedState.remove<Bundle>(key) else savedState[key] = value
            }
        }
    }
}

/** Use a stable workspace + dataset key, not the visible date or viewport position. */
@Composable
fun rememberTimelineCreatorState(scopeKey: String): TimelineCreatorState {
    val activity = checkNotNull(LocalActivity.current as? ComponentActivity) {
        "Timeline drafts require a ComponentActivity saved-state owner."
    }
    val drafts = viewModel<TimelineCreatorViewModel>(viewModelStoreOwner = activity)
    return drafts.state(scopeKey)
}

/** The host disables editing during creation and invokes the supplied completion only on success. */
@Composable
fun TimelineInlineCreator(
    state: TimelineCreatorState,
    enabled: Boolean,
    errorMessage: String?,
    onCreate: (CreateItemRequest, String, () -> Unit) -> Unit,
    modifier: Modifier = Modifier,
) {
    val focusManager = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    var hasFocus by remember(state) { mutableStateOf(false) }
    LaunchedEffect(state, state.creationId) {
        if (state.draft == null && hasFocus) {
            focusManager.clearFocus()
            keyboard?.hide()
        }
    }
    val draft = state.draft ?: return
    val id = requireNotNull(state.creationId)
    val focusRequester = remember(state, id) { FocusRequester() }
    var needsFocus by remember(state, id) { mutableStateOf(true) }
    val canEdit by rememberUpdatedState(enabled)
    val create by rememberUpdatedState(onCreate)
    val tag = "timeline-creator:${state.scopeKey}"
    val request = draft.request
    val revision = draft.revision

    LaunchedEffect(state, id, request, revision) {
        state.accept(id, request, revision, NativeItemParser.parseInBackground(request))
    }
    LaunchedEffect(state, id, enabled) {
        if (enabled && needsFocus) {
            focusRequester.requestFocus()
            keyboard?.show()
            needsFocus = false
        }
    }

    fun cancel() {
        if (!canEdit) return
        if (hasFocus) {
            focusManager.clearFocus()
            keyboard?.hide()
        }
        state.cancel()
    }

    fun submit() {
        if (canEdit) state.submit(create)
    }

    BackHandler(enabled = hasFocus) { cancel() }
    val result = draft.buildRequest()
    val validation = (result as? CreateItemBuildResult.Invalid)?.validation?.firstMessage
        .takeIf { draft.input.text.isNotBlank() && !draft.isPending && draft.input.composition == null }
    val failure = draft.error ?: errorMessage?.takeIf { state.attempted && enabled } ?: validation
    val type = draft.fields.selectedType.label
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = CardAppearance.shape(ItemCardPosition.Standalone),
        modifier = modifier.fillMaxWidth().testTag(tag)
            .itemCardBorder(ItemCardPosition.Standalone, CardAppearance.borderColor(MaterialTheme.colorScheme)),
    ) {
        Column(
            verticalArrangement = Arrangement.spacedBy(4.dp),
            modifier = Modifier.padding(horizontal = CardAppearance.padding, vertical = 8.dp)
                .onFocusChanged { hasFocus = it.hasFocus }.focusGroup()
                .onPreviewKeyEvent {
                    if (it.key == Key.Escape) {
                        if (it.type == KeyEventType.KeyDown) cancel()
                        true
                    } else false
                },
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                ParserTextField(
                    value = draft.input,
                    onValueChange = { if (canEdit) state.edit(it) },
                    highlights = draft.highlights,
                    focusRequester = focusRequester,
                    enabled = enabled,
                    placeholder = "New ${type.lowercase()}",
                    label = "$type title",
                    textStyle = MaterialTheme.typography.titleMedium,
                    onSubmit = ::submit,
                    modifier = Modifier.weight(1f).testTag("$tag:title"),
                )
                IconButton(
                    onClick = ::submit,
                    enabled = enabled && result is CreateItemBuildResult.Success,
                    modifier = Modifier.size(CardAppearance.controlSize).testTag("$tag:save"),
                ) { Icon(Icons.Default.Check, contentDescription = "Save ${type.lowercase()}") }
                IconButton(
                    onClick = ::cancel,
                    enabled = enabled,
                    modifier = Modifier.size(CardAppearance.controlSize).testTag("$tag:cancel"),
                ) { Icon(Icons.Default.Close, contentDescription = "Cancel new ${type.lowercase()}") }
            }
            Text(
                listOfNotNull(type, timelineDraftSummary(draft), "Parsing…".takeIf { draft.isPending })
                    .joinToString(" · "),
                style = MaterialTheme.typography.bodySmall,
                color = SubroutineTheme.colors.contextStrong,
                modifier = Modifier.testTag("$tag:summary"),
            )
            draft.warnings.forEach { warning ->
                Text(
                    warning,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            if (failure != null) {
                Text(
                    failure,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.testTag("$tag:error").semantics {
                        error(failure)
                        liveRegion = LiveRegionMode.Polite
                    },
                )
            }
        }
    }
}

private fun timelineDraftSummary(draft: ParsedCreatorDraft): String {
    val fields = draft.fields
    return listOfNotNull(
        if (fields.selectedType == CreatorItemType.Marker) markerRangeSummary(fields)
        else creatorScheduleSummary(fields.schedule),
        fields.duration?.let(::formatDuration).takeIf {
            fields.selectedType == CreatorItemType.Action || fields.selectedType == CreatorItemType.Event
        },
        fields.recurrence?.let(::recurrenceSummary),
    ).joinToString(" · ")
}
