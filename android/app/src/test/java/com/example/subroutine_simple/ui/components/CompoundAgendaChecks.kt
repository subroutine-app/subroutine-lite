package com.example.subroutine_simple.ui.components

import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.ui.agenda.AgendaEntry
import com.example.subroutine_simple.ui.agenda.occurrenceKey
import com.example.subroutine_simple.ui.agenda.projectAgenda
import com.example.subroutine_simple.ui.creator.CreatorModel
import com.example.subroutine_simple.ui.creator.CreatorModelIdentity
import com.example.subroutine_simple.ui.creator.toModel
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId

// Run this entry point with the debug unit-test runtime classpath; no test framework is required.
object CompoundAgendaChecks {
    @JvmStatic
    fun main(args: Array<String>) {
        upcomingPartitionsAndOrdersWholeDays()
        futureWorkOutlivesMarkerExpansion()
        markersKeepInclusiveDateIdentityAcrossZones()
        compoundContentPreservesNotesAndSchedulePrecision()
        inlineCreationUsesOnlyDateAndTitle()
        println("Compound agenda checks passed")
    }

    private val today = LocalDate.of(2026, 10, 15)
    private val zone = ZoneId.of("America/Los_Angeles")

    private fun upcomingPartitionsAndOrdersWholeDays() {
        val missed = action("missed", "2026-10-14")
        val dateOnly = action("date", today.toString())
        val timed = action("timed", "2026-10-15T16:00:00Z")
        val finalDay = today.plusDays(366)
        val sections = projectAgenda(
            actions = listOf(
                dateOnly, timed, missed,
                action("undated", null),
                action("unqueued-undated", null).copy(queued = false),
                action("completed-undated", null).copy(completion = "2026-10-14T16:00:00Z"),
                action("backlog", today.toString()).copy(queued = false),
                action("completed", "2026-10-13").copy(completion = "2026-10-14T16:00:00Z"),
                action("horizon", finalDay.toString()),
                action("beyond", finalDay.plusDays(1).toString()),
            ),
            events = listOf(
                Event(
                    "event",
                    "event",
                    title = "Event",
                    start = "2026-10-15T15:00:00Z",
                    duration = "PT30M"
                )
            ),
            markers = emptyList(),
            signals = listOf(Signal("signal", "signal", title = "Signal", datetime = "2026-10-15T15:30:00Z")),
            today = today,
            zoneId = zone,
            now = Instant.parse("2026-10-15T20:00:00Z"),
        )
        check(sections.map { it.key } == listOf(
            "missed",
            "any-time",
            "day:$today",
            "day:$finalDay",
            "day:${finalDay.plusDays(1)}"
        ))
        check(sections.first().entries == listOf(AgendaEntry.ActionEntry(missed)))
        check(sections.single { it.key == "any-time" }.items.map { it.key } == listOf("action:undated")) {
            "Queue-any-time must remain discoverable after the action leaves backlog"
        }
        check(sections.single { it.date == today }.items.map { it.key } == listOf(
            "event:event",
            "signal:signal",
            "action:timed",
            "action:date"
        )) {
            "Timed entries, including elapsed events/signals, precede date-only actions"
        }
    }

    private fun futureWorkOutlivesMarkerExpansion() {
        val future = today.plusYears(4)
        val signalDate = future.plusDays(1)
        val markerDate = future.plusDays(2)
        val span = Marker("long", "long", title = "Long span", date = today.toString(), endDate = signalDate.toString())
        val sections = projectAgenda(
            actions = listOf(action("future-action", future.toString())),
            events = listOf(
                Event(
                    "future-event", "future-event", title = "Future event",
                    start = future.atTime(9, 0).atZone(zone).toInstant().toString(), duration = "PT1H",
                )
            ),
            markers = listOf(
                span,
                Marker("future-marker", "future-marker", title = "Future marker", date = markerDate.toString())
            ),
            signals = listOf(
                Signal(
                    "future-signal", "future-signal", title = "Future signal",
                    datetime = signalDate.atTime(10, 0).atZone(zone).toInstant().toString(),
                )
            ),
            today = today,
            zoneId = zone,
            horizonDays = 2,
        )
        check(sections.map { it.date } == listOf(
            today,
            today.plusDays(1),
            today.plusDays(2),
            future,
            signalDate,
            markerDate
        )) {
            "Only marker span expansion is bounded, never concrete future work"
        }
        check(sections.single { it.date == future }.items.map { it.key } == listOf(
            "event:future-event",
            "action:future-action"
        ))
        check(sections.single { it.date == signalDate }.items.single().key == "signal:future-signal")
        check(sections.single { it.date == future }.markers.single().marker == span)
        check(sections.single { it.date == markerDate }.markers.single().key == "marker:future-marker")
    }

    private fun markersKeepInclusiveDateIdentityAcrossZones() {
        val marker = Marker("span", "span", title = "Span", date = "2026-10-14", endDate = "2026-10-16")
        val sections = projectAgenda(
            actions = listOf(
                action("date", today.toString()),
                action("offset", "2026-10-16T01:00:00Z"),
            ),
            events = emptyList(),
            markers = listOf(marker, marker.copy(id = "invalid", endDate = "bad-date")),
            signals = emptyList(),
            today = today,
            zoneId = zone,
        )
        check(sections.map { it.date } == listOf(today, today.plusDays(1)))
        check(sections.first().items.map { it.key } == listOf("action:offset", "action:date"))
        check(sections.all { it.markers == listOf(AgendaEntry.MarkerEntry(marker)) })
        val occurrenceKeys = sections.flatMap { section -> section.entries.map(section::occurrenceKey) }
        check(occurrenceKeys.size == occurrenceKeys.toSet().size)
        check(
            projectAgenda(emptyList(), emptyList(), emptyList(), emptyList(), today, zone)
                .single().let { it.date == today && it.entries.isEmpty() },
        )
    }

    private fun compoundContentPreservesNotesAndSchedulePrecision() {
        val notes = "First paragraph\n\nSecond paragraph\n- detail"
        val action = action("card", today.toString()).copy(
            content = notes,
            duration = "PT30S",
            recurrence = JsonObject(emptyMap()),
        )
        val content = projectCompoundItem(AgendaEntry.ActionEntry(action), today, zone, today)
        check(content.timing == "Any time · 30s")
        check(content.notes == notes && content.repeating && !content.completed)
        val unscheduled = projectCompoundItem(
            AgendaEntry.ActionEntry(action.copy(start = null, duration = null, recurrence = JsonNull)),
            today, zone, today,
        )
        check(unscheduled.timing == null && !unscheduled.repeating) {
            "An undated action must not claim to be scheduled"
        }
        val completed = projectCompoundItem(
            AgendaEntry.ActionEntry(action.copy(completion = "2026-10-15T16:00:00Z")),
            today, zone, today,
        )
        check(completed.completed)
        val signal = Signal("signal", "signal", title = "Signal", datetime = "2026-10-15T16:00:00Z", content = notes)
        val signalContent = projectCompoundItem(AgendaEntry.SignalEntry(signal), today, zone, today)
        check(signalContent.kind == ItemCardKind.Signal && signalContent.notes == notes && signalContent.timing != null)
    }

    private fun inlineCreationUsesOnlyDateAndTitle() {
        check(inlineActionRequest("   ", today) == null)
        val request = requireNotNull(inlineActionRequest("  New action  ", today))
        val action = (request.toModel(CreatorModelIdentity("created")) as CreatorModel.Action).value
        check(action.title == "New action" && action.queued && action.start == today.toString())
        check(!action.pinned && action.content == null && action.duration == null && action.completion == null)
    }

    private fun action(id: String, start: String?) = Action(
        id = id,
        recurrenceId = id,
        title = id,
        queued = true,
        start = start,
    )
}
