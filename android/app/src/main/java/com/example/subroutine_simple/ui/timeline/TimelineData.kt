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

private val DefaultIndexedActionDuration = Duration.ofMinutes(5)

enum class TimelineItemKind {
    Action,
    Event,
    Marker,
}

data class TimelineIndexedInterval(
    val key: String,
    val kind: TimelineItemKind,
    val id: String,
    val title: String,
    val content: String?,
    val start: Instant,
    val endExclusive: Instant,
    val isDateSpan: Boolean,
    val hasExplicitDuration: Boolean = true,
)

data class TimelineIndexedSignal(
    val key: String,
    val id: String,
    val title: String,
    val content: String?,
    val instant: Instant,
)

data class TimelineVisibleData(
    val intervals: List<TimelineIndexedInterval>,
    val signals: List<TimelineIndexedSignal>,
)

class TimelineDataIndex private constructor(
    internal val intervals: List<TimelineIndexedInterval>,
    private val prefixMaximumEnd: List<Instant>,
    private val signals: List<TimelineIndexedSignal>,
) {
    val isEmpty: Boolean
        get() = intervals.isEmpty() && signals.isEmpty()

    fun query(range: TimelineInstantRange): TimelineVisibleData {
        val intervalEnd = intervals.lowerBound { it.start >= range.endExclusive }
        val intervalStart = prefixMaximumEnd.lowerBound { it > range.start }
        val visibleIntervals = if (intervalStart >= intervalEnd) {
            emptyList()
        } else {
            intervals.subList(intervalStart, intervalEnd).filter {
                range.overlaps(it.start, it.endExclusive)
            }
        }

        val signalStart = signals.lowerBound { it.instant >= range.start }
        val signalEnd = signals.lowerBound { it.instant >= range.endExclusive }
        return TimelineVisibleData(
            intervals = visibleIntervals,
            signals = signals.subList(signalStart, signalEnd),
        )
    }

    internal fun matching(keys: Set<String>): TimelineVisibleData = TimelineVisibleData(
        intervals = intervals.filter { it.key in keys },
        signals = signals.filter { it.key in keys },
    )

    companion object {
        fun build(
            zoneId: ZoneId,
            actions: List<Action>,
            events: List<Event>,
            markers: List<Marker>,
            signals: List<Signal>,
            defaultActionDuration: Duration = DefaultIndexedActionDuration,
        ): TimelineDataIndex {
            val fallbackActionSeconds = defaultActionDuration.seconds.takeIf { it > 0 }
                ?: DefaultIndexedActionDuration.seconds
            val indexedIntervals = buildList {
                actions.forEach { action ->
                    if (!action.queued || action.completion != null) return@forEach
                    val startValue = action.start ?: return@forEach
                    val date = parseDateOnly(startValue)
                    if (date != null) {
                        add(
                            TimelineIndexedInterval(
                                key = "action:${action.id}",
                                kind = TimelineItemKind.Action,
                                id = action.id,
                                title = action.title,
                                content = action.content,
                                start = date.atStartOfDay(zoneId).toInstant(),
                                endExclusive = date.plusDays(1).atStartOfDay(zoneId).toInstant(),
                                isDateSpan = true,
                            ),
                        )
                        return@forEach
                    }

                    val start = parseTimelineTimestamp(startValue) ?: return@forEach
                    val durationSeconds = parseIsoDurationSeconds(action.duration)?.takeIf { it > 0 }
                    val end = runCatching { start.plusSeconds(durationSeconds ?: fallbackActionSeconds) }.getOrNull()
                        ?: return@forEach
                    add(
                        TimelineIndexedInterval(
                            key = "action:${action.id}",
                            kind = TimelineItemKind.Action,
                            id = action.id,
                            title = action.title,
                            content = action.content,
                            start = start,
                            endExclusive = end,
                            isDateSpan = false,
                            hasExplicitDuration = durationSeconds != null,
                        ),
                    )
                }

                events.forEach { event ->
                    val start = parseTimelineTimestamp(event.start) ?: return@forEach
                    val durationSeconds = parseIsoDurationSeconds(event.duration)
                        ?.takeIf { it > 0 }
                        ?: return@forEach
                    val end = runCatching { start.plusSeconds(durationSeconds) }.getOrNull()
                        ?: return@forEach
                    add(
                        TimelineIndexedInterval(
                            key = "event:${event.id}",
                            kind = TimelineItemKind.Event,
                            id = event.id,
                            title = event.title,
                            content = event.content,
                            start = start,
                            endExclusive = end,
                            isDateSpan = false,
                        ),
                    )
                }

                markers.forEach { marker ->
                    val startDate = runCatching { LocalDate.parse(marker.date) }.getOrNull()
                        ?: return@forEach
                    val endDate = marker.endDate
                        ?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
                        ?: startDate
                    if (endDate < startDate) return@forEach
                    add(
                        TimelineIndexedInterval(
                            key = "marker:${marker.id}",
                            kind = TimelineItemKind.Marker,
                            id = marker.id,
                            title = marker.title,
                            content = marker.content,
                            start = startDate.atStartOfDay(zoneId).toInstant(),
                            endExclusive = endDate.plusDays(1).atStartOfDay(zoneId).toInstant(),
                            isDateSpan = true,
                        ),
                    )
                }
            }.sortedWith(compareBy({ it.start }, { it.endExclusive }, { it.key }))

            var maximumEnd = Instant.MIN
            val prefixMaximumEnd = indexedIntervals.map { interval ->
                maximumEnd = maxOf(maximumEnd, interval.endExclusive)
                maximumEnd
            }

            val indexedSignals = signals.mapNotNull { signal ->
                val instant = parseTimelineTimestamp(signal.datetime) ?: return@mapNotNull null
                TimelineIndexedSignal(
                    key = "signal:${signal.id}",
                    id = signal.id,
                    title = signal.title,
                    content = signal.content,
                    instant = instant,
                )
            }.sortedWith(compareBy({ it.instant }, { it.key }))

            return TimelineDataIndex(
                intervals = indexedIntervals,
                prefixMaximumEnd = prefixMaximumEnd,
                signals = indexedSignals,
            )
        }
    }
}

private inline fun <T> List<T>.lowerBound(predicate: (T) -> Boolean): Int {
    var low = 0
    var high = size
    while (low < high) {
        val middle = (low + high) ushr 1
        if (predicate(this[middle])) high = middle else low = middle + 1
    }
    return low
}

private fun parseDateOnly(value: String): LocalDate? {
    if ('T' in value) return null
    return runCatching { LocalDate.parse(value) }.getOrNull()
}

private fun parseTimelineTimestamp(value: String): Instant? {
    if ('T' !in value) return null
    return runCatching { Instant.parse(value) }
        .recoverCatching { OffsetDateTime.parse(value).toInstant() }
        .getOrNull()
}
