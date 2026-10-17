package com.example.subroutine_simple.ui.timeline

import java.time.Duration
import java.time.Instant
import java.time.ZoneId
import java.time.temporal.ChronoUnit

enum class TimelineEditMode { Move, Start, End }

sealed interface TimelineEdit {
    val key: String
    val changed: Boolean

    data class Interval(
        val original: TimelineIndexedInterval,
        val start: Instant,
        val endExclusive: Instant,
        val mode: TimelineEditMode,
    ) : TimelineEdit {
        init {
            require(original.key.isNotBlank() && original.id.isNotBlank())
            require(Duration.between(original.start, original.endExclusive) >= Duration.ofSeconds(1))
            require(Duration.between(start, endExclusive) >= Duration.ofSeconds(1))
            require(original.kind != TimelineItemKind.Marker || original.isDateSpan)
            require(original.kind != TimelineItemKind.Event || !original.isDateSpan)
            require(original.kind != TimelineItemKind.Action || !original.isDateSpan || mode == TimelineEditMode.Move) {
                "All-day actions can only move to another date."
            }
            when (mode) {
                TimelineEditMode.Move -> if (!original.isDateSpan) {
                    require(
                        Duration.between(start, endExclusive) == Duration.between(
                            original.start,
                            original.endExclusive
                        )
                    ) {
                        "Moving a timed interval must preserve its duration."
                    }
                }

                TimelineEditMode.Start -> require(endExclusive == original.endExclusive)
                TimelineEditMode.End -> require(start == original.start)
            }
        }

        override val key: String get() = original.key
        override val changed: Boolean get() = start != original.start || endExclusive != original.endExclusive
    }

    data class Signal(
        val original: TimelineIndexedSignal,
        val instant: Instant,
    ) : TimelineEdit {
        init {
            require(original.key.isNotBlank() && original.id.isNotBlank())
        }

        override val key: String get() = original.key
        override val changed: Boolean get() = instant != original.instant
    }
}

internal fun editTimelineInterval(
    original: TimelineIndexedInterval,
    mode: TimelineEditMode,
    pointerTime: Instant,
    grabOffsetSeconds: Double,
    division: TimelineDivision,
    zoneId: ZoneId,
): TimelineEdit.Interval {
    require(grabOffsetSeconds.isFinite())
    val snapDivision = if (original.isDateSpan && division < TimelineDivision.Day) TimelineDivision.Day else division
    val target = snapTimelineInstant(pointerTime.plusSecondsPrecise(-grabOffsetSeconds), snapDivision, zoneId)
    val duration = Duration.between(original.start, original.endExclusive)
    val edit = if (original.isDateSpan) {
        val startDate = original.start.atZone(zoneId).toLocalDate()
        val endDate = original.endExclusive.atZone(zoneId).toLocalDate()
        val targetDate = target.atZone(zoneId).toLocalDate()
        when (mode) {
            TimelineEditMode.Move -> TimelineEdit.Interval(
                original,
                targetDate.atStartOfDay(zoneId).toInstant(),
                targetDate.plusDays(ChronoUnit.DAYS.between(startDate, endDate)).atStartOfDay(zoneId).toInstant(),
                mode,
            )

            TimelineEditMode.Start -> TimelineEdit.Interval(
                original,
                minOf(targetDate, endDate.minusDays(1)).atStartOfDay(zoneId).toInstant(),
                original.endExclusive,
                mode,
            )

            TimelineEditMode.End -> TimelineEdit.Interval(
                original,
                original.start,
                maxOf(targetDate, startDate.plusDays(1)).atStartOfDay(zoneId).toInstant(),
                mode,
            )
        }
    } else {
        when (mode) {
            TimelineEditMode.Move -> TimelineEdit.Interval(original, target, target.plus(duration), mode)
            TimelineEditMode.Start -> TimelineEdit.Interval(
                original, minOf(target, original.endExclusive.minusSeconds(1)), original.endExclusive, mode,
            )

            TimelineEditMode.End -> TimelineEdit.Interval(
                original, original.start, maxOf(target, original.start.plusSeconds(1)), mode,
            )
        }
    }
    return edit.also { it.validateDates(zoneId) }
}

internal fun editTimelineSignal(
    original: TimelineIndexedSignal,
    pointerTime: Instant,
    grabOffsetSeconds: Double,
    division: TimelineDivision,
    zoneId: ZoneId,
): TimelineEdit.Signal {
    require(grabOffsetSeconds.isFinite())
    return TimelineEdit.Signal(
        original,
        snapTimelineInstant(pointerTime.plusSecondsPrecise(-grabOffsetSeconds), division, zoneId),
    )
}

// The public edit contract carries instants, not a zone. Check calendar invariants wherever the zone is known.
internal fun TimelineEdit.Interval.validateDates(zoneId: ZoneId) {
    if (!original.isDateSpan) return
    for (instant in listOf(original.start, original.endExclusive, start, endExclusive)) {
        require(instant == instant.atZone(zoneId).toLocalDate().atStartOfDay(zoneId).toInstant()) {
            "Date edits must use local day boundaries."
        }
    }
    val originalDays = ChronoUnit.DAYS.between(
        original.start.atZone(zoneId).toLocalDate(), original.endExclusive.atZone(zoneId).toLocalDate(),
    )
    val editedDays =
        ChronoUnit.DAYS.between(start.atZone(zoneId).toLocalDate(), endExclusive.atZone(zoneId).toLocalDate())
    require(originalDays > 0 && editedDays > 0)
    if (mode == TimelineEditMode.Move) require(editedDays == originalDays) {
        "Moving a date interval must preserve its local day span."
    }
    if (original.kind == TimelineItemKind.Action) require(originalDays == 1L && editedDays == 1L)
}
