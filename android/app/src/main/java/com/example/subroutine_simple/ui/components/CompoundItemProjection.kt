package com.example.subroutine_simple.ui.components

import com.example.subroutine_simple.data.models.isCompleted
import com.example.subroutine_simple.data.models.parseIsoDurationSeconds
import com.example.subroutine_simple.ui.agenda.AgendaEntry
import com.example.subroutine_simple.ui.agenda.AgendaTime
import com.example.subroutine_simple.ui.agenda.parseAgendaDate
import com.example.subroutine_simple.ui.agenda.parseAgendaTime
import kotlinx.serialization.json.JsonNull
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

enum class ItemCardPosition {
    Standalone, First, Middle, Last;

    companion object {
        fun fromNeighbors(previous: Boolean, next: Boolean): ItemCardPosition = when {
            previous && next -> Middle
            previous -> Last
            next -> First
            else -> Standalone
        }
    }
}

internal enum class ItemCardKind { Action, Event, Marker, Signal }

internal data class CompoundItemContent(
    val kind: ItemCardKind,
    val timing: String?,
    val notes: String?,
    val repeating: Boolean,
    val pinned: Boolean = false,
    val completed: Boolean = false,
)

internal fun projectCompoundItem(
    entry: AgendaEntry,
    dateContext: LocalDate? = null,
    zoneId: ZoneId = ZoneId.systemDefault(),
    today: LocalDate = LocalDate.now(zoneId),
): CompoundItemContent = when (entry) {
    is AgendaEntry.ActionEntry -> with(entry.action) {
        CompoundItemContent(
            kind = ItemCardKind.Action,
            timing = timingLabel(start, dateContext, zoneId, due = !queued && !isCompleted, today = today)
                .withDuration(duration),
            notes = content?.takeIf { it.isNotBlank() },
            repeating = recurrence != null && recurrence != JsonNull,
            pinned = pinned && start != null,
            completed = isCompleted,
        )
    }

    is AgendaEntry.EventEntry -> with(entry.event) {
        CompoundItemContent(
            kind = ItemCardKind.Event,
            timing = timingLabel(start, dateContext, zoneId).withDuration(duration),
            notes = content?.takeIf { it.isNotBlank() },
            repeating = recurrence != null && recurrence != JsonNull,
        )
    }

    is AgendaEntry.MarkerEntry -> with(entry.marker) {
        val start = parseAgendaDate(date)
        val end = endDate?.let(::parseAgendaDate)
        CompoundItemContent(
            kind = ItemCardKind.Marker,
            timing = when {
                start == null || (endDate != null && (end == null || end < start)) -> "Date unavailable"
                end != null && end > start -> "${start.cardDate()} – ${end.cardDate()}"
                start != dateContext -> start.cardDate()
                else -> null
            },
            notes = content?.takeIf { it.isNotBlank() },
            repeating = recurrence != null && recurrence != JsonNull,
        )
    }

    is AgendaEntry.SignalEntry -> with(entry.signal) {
        CompoundItemContent(
            kind = ItemCardKind.Signal,
            timing = timingLabel(datetime, dateContext, zoneId),
            notes = content?.takeIf { it.isNotBlank() },
            repeating = recurrence != null && recurrence != JsonNull,
        )
    }
}

private fun timingLabel(
    value: String?,
    dateContext: LocalDate?,
    zoneId: ZoneId,
    due: Boolean = false,
    today: LocalDate = LocalDate.now(zoneId),
): String? {
    val point = parseAgendaTime(value) ?: return if (value == null) null else "Time unavailable"
    val date = point.dateIn(zoneId)
    val time = (point as? AgendaTime.Timed)?.instant?.atZone(zoneId)
        ?.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
    if (due) {
        val dueDate = when (date) {
            today -> "Due today"
            today.plusDays(1) -> "Due tomorrow"
            else -> if (date < today) "Overdue · ${date.cardDate()}" else "Due ${date.cardDate()}"
        }
        return listOfNotNull(dueDate, time).joinToString(" · ")
    }
    return listOfNotNull(date.takeUnless { it == dateContext }?.cardDate(), time ?: "Any time")
        .joinToString(" · ")
}

private fun String?.withDuration(value: String?): String? {
    val duration = parseIsoDurationSeconds(value)?.takeIf { it > 0 }?.let { seconds ->
        if (seconds < 60) "${seconds}s" else formatDuration(seconds / 60)
    }
    return listOfNotNull(this, duration).takeIf { it.isNotEmpty() }?.joinToString(" · ")
}

private fun LocalDate.cardDate(): String = format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM))
