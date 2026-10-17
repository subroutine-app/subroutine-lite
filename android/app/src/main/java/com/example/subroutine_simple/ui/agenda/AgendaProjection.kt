package com.example.subroutine_simple.ui.agenda

import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.data.models.isQueued
import java.time.Instant
import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.ZoneId

sealed interface AgendaEntry {
    val key: String
    val title: String

    data class ActionEntry(val action: Action) : AgendaEntry {
        override val key = "action:${action.id}"
        override val title = action.title
    }

    data class EventEntry(val event: Event) : AgendaEntry {
        override val key = "event:${event.id}"
        override val title = event.title
    }

    data class MarkerEntry(val marker: Marker) : AgendaEntry {
        override val key = "marker:${marker.id}"
        override val title = marker.title
    }

    data class SignalEntry(val signal: Signal) : AgendaEntry {
        override val key = "signal:${signal.id}"
        override val title = signal.title
    }
}

data class AgendaSection(
    val key: String,
    val date: LocalDate?,
    val entries: List<AgendaEntry>,
) {
    val markers: List<AgendaEntry.MarkerEntry>
        get() = entries.filterIsInstance<AgendaEntry.MarkerEntry>()

    val items: List<AgendaEntry>
        get() = entries.filterNot { it is AgendaEntry.MarkerEntry }
}

internal fun AgendaSection.occurrenceKey(entry: AgendaEntry): String = "$key:${entry.key}"

fun projectAgenda(
    actions: List<Action>,
    events: List<Event>,
    markers: List<Marker>,
    signals: List<Signal>,
    today: LocalDate = LocalDate.now(),
    zoneId: ZoneId = ZoneId.systemDefault(),
    // Kept for existing callers. Upcoming shows the whole local day, not just unelapsed times.
    @Suppress("UNUSED_PARAMETER") now: Instant = Instant.now(),
    horizonDays: Long = 366,
): List<AgendaSection> {
    require(horizonDays in 0..366) { "Marker expansion must be between 0 and 366 days" }
    val finalDate = today.plusDays(horizonDays)
    val missed = mutableListOf<AgendaEntry>()
    val anyTime = mutableListOf<AgendaEntry>()
    val byDate = sortedMapOf<LocalDate, MutableList<AgendaEntry>>()
    fun add(date: LocalDate, entry: AgendaEntry) {
        if (date >= today) byDate.getOrPut(date) { mutableListOf() }.add(entry)
    }

    actions.filter { it.isQueued }.forEach { action ->
        val entry = AgendaEntry.ActionEntry(action)
        if (action.start == null) {
            anyTime.add(entry)
        } else {
            val date = parseAgendaTime(action.start)?.dateIn(zoneId) ?: return@forEach
            if (date < today) missed.add(entry) else add(date, entry)
        }
    }
    events.forEach { event ->
        val date = parseAgendaTime(event.start)?.dateIn(zoneId) ?: return@forEach
        add(date, AgendaEntry.EventEntry(event))
    }
    signals.forEach { signal ->
        val date = parseAgendaTime(signal.datetime)?.dateIn(zoneId) ?: return@forEach
        add(date, AgendaEntry.SignalEntry(signal))
    }
    val futureWorkDates = byDate.keys.filter { it > finalDate }
    markers.forEach { marker ->
        val start = parseAgendaDate(marker.date) ?: return@forEach
        val end = if (marker.endDate == null) start else parseAgendaDate(marker.endDate) ?: return@forEach
        if (end < start) return@forEach
        var date = maxOf(start, today)
        val boundedEnd = minOf(end, finalDate)
        while (date <= boundedEnd) {
            add(date, AgendaEntry.MarkerEntry(marker))
            date = date.plusDays(1)
        }
        // Keep concrete markers and annotations on known future work without expanding the intervening days.
        val futureDates = futureWorkDates.filter { it in start..end }.toMutableSet()
        if (start > finalDate) futureDates.add(start)
        futureDates.forEach { add(it, AgendaEntry.MarkerEntry(marker)) }
    }

    val dayOrder = compareBy<AgendaEntry> { it.time() !is AgendaTime.Timed }
        .thenBy { (it.time() as? AgendaTime.Timed)?.instant }
        .thenBy { it.key }
    byDate.getOrPut(today) { mutableListOf() }
    return buildList {
        if (missed.isNotEmpty()) {
            add(
                AgendaSection(
                    key = "missed",
                    date = null,
                    entries = missed.sortedWith(
                        compareBy<AgendaEntry> { it.time()?.dateIn(zoneId) }.then(dayOrder),
                    ),
                ),
            )
        }
        if (anyTime.isNotEmpty()) {
            add(AgendaSection("any-time", null, anyTime.sortedBy { it.key }))
        }
        byDate.forEach { (date, entries) ->
            add(AgendaSection("day:$date", date, entries.sortedWith(dayOrder)))
        }
    }
}

internal sealed interface AgendaTime {
    data class Day(val date: LocalDate) : AgendaTime
    data class Timed(val instant: Instant) : AgendaTime

    fun dateIn(zoneId: ZoneId): LocalDate = when (this) {
        is Day -> date
        is Timed -> instant.atZone(zoneId).toLocalDate()
    }
}

internal fun parseAgendaDate(value: String): LocalDate? =
    runCatching { LocalDate.parse(value) }.getOrNull()

internal fun parseAgendaTime(value: String?): AgendaTime? {
    if (value.isNullOrBlank()) return null
    if ('T' !in value) return parseAgendaDate(value)?.let { AgendaTime.Day(it) }
    return runCatching { OffsetDateTime.parse(value).toInstant() }.getOrNull()?.let { AgendaTime.Timed(it) }
}

internal fun AgendaEntry.time(): AgendaTime? = when (this) {
    is AgendaEntry.ActionEntry -> parseAgendaTime(action.start)
    is AgendaEntry.EventEntry -> parseAgendaTime(event.start)
    is AgendaEntry.MarkerEntry -> parseAgendaTime(marker.date)
    is AgendaEntry.SignalEntry -> parseAgendaTime(signal.datetime)
}
