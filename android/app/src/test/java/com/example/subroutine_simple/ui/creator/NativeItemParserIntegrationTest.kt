package com.example.subroutine_simple.ui.creator

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import java.time.Clock
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset

import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.*
import org.junit.Test

class NativeItemParserIntegrationTest {
    private val now = Instant.parse("2026-10-15T12:00:00Z")
    private val zone = ZoneId.of("America/New_York")
    private val title = "🧑🏽‍💻 Café 東京 e\u0301"
    private val notes = "First paragraph\n\nSecond paragraph"

    private fun initial(type: CreatorItemType = CreatorItemType.Action): ItemCreatorState =
        ItemCreatorState.initial(zone).selectType(type, Clock.fixed(now, zone)).copy(
            notes = notes,
            routineSteps = if (type == CreatorItemType.Routine) {
                listOf(RoutineStepDraft("warm-up", "Warm up", Duration.ofMinutes(5)))
            } else emptyList(),
        )

    private fun draft(text: String, state: ItemCreatorState = initial()): ParsedCreatorDraft =
        ParsedCreatorDraft.initial(state, now).withInput(TextFieldValue(text, TextRange(text.length)))

    private fun parseReady(draft: ParsedCreatorDraft): ParsedCreatorDraft {
        val response = NativeItemParser.parse(draft.request)
        assertNull("Native ${draft.request.kind}: ${response.error}", response.error)
        val parsed = draft.accept(draft.request, draft.revision, ParserOutcome.Ready(response))
        assertNull("Projection: ${parsed.error}", parsed.error)
        assertFalse(parsed.isPending)
        return parsed
    }

    private fun request(draft: ParsedCreatorDraft): CreateItemRequest = when (val result = draft.buildRequest()) {
        is CreateItemBuildResult.Success -> result.request
        is CreateItemBuildResult.Invalid -> error("Cannot create ${draft.fields.selectedType}: ${result.validation.issues}")
    }

    @Test
    fun unicodeRecurrenceAndTimingSurviveEverySupportedItemModel() {
        val recurrence = parserJson.parseToJsonElement(
            """{"rule":{"type":"relative","value":{"unit":"days","interval":2}},"end_date":null,"remaining":3,"timezone":"America/New_York"}""",
        )
        CreatorItemType.entries.forEach { type ->
            val text = when (type) {
                CreatorItemType.Action, CreatorItemType.Event ->
                    "$title 2026-10-20 at 9am every 2 days for 20m for 3 occurrences"

                CreatorItemType.Marker ->
                    "$title 2026-10-20 every 2 days for 2 days for 3 occurrences"

                CreatorItemType.Signal, CreatorItemType.Routine ->
                    "$title 2026-10-20 at 9am every 2 days for 3 occurrences"
            }
            val parsed = parseReady(draft(text, initial(type)))
            val response = requireNotNull(parsed.response)
            assertEquals(text, parsed.input.text)
            assertEquals(title, response.title)
            assertEquals(recurrence, response.recurrence)
            assertEquals(
                if (type == CreatorItemType.Marker) "2026-10-20" else "2026-10-20T13:00:00Z",
                response.whenValue
            )
            assertTrue(parsed.highlights.all { it.isValidFor(text) })
            assertEquals(
                if (type == CreatorItemType.Marker) "2026-10-20" else "2026-10-20 at 9am",
                parsed.highlights.single { it.kind == ParserHighlightKind.When }
                    .let { text.substring(it.start, it.end) },
            )
            val request = request(parsed)
            assertEquals(title, request.title)
            assertEquals(notes, request.notes)
            assertEquals(recurrence, request.recurrence)
            val modelRecurrence = when (val model = request.toModel(CreatorModelIdentity("native-item"))) {
                is CreatorModel.Action -> {
                    assertEquals("2026-10-20T13:00:00Z", model.value.start)
                    assertEquals("PT20M", model.value.duration)
                    assertTrue(model.value.queued)
                    assertFalse(model.value.pinned)
                    model.value.recurrence
                }

                is CreatorModel.Event -> {
                    assertEquals("2026-10-20T13:00:00Z", model.value.start)
                    assertEquals("PT20M", model.value.duration)
                    model.value.recurrence
                }

                is CreatorModel.Marker -> {
                    assertEquals("2026-10-20", model.value.date)
                    assertEquals("2026-10-21", model.value.endDate)
                    model.value.recurrence
                }

                is CreatorModel.Signal -> {
                    assertEquals("2026-10-20T13:00:00Z", model.value.datetime)
                    model.value.recurrence
                }

                is CreatorModel.Routine -> {
                    assertEquals("2026-10-20T13:00:00Z", model.value.target)
                    assertEquals("Warm up", model.value.steps.single().title)
                    model.value.recurrence
                }
            }
            assertEquals(recurrence, modelRecurrence)
        }
    }

    @Test
    fun allSevenNativeKindsDecodeTheSameCoreRecurrenceContract() {
        ParserItemKind.entries.forEach { kind ->
            val response = NativeItemParser.parse(
                ItemParseRequest(
                    text = "🧪 café 東京 daily for 20m for 3 occurrences",
                    kind = kind,
                    now = now.toString(),
                    timezone = zone.id,
                    defaultDate = "2030-06-05",
                )
            )
            assertNull("$kind: ${response.error}", response.error)
            assertEquals("🧪 café 東京", response.title)
            assertEquals(1_200L, response.durationSeconds)
            assertEquals(JsonPrimitive(3), (response.recurrence as JsonObject)["remaining"])
            assertEquals(JsonPrimitive(zone.id), (response.recurrence as JsonObject)["timezone"])
            if (kind == ParserItemKind.RoutineStep) assertNotNull(response.routineStepError)
        }
    }

    @Test
    fun requestedTimezoneAndDefaultDateDriveClockAndRelativeDateParsing() {
        listOf(
            Triple("America/New_York", "2030-06-05T13:00:00Z", "2026-10-16"),
            Triple("Asia/Tokyo", "2030-06-05T00:00:00Z", "2026-10-17"),
        ).forEach { (timezone, expectedTime, tomorrow) ->
            val state = initial().copy(
                schedule = CreatorSchedule(
                    date = LocalDate.parse("2030-06-05"), zoneId = ZoneId.of(timezone),
                )
            )
            val base = ParsedCreatorDraft.initial(state, Instant.parse("2026-10-15T23:30:00Z"))
            val timed = parseReady(base.withInput(TextFieldValue("$title at 9am")))
            assertEquals(expectedTime, (request(timed) as CreateItemRequest.Action).start?.toModelValue())
            val dated = parseReady(base.withInput(TextFieldValue("$title tomorrow")))
            assertEquals(tomorrow, (request(dated) as CreateItemRequest.Action).start?.toModelValue())
            assertEquals("2030-06-05", dated.request.defaultDate)
        }
    }

    @Test
    fun dateOnlyReplacesParserOwnedClockButPreservesTheManualClockAndQueue() {
        listOf(null, LocalTime.of(9, 0)).forEach { manualTime ->
            val state = initial().copy(
                schedule = CreatorSchedule(LocalDate.parse("2030-06-05"), manualTime, zone),
                actionQueued = manualTime != null,
                actionPinned = manualTime != null,
            )
            val timed = parseReady(draft("$title 2031-07-05 at 2pm", state))
            assertEquals(LocalTime.of(14, 0), timed.fields.schedule.time)
            assertTrue(timed.fields.actionQueued)
            val dated = parseReady(timed.withInput(TextFieldValue("$title 2031-07-06")))
            assertEquals("2031-07-06", dated.response?.whenValue)
            assertEquals(manualTime, dated.fields.schedule.time)
            assertEquals(state.actionQueued, dated.fields.actionQueued)
            assertEquals(state.actionPinned, dated.fields.actionPinned)
            assertEquals(
                if (manualTime == null) "2031-07-06" else "2031-07-06T13:00:00Z",
                (request(dated) as CreateItemRequest.Action).start?.toModelValue(),
            )
            val cleared = parseReady(dated.withInput(TextFieldValue(title)))
            assertNull(cleared.response?.whenValue)
            assertEquals(state.schedule, cleared.fields.schedule)
            assertEquals(state.actionQueued, cleared.fields.actionQueued)
        }
    }

    @Test
    fun nativeHighlightsSupportClauseClaimsWithoutRewritingImeText() {
        val text = "$title tomorrow for 15m"
        val pending = draft(text).withInput(TextFieldValue(text, TextRange(2, 4), TextRange(8, 12)))
        val ready = parseReady(pending)
        assertEquals(pending.input, ready.input)
        assertEquals(listOf("tomorrow", "for 15m"), ready.highlights.filter { it.kind != ParserHighlightKind.Title }
            .map { text.substring(it.start, it.end) })
        assertTrue(ready.buildRequest() is CreateItemBuildResult.Invalid)
        val committed = ready.withInput(ready.input.copy(composition = null))
        assertEquals(title, request(committed).title)
        val manual =
            committed.editFields(committed.fields.withScheduleDate(LocalDate.parse("2026-10-25")).stepDuration(1))
        assertEquals(title, manual.input.text)
        assertEquals(TextRange(2, 4), manual.input.selection)
        val reparsed = parseReady(manual)
        val request = request(reparsed) as CreateItemRequest.Action
        assertEquals("2026-10-25", request.start?.toModelValue())
        assertEquals(Duration.ofMinutes(30), request.duration)
        assertEquals(notes, request.notes)
    }

    @Test
    fun nativeRecurrenceBoundsWarnAndRemainIntactThroughEditsAndClearing() {
        val state = initial().copy(schedule = CreatorSchedule(LocalDate.parse("2030-06-05"), LocalTime.of(9, 0), zone))
        val until = parseReady(draft("$title daily for 3 occurrences until 2031-02-01", state))
        assertEquals(1, until.warnings.size)
        assertEquals(JsonPrimitive("2031-02-01"), (request(until).recurrence as JsonObject)["end_date"])
        assertEquals(JsonNull, (request(until).recurrence as JsonObject)["remaining"])
        val counted = parseReady(until.withInput(TextFieldValue("$title daily until 2031-02-01 for 4 occurrences")))
        assertEquals(1, counted.warnings.size)
        assertEquals(JsonPrimitive(4), (request(counted).recurrence as JsonObject)["remaining"])
        assertEquals(JsonNull, (request(counted).recurrence as JsonObject)["end_date"])
        assertEquals(JsonPrimitive(zone.id), (request(counted).recurrence as JsonObject)["timezone"])
        val removed = counted.editFields(counted.fields.copy(recurrence = null))
        assertEquals(title, removed.input.text)
        assertNull(request(parseReady(removed)).recurrence)
        val empty = parseReady(counted.withInput(TextFieldValue()))
        assertEquals(state.schedule, empty.fields.schedule)
        assertNull(empty.fields.recurrence)
        assertEquals(notes, empty.fields.notes)
        assertTrue(empty.buildRequest() is CreateItemBuildResult.Invalid)
    }

    @Test
    fun nativeDstGapFailsAndBothFoldInstantsRemainExact() {
        val gap = draft("$title 2026-03-08 at 2:30am")
        val gapResponse = NativeItemParser.parse(gap.request)
        assertNotNull(gapResponse.error)
        assertEquals("", gapResponse.title)
        val rejected = gap.accept(gap.request, gap.revision, ParserOutcome.Ready(gapResponse))
        assertEquals(gap.input, rejected.input)
        assertTrue(rejected.buildRequest() is CreateItemBuildResult.Invalid)
        listOf(
            Triple("2026-11-01 at 1:30am", "2026-11-01T05:30:00Z", -4),
            Triple("2026-11-01T01:30:00-05:00", "2026-11-01T06:30:00Z", -5),
        ).forEach { (clause, instant, hours) ->
            val parsed = parseReady(draft("$title $clause"))
            assertEquals(LocalTime.of(1, 30), parsed.fields.schedule.time)
            assertEquals(ZoneOffset.ofHours(hours), parsed.fields.schedule.preferredOffset)
            val model = request(parsed).toModel(CreatorModelIdentity("fold")) as CreatorModel.Action
            assertEquals(instant, model.value.start)
        }
    }

    @Test
    fun stepDurationAndTemplateClocksUseTheNativeGrammar() {
        fun parse(kind: ParserItemKind, text: String): ItemParseResponse = NativeItemParser.parse(
            ItemParseRequest(
                text = text, kind = kind, now = now.toString(), timezone = zone.id, defaultDate = "2030-06-05",
            )
        )

        val step = parse(ParserItemKind.RoutineStep, "$title for 5m")
        assertNull(step.error)
        assertNull(step.routineStepError)
        assertEquals(title, step.title)
        assertEquals(300L, step.durationSeconds)
        val routine = parseReady(
            draft(
                "Practice sequence", initial(CreatorItemType.Routine).copy(
                    routineSteps = listOf(
                        RoutineStepDraft(
                            title = step.title,
                            duration = Duration.ofSeconds(requireNotNull(step.durationSeconds))
                        ),
                    )
                )
            )
        )
        val request = request(routine) as CreateItemRequest.Routine
        assertEquals(title, request.steps.single().title)
        assertEquals("PT5M", request.steps.single().duration)
        assertNotNull(parse(ParserItemKind.RoutineStep, "$title at 9am").routineStepError)
        val actionTemplate = parse(ParserItemKind.ActionTemplate, "Café at 9am for 5m")
        assertEquals("Café", actionTemplate.title)
        assertEquals("09:00:00", actionTemplate.naiveTime)
        assertNull(actionTemplate.whenValue)
        val eventTemplate = parse(ParserItemKind.EventTemplate, "Café at 9am for 5m")
        assertEquals("Café at 9am", eventTemplate.title)
        assertNull(eventTemplate.naiveTime)
        assertNull(eventTemplate.whenValue)
    }

    @Test
    fun emptyInputHasNoNativeErrorOrImplicitMetadataForAnyKind() {
        ParserItemKind.entries.forEach { kind ->
            val response = NativeItemParser.parse(
                ItemParseRequest(
                    text = " \t\n\u2003",
                    kind = kind,
                    now = now.toString(),
                    timezone = zone.id,
                    defaultDate = "2030-06-05",
                )
            )
            assertNull(response.error)
            assertEquals("", response.title)
            assertNull(response.whenValue)
            assertNull(response.naiveTime)
            assertNull(response.durationSeconds)
            assertNull(response.recurrence)
            assertTrue(response.warnings.isEmpty())
            assertTrue(response.highlights.isEmpty())
        }
    }
}
