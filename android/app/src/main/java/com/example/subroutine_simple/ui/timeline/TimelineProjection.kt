package com.example.subroutine_simple.ui.timeline

import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.data.models.parseIsoDurationSeconds
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.temporal.ChronoUnit

private const val MINUTES_PER_DAY = 24f * 60f
private val DefaultActionDuration: Duration = Duration.ofMinutes(5)

enum class TimelineIntervalKind(val keyPrefix: String) {
    Action("action"),
    Event("event"),
}

data class TimelineInterval(
    val key: String,
    val kind: TimelineIntervalKind,
    val id: String,
    val title: String,
    val content: String?,
    val startsAt: ZonedDateTime,
    val endsAt: ZonedDateTime,
    val startMinute: Float,
    val endMinute: Float,
    val isAllDay: Boolean,
    val lane: Int = 0,
    val laneCount: Int = 1,
)

data class TimelineMarker(
    val key: String,
    val id: String,
    val title: String,
    val content: String?,
    val startDate: LocalDate,
    val endDate: LocalDate,
    val dayInSpan: Int,
    val spanDays: Int,
)

data class TimelineSignal(
    val key: String,
    val id: String,
    val title: String,
    val content: String?,
    val occursAt: ZonedDateTime,
    val minuteOfDay: Float,
)

data class TimelineDay(
    val date: LocalDate,
    val zoneId: ZoneId,
    val intervals: List<TimelineInterval>,
    val markers: List<TimelineMarker>,
    val signals: List<TimelineSignal>,
) {
    val allDayIntervals: List<TimelineInterval>
        get() = intervals.filter(TimelineInterval::isAllDay)

    val timedIntervals: List<TimelineInterval>
        get() = intervals.filterNot(TimelineInterval::isAllDay)

    val isEmpty: Boolean
        get() = intervals.isEmpty() && markers.isEmpty() && signals.isEmpty()
}

fun projectTimelineDay(
    selectedDate: LocalDate,
    zoneId: ZoneId,
    actions: List<Action>,
    events: List<Event>,
    markers: List<Marker>,
    signals: List<Signal>,
    defaultActionDuration: Duration = DefaultActionDuration,
): TimelineDay {
    val dayStart = selectedDate.atStartOfDay(zoneId)
    val nextDayStart = selectedDate.plusDays(1).atStartOfDay(zoneId)
    val fallbackActionSeconds = defaultActionDuration.seconds.takeIf { it > 0 }
        ?: DefaultActionDuration.seconds

    val allDayActions = actions.mapNotNull { action ->
        if (!action.queued || action.completion != null) return@mapNotNull null
        val date = action.start?.let(::parseDateOnly) ?: return@mapNotNull null
        if (date != selectedDate) return@mapNotNull null
        TimelineInterval(
            key = stableKey(TimelineIntervalKind.Action.keyPrefix, action.id),
            kind = TimelineIntervalKind.Action,
            id = action.id,
            title = action.title,
            content = action.content,
            startsAt = dayStart,
            endsAt = nextDayStart,
            startMinute = 0f,
            endMinute = MINUTES_PER_DAY,
            isAllDay = true,
        )
    }

    val timedActions = actions.mapNotNull { action ->
        if (!action.queued || action.completion != null) return@mapNotNull null
        val start = action.start?.let(::parseTimestamp) ?: return@mapNotNull null
        val durationSeconds = parseIsoDurationSeconds(action.duration)
            ?.takeIf { it > 0 }
            ?: fallbackActionSeconds
        timedInterval(
            keyPrefix = TimelineIntervalKind.Action.keyPrefix,
            kind = TimelineIntervalKind.Action,
            id = action.id,
            title = action.title,
            content = action.content,
            start = start,
            durationSeconds = durationSeconds,
            dayStart = dayStart,
            nextDayStart = nextDayStart,
            zoneId = zoneId,
        )
    }

    val timedEvents = events.mapNotNull { event ->
        val start = parseTimestamp(event.start) ?: return@mapNotNull null
        val durationSeconds = parseIsoDurationSeconds(event.duration)?.takeIf { it > 0 }
            ?: return@mapNotNull null
        timedInterval(
            keyPrefix = TimelineIntervalKind.Event.keyPrefix,
            kind = TimelineIntervalKind.Event,
            id = event.id,
            title = event.title,
            content = event.content,
            start = start,
            durationSeconds = durationSeconds,
            dayStart = dayStart,
            nextDayStart = nextDayStart,
            zoneId = zoneId,
        )
    }

    val timedIntervals = assignLanes(
        (timedActions + timedEvents).sortedWith(
            compareBy<TimelineInterval>({ it.startMinute }, { it.endMinute }, { it.key }),
        ),
    )

    val projectedMarkers = markers.mapNotNull { marker ->
        val startDate = runCatching { LocalDate.parse(marker.date) }.getOrNull()
            ?: return@mapNotNull null
        val endDate = marker.endDate
            ?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
            ?: startDate
        if (endDate < startDate || selectedDate < startDate || selectedDate > endDate) {
            return@mapNotNull null
        }
        TimelineMarker(
            key = stableKey("marker", marker.id),
            id = marker.id,
            title = marker.title,
            content = marker.content,
            startDate = startDate,
            endDate = endDate,
            dayInSpan = ChronoUnit.DAYS.between(startDate, selectedDate).toInt() + 1,
            spanDays = ChronoUnit.DAYS.between(startDate, endDate).toInt() + 1,
        )
    }.sortedWith(compareBy({ it.startDate }, { it.key }))

    val projectedSignals = signals.mapNotNull { signal ->
        val instant = parseTimestamp(signal.datetime) ?: return@mapNotNull null
        val local = instant.atZone(zoneId)
        if (local.toLocalDate() != selectedDate) return@mapNotNull null
        TimelineSignal(
            key = stableKey("signal", signal.id),
            id = signal.id,
            title = signal.title,
            content = signal.content,
            occursAt = local,
            minuteOfDay = local.minuteOfDay(),
        )
    }.sortedWith(compareBy({ it.minuteOfDay }, { it.key }))

    return TimelineDay(
        date = selectedDate,
        zoneId = zoneId,
        intervals = allDayActions.sortedBy(TimelineInterval::key) + timedIntervals,
        markers = projectedMarkers,
        signals = projectedSignals,
    )
}

private fun timedInterval(
    keyPrefix: String,
    kind: TimelineIntervalKind,
    id: String,
    title: String,
    content: String?,
    start: Instant,
    durationSeconds: Long,
    dayStart: ZonedDateTime,
    nextDayStart: ZonedDateTime,
    zoneId: ZoneId,
): TimelineInterval? {
    val end = runCatching { start.plusSeconds(durationSeconds) }.getOrNull() ?: return null
    val dayStartInstant = dayStart.toInstant()
    val nextDayStartInstant = nextDayStart.toInstant()
    if (start >= nextDayStartInstant || end <= dayStartInstant) return null

    val clippedStart = if (start < dayStartInstant) dayStartInstant else start
    val clippedEnd = if (end > nextDayStartInstant) nextDayStartInstant else end
    val startMinute = if (clippedStart == dayStartInstant) 0f else clippedStart.atZone(zoneId).minuteOfDay()
    val endMinute = if (clippedEnd == nextDayStartInstant) {
        MINUTES_PER_DAY
    } else {
        clippedEnd.atZone(zoneId).minuteOfDay()
    }
    if (endMinute <= startMinute) return null

    return TimelineInterval(
        key = stableKey(keyPrefix, id),
        kind = kind,
        id = id,
        title = title,
        content = content,
        startsAt = start.atZone(zoneId),
        endsAt = end.atZone(zoneId),
        startMinute = startMinute,
        endMinute = endMinute,
        isAllDay = false,
    )
}

private fun assignLanes(intervals: List<TimelineInterval>): List<TimelineInterval> {
    if (intervals.isEmpty()) return emptyList()
    val result = mutableListOf<TimelineInterval>()
    val group = mutableListOf<TimelineInterval>()
    var groupEnd = -1f

    fun flushGroup() {
        if (group.isEmpty()) return
        val laneEnds = mutableListOf<Float>()
        val placements = group.map { interval ->
            val reusableLane = laneEnds.indexOfFirst { it <= interval.startMinute }
            val lane = if (reusableLane >= 0) reusableLane else laneEnds.size
            if (lane == laneEnds.size) laneEnds += interval.endMinute else laneEnds[lane] = interval.endMinute
            interval to lane
        }
        val laneCount = laneEnds.size
        result += placements.map { (interval, lane) ->
            interval.copy(lane = lane, laneCount = laneCount)
        }
        group.clear()
    }

    intervals.forEach { interval ->
        if (group.isNotEmpty() && interval.startMinute >= groupEnd) {
            flushGroup()
            groupEnd = -1f
        }
        group += interval
        groupEnd = maxOf(groupEnd, interval.endMinute)
    }
    flushGroup()
    return result
}

private fun parseDateOnly(value: String): LocalDate? {
    if ('T' in value) return null
    return runCatching { LocalDate.parse(value) }.getOrNull()
}

private fun parseTimestamp(value: String): Instant? {
    if ('T' !in value) return null
    return runCatching { Instant.parse(value) }
        .recoverCatching { OffsetDateTime.parse(value).toInstant() }
        .getOrNull()
}

private fun ZonedDateTime.minuteOfDay(): Float =
    hour * 60f + minute + second / 60f + nano / 60_000_000_000f

private fun stableKey(kind: String, id: String): String = "$kind:$id"
