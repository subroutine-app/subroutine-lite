package com.example.subroutine_simple.ui.creator

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset
import kotlinx.serialization.json.JsonElement
import org.junit.Assert.*
import org.junit.Test

class ParsedCreatorDraftTest {
    private val zone = ZoneId.of("America/New_York")
    private val initial = ItemCreatorState(
        notes = "First paragraph\n\nSecond paragraph",
        schedule = CreatorSchedule(LocalDate.parse("2026-10-20"), LocalTime.of(9, 0), zone),
        duration = Duration.ofMinutes(45),
    )

    private fun draft(state: ItemCreatorState = initial) =
        ParsedCreatorDraft.initial(state, Instant.parse("2026-10-15T12:00:00Z"))

    private fun read(
        draft: ParsedCreatorDraft,
        text: String,
        title: String = text,
        whenValue: String? = null,
        seconds: Long? = null,
        recurrence: JsonElement? = null,
        clauses: List<Pair<String, ParserHighlightKind>> = emptyList(),
    ): ParsedCreatorDraft {
        val next = draft.withInput(TextFieldValue(text, TextRange(text.length)))
        val response = ItemParseResponse(
            version = 1, title = title, whenValue = whenValue, durationSeconds = seconds, recurrence = recurrence,
            warnings = emptyList(),
            highlights = clauses.map { (clause, kind) ->
                val start = text.indexOf(clause)
                ParserHighlight(start, start + clause.length, kind)
            },
        )
        return next.accept(next.request, next.revision, ParserOutcome.Ready(response))
    }

    @Test
    fun typingUsesRawTextAndSaveUsesCleanTitle() {
        val raw = "Café 🧪 tomorrow for 30m"
        val ready = read(
            draft(), raw, "Café 🧪", "2026-10-16", 1_800, clauses = listOf(
                "tomorrow" to ParserHighlightKind.When, "for 30m" to ParserHighlightKind.Duration,
            )
        )
        assertEquals(raw, ready.input.text)
        assertEquals(TextRange(raw.length), ready.input.selection)
        val request = (ready.buildRequest() as CreateItemBuildResult.Success).request as CreateItemRequest.Action
        assertEquals("Café 🧪", request.title)
        assertEquals(initial.notes, request.notes)
        assertEquals(LocalTime.of(9, 0), request.start?.time)
        assertEquals(LocalDate.parse("2026-10-16"), request.start?.date)
        assertEquals(Duration.ofMinutes(30), request.duration)
        assertEquals("2026-10-20", ready.request.defaultDate)
    }

    @Test
    fun clearingRemovesOnlyParserMetadataAndRestoresManualContext() {
        val ready = read(
            draft(), "Work tomorrow for 30m", "Work", "2026-10-16", 1_800, clauses = listOf(
                "tomorrow" to ParserHighlightKind.When, "for 30m" to ParserHighlightKind.Duration,
            )
        )
        val cleared = read(ready, "", "")
        assertEquals(initial.schedule, cleared.fields.schedule)
        assertEquals(initial.duration, cleared.fields.duration)
        assertEquals(initial.notes, cleared.fields.notes)
        assertFalse(cleared.fields.actionQueued)
        assertFalse(cleared.isDirty)
        assertTrue(cleared.buildRequest() is CreateItemBuildResult.Invalid)
    }

    @Test
    fun implicitWhenWithoutWhenHighlightOrRecurrenceIsIgnored() {
        val ready = read(draft(), "Work", whenValue = "2026-10-15T12:00:00Z")
        assertEquals(initial.schedule, ready.fields.schedule)
        assertFalse(ready.fields.actionQueued)
    }

    @Test
    fun unchangedReadingDoesNotOverwriteManualFields() {
        val ready = read(
            draft(), "Work for 30m", "Work", seconds = 1_800,
            clauses = listOf("for 30m" to ParserHighlightKind.Duration)
        )
        val manual = ready.copy(fields = ready.fields.copy(duration = Duration.ofMinutes(60)))
        val renamed = read(
            manual, "Read for 30m", "Read", seconds = 1_800,
            clauses = listOf("for 30m" to ParserHighlightKind.Duration)
        )
        assertEquals(Duration.ofMinutes(60), renamed.fields.duration)
    }

    @Test
    fun explicitEditsClaimAndRemoveClausesThenSurviveReparseAndClear() {
        val ready = read(
            draft(), "Work tomorrow for 30m", "Work", "2026-10-16", 1_800, clauses = listOf(
                "tomorrow" to ParserHighlightKind.When, "for 30m" to ParserHighlightKind.Duration,
            )
        )
        val manual = ready.editFields(ready.fields.withScheduleTime(LocalTime.of(14, 0)).stepDuration(1))
        assertEquals("Work", manual.input.text)
        assertTrue(manual.isPending)
        val reparsed =
            readUnchanged(manual, ItemParseResponse(1, "Work", warnings = emptyList(), highlights = emptyList()))
        assertEquals(LocalTime.of(14, 0), reparsed.fields.schedule.time)
        assertEquals(LocalDate.parse("2026-10-16"), reparsed.fields.schedule.date)
        assertEquals(Duration.ofMinutes(45), reparsed.fields.duration)
        val cleared = read(reparsed, "", "")
        assertEquals(reparsed.fields.schedule, cleared.fields.schedule)
        assertEquals(reparsed.fields.duration, cleared.fields.duration)
    }

    @Test
    fun staleRequestsIncludingSameTextRoundTripsCannotSubmit() {
        val first = draft().withInput(TextFieldValue("Old"))
        val next = first.withInput(TextFieldValue("New")).withInput(TextFieldValue("Old"))
        val response = ItemParseResponse(1, "Old", warnings = emptyList(), highlights = emptyList())
        assertEquals(next, next.accept(first.request, first.revision, ParserOutcome.Ready(response)))
        assertTrue(next.buildRequest() is CreateItemBuildResult.Invalid)
        val ready = readUnchanged(next, response)
        assertTrue(ready.buildRequest() is CreateItemBuildResult.Success)
        assertTrue(ready.withInput(TextFieldValue("New")).buildRequest() is CreateItemBuildResult.Invalid)
    }

    @Test
    fun parseCompletionDoesNotReplaceSelectionOrComposition() {
        val pending = draft().withInput(TextFieldValue("Café demain", TextRange(3), TextRange(0, 4)))
        val request = pending.request
        val moved = pending.withInput(pending.input.copy(selection = TextRange(1, 3)))
        val response = ItemParseResponse(
            1, "Café", whenValue = "2026-10-16", warnings = emptyList(),
            highlights = listOf(ParserHighlight(5, 11, ParserHighlightKind.When))
        )
        val ready = moved.accept(request, pending.revision, ParserOutcome.Ready(response))
        assertEquals(moved.input, ready.input)
        assertTrue(ready.buildRequest() is CreateItemBuildResult.Invalid)
        assertTrue(
            ready.withInput(ready.input.copy(composition = null)).buildRequest() is CreateItemBuildResult.Success
        )
    }

    @Test
    fun nativeFailureBlocksGrammarAsTitleAndKeepsDraft() {
        val pending = draft().withInput(TextFieldValue("Work tomorrow"))
        val failed =
            pending.accept(pending.request, pending.revision, ParserOutcome.Failed("Install the native parser ABI."))
        assertEquals("Work tomorrow", failed.input.text)
        assertEquals("Install the native parser ABI.", failed.error)
        assertTrue(failed.buildRequest() is CreateItemBuildResult.Invalid)
    }

    @Test
    fun utcInstantSurvivesTheLaterDstFoldThroughModelAndSaver() {
        val ready = read(
            draft(), "Work at 1:30", "Work", "2026-11-01T06:30:00Z",
            clauses = listOf("at 1:30" to ParserHighlightKind.When)
        )
        assertEquals(ZoneOffset.ofHours(-5), ready.fields.schedule.preferredOffset)
        assertEquals(LocalTime.of(1, 30), ready.fields.schedule.time)
        val action =
            (ready.buildRequest() as CreateItemBuildResult.Success).request.toModel(CreatorModelIdentity("id")) as CreatorModel.Action
        assertEquals("2026-11-01T06:30:00Z", action.value.start)
    }

    @Test
    fun recurrenceLimitsComeFromCoreAndSurviveEverySupportedModel() {
        val recurrence = parserJson.parseToJsonElement(
            """{"rule":{"type":"relative","value":{"unit":"days","interval":2}},"remaining":4,"end_date":null,"timezone":"America/New_York"}""",
        )
        CreatorItemType.entries.forEach { type ->
            val fields = initial.selectType(type).copy(
                routineSteps = listOf(RoutineStepDraft("step", "Warm up", Duration.ofMinutes(5))),
            )
            val ready = read(
                draft(fields), "Work every 2 days for 4 times", "Work", recurrence = recurrence,
                clauses = listOf(
                    "every 2 days" to ParserHighlightKind.Recurrence,
                    "for 4 times" to ParserHighlightKind.Recurrence
                )
            )
            val request = (ready.buildRequest() as CreateItemBuildResult.Success).request
            assertEquals(recurrence, request.recurrence)
            val modelRecurrence = when (val model = request.toModel(CreatorModelIdentity("id"))) {
                is CreatorModel.Action -> model.value.recurrence
                is CreatorModel.Event -> model.value.recurrence
                is CreatorModel.Marker -> model.value.recurrence
                is CreatorModel.Signal -> model.value.recurrence
                is CreatorModel.Routine -> model.value.recurrence
            }
            assertEquals(recurrence, modelRecurrence)
            val cleared = ready.editFields(ready.fields.copy(recurrence = null))
            assertEquals("Work", cleared.input.text)
            assertNull(
                readUnchanged(
                    cleared,
                    ItemParseResponse(1, "Work", warnings = emptyList(), highlights = emptyList())
                ).fields.recurrence
            )
        }
    }

    @Test
    fun manualScheduleSurvivesAnImplicitRecurrenceAnchor() {
        val recurrence =
            parserJson.parseToJsonElement("""{"rule":{"type":"relative","value":{"unit":"days","interval":1}}}""")
        val ready = read(
            draft(), "Work every day", "Work", "2026-10-20", recurrence = recurrence,
            clauses = listOf("every day" to ParserHighlightKind.Recurrence)
        )
        val manual = ready.editFields(ready.fields.withScheduleTime(LocalTime.of(16, 0)))
        val response = ItemParseResponse(
            1, "Work", whenValue = "2026-10-20T13:00:00Z", recurrence = recurrence,
            warnings = emptyList(), highlights = listOf(ParserHighlight(5, 14, ParserHighlightKind.Recurrence))
        )
        val reparsed = readUnchanged(manual, response)
        assertEquals(LocalTime.of(16, 0), reparsed.fields.schedule.time)
        assertEquals(reparsed.fields.schedule, readUnchanged(reparsed, response).fields.schedule)
    }

    @Test
    fun recurrenceLimitMutationAndRemovalRestoreManualRecurrence() {
        val baseline =
            parserJson.parseToJsonElement("""{"rule":{"type":"relative","value":{"unit":"weeks","interval":1}}}""")
        val until =
            parserJson.parseToJsonElement("""{"rule":{"type":"relative","value":{"unit":"days","interval":1}},"end_date":"2027-02-01"}""")
        val count =
            parserJson.parseToJsonElement("""{"rule":{"type":"relative","value":{"unit":"days","interval":1}},"remaining":3}""")
        val first = read(
            draft(initial.copy(recurrence = baseline)), "Work daily until February", "Work", recurrence = until,
            clauses = listOf("daily until February" to ParserHighlightKind.Recurrence)
        )
        val changed = read(
            first, "Work daily for 3 times", "Work", recurrence = count,
            clauses = listOf("daily for 3 times" to ParserHighlightKind.Recurrence)
        )
        assertEquals(count, changed.fields.recurrence)
        assertEquals(baseline, read(changed, "Work").fields.recurrence)
    }

    @Test
    fun markerDurationControlsInclusiveSpanAndClearingRestoresManualRange() {
        val state = initial.selectType(CreatorItemType.Marker).copy(markerEndDate = LocalDate.parse("2026-10-22"))
        val ready = read(
            draft(state), "Trip tomorrow for 2 days", "Trip", "2026-10-16", 172_800,
            clauses = listOf("tomorrow" to ParserHighlightKind.When, "for 2 days" to ParserHighlightKind.Duration)
        )
        assertEquals(LocalDate.parse("2026-10-17"), ready.fields.markerEndDate)
        val cleared = read(ready, "Trip")
        assertEquals(state.schedule, cleared.fields.schedule)
        assertEquals(state.markerEndDate, cleared.fields.markerEndDate)
    }

    @Test
    fun routineDurationCannotBeSilentlyDiscarded() {
        val state = initial.copy(
            selectedType = CreatorItemType.Routine,
            routineSteps = listOf(RoutineStepDraft(title = "Step"))
        )
        val ready = read(
            draft(state), "Routine for 30m", "Routine", seconds = 1_800,
            clauses = listOf("for 30m" to ParserHighlightKind.Duration)
        )
        assertTrue(ready.buildRequest() is CreateItemBuildResult.Invalid)
    }

    private fun readUnchanged(draft: ParsedCreatorDraft, response: ItemParseResponse) =
        draft.accept(draft.request, draft.revision, ParserOutcome.Ready(response))
}
