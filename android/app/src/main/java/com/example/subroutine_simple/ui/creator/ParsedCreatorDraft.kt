package com.example.subroutine_simple.ui.creator

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import java.time.Clock
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.temporal.ChronoUnit
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

@Serializable
data class ParserReading(
    val whenValue: String? = null,
    val durationSeconds: Long? = null,
    val recurrence: JsonElement? = null,
) {
    companion object {
        fun of(response: ItemParseResponse): ParserReading = ParserReading(
            whenValue = response.whenValue.takeIf {
                response.highlights.any { it.kind == ParserHighlightKind.When } || response.recurrence != null
            },
            durationSeconds = response.durationSeconds,
            recurrence = response.recurrence,
        )
    }
}

data class ParserManualValues(
    val schedule: CreatorSchedule,
    val duration: Duration?,
    val recurrence: JsonElement?,
    val queued: Boolean,
    val pinned: Boolean,
    val markerEndDate: LocalDate?,
) {
    companion object {
        fun of(state: ItemCreatorState) = ParserManualValues(
            state.schedule, state.duration, state.recurrence,
            state.actionQueued, state.actionPinned, state.markerEndDate,
        )
    }

    fun applyTo(state: ItemCreatorState): ItemCreatorState = state.copy(
        schedule = schedule, duration = duration, recurrence = recurrence,
        actionQueued = queued, actionPinned = pinned, markerEndDate = markerEndDate,
    )
}

data class CreatorParseSnapshot(
    val request: ItemParseRequest,
    val outcome: ParserOutcome,
    val verified: Boolean = true,
)

data class ParsedCreatorDraft(
    val fields: ItemCreatorState,
    val input: TextFieldValue,
    val now: Instant,
    val manual: ParserManualValues,
    val initial: ItemCreatorState,
    val reading: ParserReading = ParserReading(),
    val snapshot: CreatorParseSnapshot? = null,
    val revision: Long = 0,
    val stepInput: TextFieldValue = TextFieldValue(),
    val claimed: Set<ParserHighlightKind> = emptySet(),
) {
    companion object {
        fun initial(state: ItemCreatorState = ItemCreatorState.initial(), now: Instant = Instant.now()) =
            ParsedCreatorDraft(
                fields = state,
                input = TextFieldValue(state.title, TextRange(state.title.length)),
                now = now,
                manual = ParserManualValues.of(state),
                initial = state,
            )
    }

    val request: ItemParseRequest
        get() = ItemParseRequest(
            text = input.text,
            kind = fields.selectedType.parserKind,
            now = now.toString(),
            timezone = fields.schedule.zoneId.id,
            defaultDate = manual.schedule.date?.toString(),
        )

    val response: ItemParseResponse?
        get() = (snapshot?.takeIf { it.request == request }?.outcome as? ParserOutcome.Ready)?.response

    val highlights: List<ParserHighlight> get() = response?.highlights.orEmpty()
    val warnings: List<String> get() = response?.warnings.orEmpty()
    val isPending: Boolean get() = snapshot?.let { it.verified && it.request == request } != true
    val error: String?
        get() = when (val outcome = snapshot?.takeIf { it.request == request }?.outcome) {
            is ParserOutcome.Ready -> outcome.response.error
            is ParserOutcome.Failed -> outcome.message
            null -> null
        }
    val isDirty: Boolean
        get() = input.text != initial.title || stepInput.text.isNotBlank() ||
                fields.copy(title = initial.title) != initial

    fun withInput(value: TextFieldValue): ParsedCreatorDraft = copy(
        input = value,
        snapshot = snapshot.takeIf { value.text == input.text },
        claimed = claimed.takeIf { value.text == input.text }.orEmpty(),
        revision = revision + if (value.text == input.text) 0 else 1,
    )

    fun selectType(type: CreatorItemType): ParsedCreatorDraft {
        if (type == fields.selectedType) return this
        val next = manual.applyTo(fields).selectType(type, Clock.fixed(now, fields.schedule.zoneId))
        return copy(
            fields = next,
            manual = ParserManualValues.of(next),
            reading = ParserReading(),
            claimed = emptySet(),
            snapshot = null,
            revision = revision + 1,
        )
    }

    fun accept(request: ItemParseRequest, revision: Long, outcome: ParserOutcome): ParsedCreatorDraft {
        if (revision != this.revision || request != this.request) return this
        val updated = copy(snapshot = CreatorParseSnapshot(request, outcome))
        if (outcome !is ParserOutcome.Ready || outcome.response.error != null) return updated
        return try {
            updated.project(outcome.response.validate(request))
        } catch (failure: IllegalArgumentException) {
            updated.copy(
                snapshot = CreatorParseSnapshot(
                    request, ParserOutcome.Failed(
                        failure.message ?: "The parsed schedule is outside the supported range.",
                    )
                )
            )
        } catch (failure: java.time.DateTimeException) {
            updated.copy(
                snapshot = CreatorParseSnapshot(
                    request, ParserOutcome.Failed(
                        failure.message ?: "The parsed schedule is outside the supported range.",
                    )
                )
            )
        }
    }

    private fun project(response: ItemParseResponse): ParsedCreatorDraft {
        val next = ParserReading.of(response)
        val whenChanged = next.whenValue != reading.whenValue && ParserHighlightKind.When !in claimed
        val durationChanged =
            next.durationSeconds != reading.durationSeconds && ParserHighlightKind.Duration !in claimed
        var projected = fields.copy(title = response.title)
        if (whenChanged) {
            val schedule = when (val value = next.whenValue) {
                null -> manual.schedule
                else -> if ('T' in value) {
                    val local = Instant.parse(value).atZone(fields.schedule.zoneId)
                    CreatorSchedule(local.toLocalDate(), local.toLocalTime(), local.zone, local.offset)
                } else {
                    manual.schedule.copy(date = LocalDate.parse(value), preferredOffset = null)
                }
            }.let {
                if (fields.selectedType == CreatorItemType.Marker) it.copy(
                    time = null,
                    preferredOffset = null
                ) else it
            }
            val queued = manual.queued || (next.whenValue?.contains('T') == true)
            projected = projected.copy(
                schedule = schedule,
                actionQueued = queued,
                actionPinned = manual.pinned && queued && schedule.hasTime,
            )
        }
        if (durationChanged) {
            projected = projected.copy(duration = next.durationSeconds?.let(Duration::ofSeconds) ?: manual.duration)
        }
        if (fields.selectedType == CreatorItemType.Marker && (whenChanged || durationChanged)) {
            val start = projected.schedule.date
            val manualDays = manual.schedule.date?.let { date ->
                manual.markerEndDate?.let { ChronoUnit.DAYS.between(date, it) + 1 }
            } ?: 1
            val days = next.durationSeconds?.let { (it / 86_400).coerceAtLeast(1) } ?: manualDays.coerceAtLeast(1)
            projected = projected.copy(markerEndDate = start?.plusDays(days - 1) ?: manual.markerEndDate)
        }
        if (next.recurrence != reading.recurrence && ParserHighlightKind.Recurrence !in claimed) {
            projected = projected.copy(recurrence = next.recurrence ?: manual.recurrence)
        }
        return copy(fields = projected, reading = next, claimed = emptySet())
    }

    fun editFields(next: ItemCreatorState): ParsedCreatorDraft {
        val whenChanged = next.schedule != fields.schedule || next.actionQueued != fields.actionQueued ||
                next.actionPinned != fields.actionPinned
        val durationChanged = next.duration != fields.duration || next.markerEndDate != fields.markerEndDate
        val recurrenceChanged = next.recurrence != fields.recurrence
        val claims = buildSet {
            if (whenChanged) add(ParserHighlightKind.When)
            if (durationChanged) add(ParserHighlightKind.Duration)
            if (recurrenceChanged) add(ParserHighlightKind.Recurrence)
        }
        if (claims.isEmpty()) return copy(fields = next)
        val stripped = removeParserClauses(input, highlights, claims)
        return copy(
            fields = next,
            input = stripped,
            manual = manual.copy(
                schedule = if (whenChanged) next.schedule else manual.schedule,
                queued = if (whenChanged) next.actionQueued else manual.queued,
                pinned = if (whenChanged) next.actionPinned else manual.pinned,
                duration = if (durationChanged) next.duration else manual.duration,
                markerEndDate = if (whenChanged || durationChanged) next.markerEndDate else manual.markerEndDate,
                recurrence = if (recurrenceChanged) next.recurrence else manual.recurrence,
            ),
            // The next reading acknowledges the edit, including an implicit recurrence anchor.
            claimed = claimed + claims,
            snapshot = null,
            revision = revision + 1,
        )
    }

    fun buildRequest(): CreateItemBuildResult {
        val blocker = when {
            isPending -> "Wait for the current text to be parsed."
            error != null -> error
            input.composition != null -> "Finish composing the title before creating."
            fields.selectedType == CreatorItemType.Routine && stepInput.text.isNotBlank() ->
                "Add or clear the unfinished routine step."

            fields.selectedType == CreatorItemType.Routine && reading.durationSeconds != null ->
                "Set durations on routine steps, not the routine title."

            response == null -> "The item parser has not verified this title."
            else -> null
        }
        if (blocker != null) return CreateItemBuildResult.Invalid(
            CreatorValidation(
                listOf(CreatorValidationIssue(CreatorField.Title, blocker)),
            )
        )
        return fields.copy(title = requireNotNull(response).title).buildRequest()
    }
}

internal val ItemParseResponse.routineStepError: String?
    get() = when {
        error != null -> error
        whenValue != null || naiveTime != null || recurrence != null ->
            "Routine steps support a title and duration, not a separate schedule or recurrence."

        title.isBlank() -> "Enter a step title."
        else -> null
    }

fun removeParserClauses(
    value: TextFieldValue,
    highlights: List<ParserHighlight>,
    kinds: Set<ParserHighlightKind>,
): TextFieldValue {
    val ranges = highlights.filter { it.kind in kinds && it.isValidFor(value.text) }
        .map { highlight ->
            var start = highlight.start
            var end = highlight.end
            // Sigils are separate tokens in the shared grammar; remove only a reported adjacent sigil.
            highlights.firstOrNull { it.kind == ParserHighlightKind.Sigil && it.end == start && it.isValidFor(value.text) }
                ?.let { start = it.start }
            if (start > 0 && value.text[start - 1] == ' ') {
                while (start > 0 && value.text[start - 1] == ' ') start--
            } else {
                while (end < value.text.length && value.text[end] == ' ') end++
            }
            start until end
        }.sortedBy { it.first }
    val merged = mutableListOf<IntRange>()
    ranges.forEach { range ->
        val previous = merged.lastOrNull()
        if (previous != null && range.first <= previous.last + 1) {
            merged[merged.lastIndex] = previous.first..maxOf(previous.last, range.last)
        } else {
            merged += range
        }
    }
    if (merged.isEmpty()) return value
    fun mapOffset(offset: Int): Int = offset - merged.sumOf { range ->
        (offset - range.first).coerceIn(0, range.last - range.first + 1)
    }

    val text = StringBuilder(value.text).apply {
        merged.asReversed().forEach { delete(it.first, it.last + 1) }
    }.toString()
    val composition = value.composition?.takeUnless { range ->
        merged.any { it.first < range.max && it.last + 1 > range.min }
    }?.let { TextRange(mapOffset(it.start), mapOffset(it.end)) }
    return TextFieldValue(
        text,
        TextRange(mapOffset(value.selection.start), mapOffset(value.selection.end)),
        composition
    )
}
