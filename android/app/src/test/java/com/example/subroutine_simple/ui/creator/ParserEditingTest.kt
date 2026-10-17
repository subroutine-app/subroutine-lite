package com.example.subroutine_simple.ui.creator

import androidx.compose.runtime.saveable.SaverScope
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextDecoration
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset
import org.junit.Assert.*
import org.junit.Test

class ParserEditingTest {
    @Test
    fun clauseRemovalUsesUtf16RangesAndRemapsSelectionAndComposition() {
        val text = "Café 🧪 @tomorrow after lunch"
        val start = text.indexOf("tomorrow")
        val value = TextFieldValue(text, TextRange(text.length, start), TextRange(0, 4))
        val removed = removeParserClauses(
            value, listOf(
                ParserHighlight(start - 1, start, ParserHighlightKind.Sigil),
                ParserHighlight(start, start + 8, ParserHighlightKind.When),
            ), setOf(ParserHighlightKind.When)
        )
        assertEquals("Café 🧪 after lunch", removed.text)
        assertEquals(TextRange(removed.text.length, 7), removed.selection)
        assertEquals(TextRange(0, 4), removed.composition)
    }

    @Test
    fun removalMergesOverlapsAndDoesNotGuessGrammar() {
        val text = "Work every day for 4 times"
        val result = removeParserClauses(
            TextFieldValue(text, TextRange(text.length)), listOf(
                ParserHighlight(5, text.length, ParserHighlightKind.Recurrence),
                ParserHighlight(15, text.length, ParserHighlightKind.Recurrence),
            ), setOf(ParserHighlightKind.Recurrence)
        )
        assertEquals("Work", result.text)
        assertEquals(TextRange(4), result.selection)
        val raw = TextFieldValue("Work tomorrow")
        assertEquals(raw, removeParserClauses(raw, emptyList(), setOf(ParserHighlightKind.When)))
    }

    @Test
    fun highlightingLeavesTextOffsetsAndCompositionUnderlineIntact() {
        val text = "Café 🧪 tomorrow"
        val original = AnnotatedString.Builder(text).apply {
            addStyle(SpanStyle(textDecoration = TextDecoration.Underline), 0, 4)
        }.toAnnotatedString()
        val transformation = ParserHighlightTransformation(
            listOf(ParserHighlight(8, text.length, ParserHighlightKind.When)), Color.Blue,
        )
        val result = transformation.filter(original)
        assertEquals(text, result.text.text)
        assertTrue(result.text.spanStyles.any { it.start == 0 && it.end == 4 && it.item.textDecoration == TextDecoration.Underline })
        assertTrue(result.text.spanStyles.any {
            it.start == 8 && it.end == text.length && it.item.color == Color.Blue && it.item.background == Color.Unspecified
        })
        (0..text.length).forEach {
            assertEquals(it, result.offsetMapping.originalToTransformed(it))
            assertEquals(it, result.offsetMapping.transformedToOriginal(it))
        }
    }

    @Test
    fun saverRetainsDraftOwnershipRecurrenceAndRawSelectionButRevalidates() {
        val initial = ItemCreatorState(schedule = CreatorSchedule(zoneId = ZoneId.of("America/New_York")))
        val recurrence = parserJson.parseToJsonElement(
            """{"rule":{"type":"relative","value":{"unit":"days","interval":1}},"remaining":3,"timezone":"America/New_York"}""",
        )
        val pending = ParsedCreatorDraft.initial(initial, Instant.parse("2026-10-15T12:00:00Z"))
            .withInput(TextFieldValue("Work tomorrow", TextRange(2, 4), TextRange(0, 4)))
        val response = ItemParseResponse(
            1, "Work", whenValue = "2026-11-01T06:30:00Z", recurrence = recurrence,
            warnings = listOf("Check this date"), highlights = listOf(ParserHighlight(5, 13, ParserHighlightKind.When))
        )
        val ready = pending.accept(pending.request, pending.revision, ParserOutcome.Ready(response))
            .copy(stepInput = TextFieldValue("Warm up for 5m", TextRange(4)))
        val scope = object : SaverScope {
            override fun canBeSaved(value: Any) = true
        }
        val saved = requireNotNull(with(ParsedCreatorDraftSaver) { scope.save(ready) })
        val restored = requireNotNull(ParsedCreatorDraftSaver.restore(saved))
        assertEquals(ready.input.text, restored.input.text)
        assertEquals(ready.input.selection, restored.input.selection)
        assertNull(restored.input.composition)
        assertEquals(ready.stepInput, restored.stepInput)
        assertEquals(ready.fields, restored.fields)
        assertEquals(ready.manual, restored.manual)
        assertEquals(ready.initial, restored.initial)
        assertEquals(ready.reading, restored.reading)
        assertEquals(ready.highlights, restored.highlights)
        assertEquals(recurrence, restored.fields.recurrence)
        assertEquals(ZoneOffset.ofHours(-5), restored.fields.schedule.preferredOffset)
        assertEquals("2026-11-01T06:30:00Z", restored.fields.schedule.toModelValue())
        assertTrue(restored.isPending)
        assertTrue(restored.isDirty)
        assertTrue(restored.buildRequest() is CreateItemBuildResult.Invalid)
    }

    @Test
    fun legacySaverPayloadStillRestores() {
        val legacy = listOf("Action", "Work", "notes", "2026-10-20", "09:00", "UTC", null, false, false, null)
        val restored = requireNotNull(ItemCreatorStateSaver.restore(legacy))
        assertEquals("Work", restored.title)
        assertNull(restored.recurrence)
        assertNull(restored.schedule.preferredOffset)
        assertEquals(LocalDate.of(2026, 10, 20), restored.schedule.date)
        assertEquals(LocalTime.of(9, 0), restored.schedule.time)
    }

    @Test
    fun manualGapAndUnresolvedFoldAreValidationFailures() {
        val state = ItemCreatorState(
            title = "Work", schedule = CreatorSchedule(
                LocalDate.of(2026, 3, 8), LocalTime.of(2, 30), ZoneId.of("America/New_York"),
            )
        )
        assertNotNull(state.validate().messageFor(CreatorField.Time))
        val fold =
            state.copy(schedule = state.schedule.copy(date = LocalDate.of(2026, 11, 1), time = LocalTime.of(1, 30)))
        assertNotNull(fold.validate().messageFor(CreatorField.Time))
        assertTrue(fold.buildRequest() is CreateItemBuildResult.Invalid)
        listOf(-4 to "2026-11-01T05:30:00Z", -5 to "2026-11-01T06:30:00Z").forEach { (hours, instant) ->
            val resolved = fold.copy(schedule = fold.schedule.copy(preferredOffset = ZoneOffset.ofHours(hours)))
            assertTrue(resolved.validate().isValid)
            val request = (resolved.buildRequest() as CreateItemBuildResult.Success).request as CreateItemRequest.Action
            assertEquals(instant, request.start?.toModelValue())
        }
        assertNotNull(
            fold.copy(schedule = fold.schedule.copy(preferredOffset = ZoneOffset.UTC))
                .validate().messageFor(CreatorField.Time)
        )
    }
}
