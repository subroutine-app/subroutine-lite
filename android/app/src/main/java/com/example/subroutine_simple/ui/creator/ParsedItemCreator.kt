package com.example.subroutine_simple.ui.creator

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.ui.components.ProgressiveEditorSection
import com.example.subroutine_simple.ui.theme.SubroutineTheme
import com.example.subroutine_simple.ui.theme.subroutineButtonColors
import java.time.Duration
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

@Composable
fun ParsedItemCreator(
    draft: ParsedCreatorDraft,
    onDraftChange: (ParsedCreatorDraft) -> Unit,
    focusRequester: FocusRequester,
    onCreate: (CreateItemRequest) -> Unit,
    onCancel: () -> Unit,
    modifier: Modifier = Modifier,
    isSubmitting: Boolean = false,
    errorMessage: String? = null,
    onDirtyChange: ((Boolean) -> Unit)? = null,
    parser: ItemParser = NativeItemParser,
    initiallyExpandedDetails: Boolean = false,
    creationEnabled: Boolean = true,
) {
    val currentDraft by rememberUpdatedState(draft)
    val changeDraft by rememberUpdatedState(onDraftChange)
    val create by rememberUpdatedState(onCreate)
    val submitting by rememberUpdatedState(isSubmitting)
    val canCreate by rememberUpdatedState(creationEnabled)
    val dirtyChanged by rememberUpdatedState(onDirtyChange)
    var showValidation by rememberSaveable { mutableStateOf(false) }
    val request = draft.request
    val revision = draft.revision

    LaunchedEffect(request, revision, parser) {
        val outcome = parser.parseInBackground(request)
        val latest = currentDraft
        val next = latest.accept(request, revision, outcome)
        if (next != latest) changeDraft(next)
    }
    LaunchedEffect(draft.isDirty) { dirtyChanged?.invoke(draft.isDirty) }

    fun submit() {
        if (submitting || !canCreate) return
        when (val result = currentDraft.buildRequest()) {
            is CreateItemBuildResult.Success -> create(result.request)
            is CreateItemBuildResult.Invalid -> showValidation = true
        }
    }

    val validation = (draft.buildRequest() as? CreateItemBuildResult.Invalid)?.validation
    val canEditDetails = !isSubmitting && !draft.isPending && draft.input.composition == null
    val canSubmit =
        creationEnabled && !isSubmitting && !draft.isPending && draft.error == null && draft.input.composition == null

    fun edit(next: ItemCreatorState) {
        if (canEditDetails) onDraftChange(currentDraft.editFields(next))
    }

    Column(
        modifier = modifier.fillMaxWidth().verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        ParserTextField(
            value = draft.input,
            onValueChange = { onDraftChange(currentDraft.withInput(it)) },
            highlights = draft.highlights,
            focusRequester = focusRequester,
            enabled = !isSubmitting,
            placeholder = when (draft.fields.selectedType) {
                CreatorItemType.Action -> "What do you want to do?"
                CreatorItemType.Event -> "What is happening?"
                CreatorItemType.Marker -> "What should this time mean?"
                CreatorItemType.Signal -> "What should you notice?"
                CreatorItemType.Routine -> "Name this routine"
            },
            onSubmit = ::submit,
        )
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            CreatorTypeMenu(draft.fields.selectedType, !isSubmitting) { onDraftChange(currentDraft.selectType(it)) }
            Spacer(Modifier.weight(1f))
            TextButton(onClick = onCancel, enabled = !isSubmitting) { Text("Cancel") }
            Button(onClick = ::submit, enabled = canSubmit, colors = subroutineButtonColors()) {
                Text(if (isSubmitting) "Creating…" else "Create")
            }
        }

        Box(Modifier.fillMaxWidth().heightIn(min = 24.dp), contentAlignment = Alignment.CenterStart) {
            ParsedMetadata(draft)
        }
        draft.warnings.forEach { ParserMessage(it) }
        draft.error?.let { ParserMessage(it, isError = true) }
        errorMessage?.let { ParserMessage(it, isError = true) }
        if (showValidation && draft.error == null) {
            validation?.issues?.distinctBy { it.message }?.forEach { ParserMessage(it.message, isError = true) }
        }

        ProgressiveEditorSection(
            title = "Details",
            summary = creatorDetailsSummary(draft.fields),
            initiallyExpanded = initiallyExpandedDetails,
            enabled = !isSubmitting,
            forceExpanded = showValidation && validation?.issues?.any { it.field != CreatorField.Title } == true,
        ) {
            OutlinedTextField(
                value = draft.fields.notes,
                onValueChange = { onDraftChange(currentDraft.editFields(currentDraft.fields.copy(notes = it))) },
                label = { Text("Notes") },
                minLines = 2,
                maxLines = 5,
                enabled = !isSubmitting,
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.large,
            )
            CompositionLocalProvider(LocalCreatorControlsEnabled provides canEditDetails) {
                when (draft.fields.selectedType) {
                    CreatorItemType.Action -> ActionFields(draft.fields, showValidation, ::edit)
                    CreatorItemType.Event -> EventFields(draft.fields, showValidation, ::edit)
                    CreatorItemType.Marker -> MarkerFields(draft.fields, showValidation, ::edit)
                    CreatorItemType.Signal -> SignalFields(draft.fields, showValidation, ::edit)
                    CreatorItemType.Routine -> ParsedRoutineFields(
                        draft,
                        onDraftChange,
                        parser,
                        canEditDetails,
                        showValidation
                    )
                }
                if (draft.fields.recurrence != null) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            recurrenceSummary(draft.fields.recurrence),
                            Modifier.weight(1f),
                            style = MaterialTheme.typography.bodyMedium
                        )
                        IconButton(
                            onClick = { edit(currentDraft.fields.copy(recurrence = null)) },
                            enabled = canEditDetails,
                        ) { Icon(Icons.Default.Close, contentDescription = "Remove recurrence") }
                    }
                }
            }
        }
    }
}

@Composable
private fun CreatorTypeMenu(type: CreatorItemType, enabled: Boolean, onSelect: (CreatorItemType) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Box {
        TextButton(onClick = { expanded = true }, enabled = enabled) {
            Text(type.label)
            Icon(Icons.Default.ArrowDropDown, contentDescription = "Item type")
        }
        DropdownMenu(expanded = expanded && enabled, onDismissRequest = { expanded = false }) {
            CreatorItemType.entries.forEach { option ->
                DropdownMenuItem(text = { Text(option.label) }, onClick = { expanded = false; onSelect(option) })
            }
        }
    }
}

@Composable
private fun ParsedMetadata(draft: ParsedCreatorDraft) {
    val state = draft.fields
    val summary = listOfNotNull(
        (if (state.selectedType == CreatorItemType.Marker) markerRangeSummary(state) else creatorScheduleSummary(state.schedule))
            .takeIf { state.schedule.hasDate },
        state.duration?.let(::formatDuration).takeIf {
            state.selectedType != CreatorItemType.Signal && state.selectedType != CreatorItemType.Marker
        },
        state.recurrence?.let(::recurrenceSummary),
    ).joinToString(" · ")
    if (summary.isNotEmpty()) {
        Text(summary, style = MaterialTheme.typography.bodySmall, color = SubroutineTheme.colors.contextStrong)
    }
}

@Composable
private fun ParserMessage(message: String, isError: Boolean = false) {
    Text(
        text = message,
        style = MaterialTheme.typography.bodySmall,
        color = if (isError) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
    )
}

@Composable
private fun ParsedRoutineFields(
    draft: ParsedCreatorDraft,
    onDraftChange: (ParsedCreatorDraft) -> Unit,
    parser: ItemParser,
    enabled: Boolean,
    showValidation: Boolean,
) {
    val current by rememberUpdatedState(draft)
    val change by rememberUpdatedState(onDraftChange)
    val controlsEnabled by rememberUpdatedState(enabled)
    val focusRequester = remember { FocusRequester() }
    val request = draft.request.copy(text = draft.stepInput.text, kind = ParserItemKind.RoutineStep)
    var snapshot by remember { mutableStateOf<CreatorParseSnapshot?>(null) }
    LaunchedEffect(request, parser) {
        snapshot = CreatorParseSnapshot(request, parser.parseInBackground(request))
    }
    val outcome = snapshot?.takeIf { it.request == request }?.outcome
    val response = (outcome as? ParserOutcome.Ready)?.response
    val canAdd = enabled && response != null && response.routineStepError == null &&
            draft.stepInput.composition == null

    fun add() {
        val latest = current
        val latestRequest = latest.request.copy(text = latest.stepInput.text, kind = ParserItemKind.RoutineStep)
        val parsed = (snapshot?.takeIf { it.request == latestRequest }?.outcome as? ParserOutcome.Ready)?.response
        if (!controlsEnabled || parsed == null || parsed.routineStepError != null || latest.stepInput.composition != null) return
        change(
            latest.editFields(
                latest.fields.copy(
                    routineSteps = latest.fields.routineSteps + RoutineStepDraft(
                        title = parsed.title,
                        duration = parsed.durationSeconds?.let(Duration::ofSeconds),
                    )
                )
            ).copy(stepInput = TextFieldValue())
        )
    }
    Text("Steps", style = MaterialTheme.typography.titleMedium)
    Row(verticalAlignment = Alignment.CenterVertically) {
        ParserTextField(
            value = draft.stepInput,
            onValueChange = { change(current.copy(stepInput = it)) },
            highlights = response?.highlights.orEmpty(),
            focusRequester = focusRequester,
            modifier = Modifier.weight(1f),
            enabled = enabled,
            placeholder = "New step",
            label = "New routine step",
            textStyle = MaterialTheme.typography.bodyLarge,
            onSubmit = ::add,
        )
        IconButton(onClick = ::add, enabled = canAdd) {
            Icon(
                Icons.Default.Add,
                contentDescription = "Add routine step"
            )
        }
        if (draft.stepInput.text.isNotEmpty()) {
            IconButton(onClick = { change(current.copy(stepInput = TextFieldValue())) }, enabled = enabled) {
                Icon(Icons.Default.Close, contentDescription = "Clear unfinished routine step")
            }
        }
    }
    response?.durationSeconds?.let {
        Text(
            formatDuration(Duration.ofSeconds(it)),
            style = MaterialTheme.typography.bodySmall
        )
    }
    response?.warnings?.forEach { ParserMessage(it) }
    if (draft.stepInput.text.isNotBlank()) {
        response?.routineStepError?.let { ParserMessage(it, isError = true) }
    }
    (outcome as? ParserOutcome.Failed)?.let { ParserMessage(it.message, isError = true) }
    draft.fields.routineSteps.forEachIndexed { index, step ->
        key(step.id) {
            RoutineStepRow(
                index = index,
                count = draft.fields.routineSteps.size,
                step = step,
                onTitleChange = { change(current.editFields(current.fields.updateRoutineStepTitle(step.id, it))) },
                onMove = { change(current.editFields(current.fields.moveRoutineStep(step.id, it))) },
                onDurationStep = { change(current.editFields(current.fields.stepRoutineDuration(step.id, it))) },
                onRemove = { change(current.editFields(current.fields.removeRoutineStep(step.id))) },
            )
        }
    }
    ScheduleFields(
        schedule = draft.fields.schedule,
        onDateChange = { change(current.editFields(current.fields.withScheduleDate(it))) },
        onTimeChange = { change(current.editFields(current.fields.withScheduleTime(it))) },
        dateRequired = false,
        timeRequired = false,
        showValidation = showValidation,
        validation = draft.fields.validate(),
    )
}

internal fun recurrenceSummary(recurrence: JsonElement?): String {
    val value = recurrence as? JsonObject ?: return "Repeats"
    fun JsonObject.text(key: String): String? = (get(key) as? JsonPrimitive)?.takeUnless { it == JsonNull }?.content
    val rule = value["rule"] as? JsonObject
    val ruleValue = rule?.get("value")
    val label = when (rule?.text("type")) {
        "relative" -> (ruleValue as? JsonObject)?.let { "Every ${it.text("interval")} ${it.text("unit")}" } ?: "Repeats"
        "week" -> "Repeats weekly"
        "day" -> "Monthly on day ${(ruleValue as? JsonPrimitive)?.content}"
        "weekday" -> "Repeats monthly"
        "year" -> "Repeats yearly"
        else -> "Repeats"
    }
    return listOfNotNull(
        label,
        value.text("end_date")?.let { "until $it" },
        value.text("remaining")?.let { "$it occurrences" },
        value.text("timezone"),
    ).joinToString(" · ")
}
