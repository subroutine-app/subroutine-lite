package com.example.subroutine_simple.ui.calendar

import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.data.models.parseIsoDurationSeconds
import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.OffsetDateTime
import java.time.YearMonth
import java.time.ZoneId
import java.time.temporal.ChronoUnit

private const val DefaultSummaryItemCount = 3
private const val CalendarDayCount = 6 * 7

enum class CalendarItemKind {
    Action,
    Event,
    Marker,
    Signal,
}

sealed interface CalendarItem {
    val key: String
    val title: String
    val content: String?
    val kind: CalendarItemKind
    val localTime: LocalTime?
    val instant: Instant?

    data class ActionItem(
        val action: Action,
        override val localTime: LocalTime?,
        override val instant: Instant?,
    ) : CalendarItem {
        override val key: String = stableItemKey("action", action.id)
        override val title: String = action.title
        override val content: String? = action.content
        override val kind: CalendarItemKind = CalendarItemKind.Action
    }

    data class EventItem(
        val event: Event,
        override val localTime: LocalTime?,
        override val instant: Instant?,
    ) : CalendarItem {
        override val key: String = stableItemKey("event", event.id)
        override val title: String = event.title
        override val content: String? = event.content
        override val kind: CalendarItemKind = CalendarItemKind.Event
    }

    data class MarkerItem(
        val marker: Marker,
        val startDate: LocalDate,
        val endDate: LocalDate,
        val dayInSpan: Int,
        val spanDays: Int,
    ) : CalendarItem {
        override val key: String = stableItemKey("marker", marker.id)
        override val title: String = marker.title
        override val content: String? = marker.content
        override val kind: CalendarItemKind = CalendarItemKind.Marker
        override val localTime: LocalTime? = null
        override val instant: Instant? = null
    }

    data class SignalItem(
        val signal: Signal,
        override val localTime: LocalTime?,
        override val instant: Instant?,
    ) : CalendarItem {
        override val key: String = stableItemKey("signal", signal.id)
        override val title: String = signal.title
        override val content: String? = signal.content
        override val kind: CalendarItemKind = CalendarItemKind.Signal
    }
}

data class CalendarDaySummary(
    val actionCount: Int,
    val eventCount: Int,
    val markerCount: Int,
    val signalCount: Int,
) {
    val workCount: Int
        get() = actionCount + eventCount

    val totalCount: Int
        get() = actionCount + eventCount + markerCount + signalCount

    val hasWork: Boolean
        get() = workCount > 0

    val hasMarkers: Boolean
        get() = markerCount > 0

    val hasSignals: Boolean
        get() = signalCount > 0
}

data class CalendarDay(
    val key: String,
    val date: LocalDate,
    val isInVisibleMonth: Boolean,
    val isSelected: Boolean,
    val isToday: Boolean,
    val items: List<CalendarItem>,
    val summaryItems: List<CalendarItem>,
    val overflowCount: Int,
    val summary: CalendarDaySummary,
)

data class CalendarProjection(
    val visibleMonth: YearMonth,
    val selectedDate: LocalDate,
    val today: LocalDate,
    val days: List<CalendarDay>,
    val selectedDay: CalendarDay,
) {
    init {
        require(days.size == CalendarDayCount) { "A calendar projection must contain exactly 42 days" }
        require(days.first().date.dayOfWeek == DayOfWeek.MONDAY) {
            "A calendar projection must start on Monday"
        }
    }
}

fun projectCalendar(
    visibleMonth: YearMonth,
    selectedDate: LocalDate,
    actions: List<Action>,
    events: List<Event>,
    markers: List<Marker>,
    signals: List<Signal>,
    zoneId: ZoneId = ZoneId.systemDefault(),
    today: LocalDate = LocalDate.now(zoneId),
    maxSummaryItems: Int = DefaultSummaryItemCount,
): CalendarProjection {
    val monthStart = visibleMonth.atDay(1)
    val gridStart = monthStart.minusDays((monthStart.dayOfWeek.value - DayOfWeek.MONDAY.value).toLong())
    val gridEnd = gridStart.plusDays(CalendarDayCount - 1L)
    val itemsByDate = mutableMapOf<LocalDate, MutableList<CalendarItem>>()

    fun add(date: LocalDate, item: CalendarItem) {
        if (date in gridStart..gridEnd || date == selectedDate) {
            itemsByDate.getOrPut(date) { mutableListOf() }.add(item)
        }
    }

    fun addInterval(point: CalendarPoint, duration: String?, item: CalendarItem) {
        val end = point.instant?.let { start ->
            parseIsoDurationSeconds(duration)?.takeIf { it > 0 }?.let { seconds ->
                runCatching { start.plusSeconds(seconds).minusNanos(1).atZone(zoneId).toLocalDate() }.getOrNull()
            }
        } ?: point.date
        var date = maxOf(point.date, gridStart)
        val clippedEnd = minOf(end, gridEnd)
        while (date <= clippedEnd) {
            add(date, item)
            date = date.plusDays(1)
        }
        if (selectedDate !in gridStart..gridEnd && selectedDate in point.date..end) add(selectedDate, item)
    }

    actions.forEach { action ->
        if (!action.queued || action.completion != null || action.start == null) return@forEach
        val point = parseCalendarPoint(action.start, zoneId) ?: return@forEach
        addInterval(point, action.duration, CalendarItem.ActionItem(action, point.localTime, point.instant))
    }

    events.forEach { event ->
        val point = parseCalendarPoint(event.start, zoneId) ?: return@forEach
        addInterval(point, event.duration, CalendarItem.EventItem(event, point.localTime, point.instant))
    }

    markers.forEach { marker ->
        val start = parseDateOnly(marker.date) ?: return@forEach
        val end = marker.endDate?.let(::parseDateOnly) ?: start
        if (end < start) return@forEach

        val clippedStart = maxOf(start, gridStart)
        val clippedEnd = minOf(end, gridEnd)
        if (clippedStart <= clippedEnd) {
            var date = clippedStart
            while (date <= clippedEnd) {
                add(date, marker.asCalendarItem(start, end, date))
                date = date.plusDays(1)
            }
        }
        if (selectedDate !in gridStart..gridEnd && selectedDate in start..end) {
            add(selectedDate, marker.asCalendarItem(start, end, selectedDate))
        }
    }

    signals.forEach { signal ->
        val point = parseCalendarPoint(signal.datetime, zoneId) ?: return@forEach
        add(point.date, CalendarItem.SignalItem(signal, point.localTime, point.instant))
    }

    val summaryLimit = maxSummaryItems.coerceAtLeast(0)
    fun day(date: LocalDate): CalendarDay {
        val items = itemsByDate[date].orEmpty().sortedWith(CalendarItemComparator)
        return CalendarDay(
            key = stableDayKey(date),
            date = date,
            isInVisibleMonth = YearMonth.from(date) == visibleMonth,
            isSelected = date == selectedDate,
            isToday = date == today,
            items = items,
            summaryItems = items.take(summaryLimit),
            overflowCount = (items.size - summaryLimit).coerceAtLeast(0),
            summary = CalendarDaySummary(
                actionCount = items.count { it.kind == CalendarItemKind.Action },
                eventCount = items.count { it.kind == CalendarItemKind.Event },
                markerCount = items.count { it.kind == CalendarItemKind.Marker },
                signalCount = items.count { it.kind == CalendarItemKind.Signal },
            ),
        )
    }

    val days = List(CalendarDayCount) { offset -> day(gridStart.plusDays(offset.toLong())) }
    return CalendarProjection(
        visibleMonth = visibleMonth,
        selectedDate = selectedDate,
        today = today,
        days = days,
        selectedDay = days.firstOrNull { it.date == selectedDate } ?: day(selectedDate),
    )
}

private data class CalendarPoint(
    val date: LocalDate,
    val localTime: LocalTime?,
    val instant: Instant?,
)

private fun parseCalendarPoint(value: String, zoneId: ZoneId): CalendarPoint? {
    parseDateOnly(value)?.let { return CalendarPoint(date = it, localTime = null, instant = null) }
    val instant = runCatching { Instant.parse(value) }
        .recoverCatching { OffsetDateTime.parse(value).toInstant() }
        .getOrNull()
        ?: return null
    val local = instant.atZone(zoneId)
    return CalendarPoint(date = local.toLocalDate(), localTime = local.toLocalTime(), instant = instant)
}

private fun parseDateOnly(value: String): LocalDate? {
    if ('T' in value) return null
    return runCatching { LocalDate.parse(value) }.getOrNull()
}

private fun Marker.asCalendarItem(
    startDate: LocalDate,
    endDate: LocalDate,
    date: LocalDate,
): CalendarItem.MarkerItem = CalendarItem.MarkerItem(
    marker = this,
    startDate = startDate,
    endDate = endDate,
    dayInSpan = ChronoUnit.DAYS.between(startDate, date).toInt() + 1,
    spanDays = ChronoUnit.DAYS.between(startDate, endDate).toInt() + 1,
)

private val CalendarItemComparator = compareBy<CalendarItem>(
    { item ->
        when {
            item is CalendarItem.MarkerItem -> 0
            item.localTime != null -> 1
            else -> 2
        }
    },
    { it.localTime ?: LocalTime.MIN },
    { it.instant ?: Instant.MIN },
    { it.kind.ordinal },
    { it.key },
)

private fun stableDayKey(date: LocalDate): String = "day:$date"

private fun stableItemKey(kind: String, id: String): String = "$kind:$id"
