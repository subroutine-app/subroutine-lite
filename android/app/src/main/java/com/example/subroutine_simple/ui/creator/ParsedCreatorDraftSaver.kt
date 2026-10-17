package com.example.subroutine_simple.ui.creator

import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import java.time.Instant
import kotlinx.serialization.Serializable

@Serializable
private data class SavedParserInput(
    val text: String,
    val selectionStart: Int,
    val selectionEnd: Int,
    val now: String,
    val reading: ParserReading,
    val request: ItemParseRequest?,
    val response: ItemParseResponse?,
    val stepText: String,
    val stepSelectionStart: Int,
    val stepSelectionEnd: Int,
    val claimed: Set<ParserHighlightKind>,
)

val ParsedCreatorDraftSaver: Saver<ParsedCreatorDraft, Any> = listSaver<ParsedCreatorDraft, Any>(
    save = { draft ->
        val input = SavedParserInput(
            text = draft.input.text,
            selectionStart = draft.input.selection.start,
            selectionEnd = draft.input.selection.end,
            now = draft.now.toString(),
            reading = draft.reading,
            request = draft.snapshot?.request,
            response = (draft.snapshot?.outcome as? ParserOutcome.Ready)?.response,
            stepText = draft.stepInput.text,
            stepSelectionStart = draft.stepInput.selection.start,
            stepSelectionEnd = draft.stepInput.selection.end,
            claimed = draft.claimed,
        )
        listOf(
            "parsed-creator-v1",
            requireNotNull(with(ItemCreatorStateSaver) { save(draft.fields) }),
            requireNotNull(with(ItemCreatorStateSaver) { save(draft.manual.applyTo(ItemCreatorState())) }),
            requireNotNull(with(ItemCreatorStateSaver) { save(draft.initial) }),
            parserJson.encodeToString(SavedParserInput.serializer(), input),
        )
    },
    restore = { values ->
        runCatching {
            require(values.size == 5 && values[0] == "parsed-creator-v1")
            val fields = requireNotNull(ItemCreatorStateSaver.restore(values[1]))
            val manual = requireNotNull(ItemCreatorStateSaver.restore(values[2]))
            val initial = requireNotNull(ItemCreatorStateSaver.restore(values[3]))
            val saved = parserJson.decodeFromString(SavedParserInput.serializer(), values[4] as String)
            fun input(text: String, start: Int, end: Int): TextFieldValue {
                require(start in 0..text.length && end in 0..text.length)
                // Composition belongs to the old IME session, not the saved draft.
                return TextFieldValue(text, TextRange(start, end))
            }

            val snapshot = saved.request?.let { request ->
                request.validate()
                saved.response?.let { response ->
                    CreatorParseSnapshot(request, ParserOutcome.Ready(response.validate(request)), verified = false)
                }
            }
            ParsedCreatorDraft(
                fields = fields,
                input = input(saved.text, saved.selectionStart, saved.selectionEnd),
                now = Instant.parse(saved.now),
                manual = ParserManualValues.of(manual),
                initial = initial,
                reading = saved.reading,
                snapshot = snapshot,
                stepInput = input(saved.stepText, saved.stepSelectionStart, saved.stepSelectionEnd),
                claimed = saved.claimed,
            )
        }.getOrNull()
    },
)
