package com.example.subroutine_simple.ui.timeline

import java.time.Instant
import kotlin.math.ceil
import kotlin.math.floor

data class TimelineCardPlacement(
    val interval: TimelineIndexedInterval,
    val bounds: TimelineCardBounds,
    val lane: Int,
    val laneCount: Int,
) {
    val topPx: Double get() = bounds.topPx
    val heightPx: Double get() = bounds.heightPx
}

data class TimelineDensityPlacement(
    val key: String,
    val bounds: TimelineCardBounds,
    val lane: Int,
    val laneCount: Int,
    val intervals: List<TimelineIndexedInterval>,
    val range: TimelineInstantRange,
) {
    val topPx: Double get() = bounds.topPx
    val heightPx: Double get() = bounds.heightPx
    val count: Int get() = intervals.size
}

data class TimelineLayoutResult(
    val timed: TimelineLayer,
    val allDay: TimelineAllDayLayer,
)

data class TimelineAllDayPlacement(
    val range: TimelineInstantRange,
    val intervals: List<TimelineIndexedInterval>,
    val bounds: TimelineCardBounds,
    val expanded: Boolean,
) {
    val key: String get() = "all-day:${range.start}"
    val topPx: Double get() = bounds.topPx
    val heightPx: Double get() = bounds.heightPx
}

data class TimelineAllDayLayer(val days: List<TimelineAllDayPlacement>) {
    internal fun topClearance(gapPx: Float): Double = days.asSequence()
        .map { it.bounds }
        .filter { it.topPx <= 0.0 && it.bottomPx > 0.0 }
        .maxOfOrNull { it.bottomPx + gapPx } ?: 0.0
}

data class TimelineLayer(
    val cards: List<TimelineCardPlacement>,
    val density: List<TimelineDensityPlacement>,
)

internal sealed interface TimelineAnnotation {
    val key: String
    val title: String
    val start: Instant
    val endExclusive: Instant

    data class Marker(val interval: TimelineIndexedInterval) : TimelineAnnotation {
        init {
            require(interval.kind == TimelineItemKind.Marker)
        }

        override val key get() = interval.key
        override val title get() = interval.title
        override val start get() = interval.start
        override val endExclusive get() = interval.endExclusive
    }

    data class Signal(val signal: TimelineIndexedSignal) : TimelineAnnotation {
        override val key get() = signal.key
        override val title get() = signal.title
        override val start get() = signal.instant
        override val endExclusive get() = signal.instant.plusNanos(1)
    }
}

internal data class TimelineAnnotationPlacement(
    val topPx: Float,
    val annotations: List<TimelineAnnotation>,
) {
    val key: String get() = annotations.first().key
    val range: TimelineInstantRange
        get() = TimelineInstantRange(
            annotations.minOf { it.start },
            annotations.maxOf { it.endExclusive },
        )
}

fun layoutTimelineIntervals(
    intervals: List<TimelineIndexedInterval>,
    viewport: TimelineViewport,
    viewportHeightPx: Float,
    maximumLanes: Int,
    minimumCardHeightPx: Float,
    cardGapPx: Float,
    expandedDays: Set<Instant> = emptySet(),
    allDayPreviewHeightPx: Float = 48f,
): TimelineLayoutResult = packTimelineIntervals(
    intervals, viewport.secondsPerPixel, maximumLanes, minimumCardHeightPx, cardGapPx,
).project(viewport, viewportHeightPx, expandedDays, allDayPreviewHeightPx)

// Packing uses the whole dataset in time coordinates, independent of scrolling and culling.
internal fun packTimelineIntervals(
    intervals: List<TimelineIndexedInterval>,
    secondsPerPixel: Double,
    maximumLanes: Int,
    minimumCardHeightPx: Float,
    cardGapPx: Float,
): TimelinePacking {
    val (allDay, timed) = intervals.filter { it.kind != TimelineItemKind.Marker }
        .partition { it.kind == TimelineItemKind.Action && it.isDateSpan }
    val days = allDay.groupBy { TimelineInstantRange(it.start, it.endExclusive) }
        .map { (range, members) -> TimelinePackedDay(range, members.sortedBy { it.key }) }
        .sortedBy { it.range.start }
    val spans = timed.map { interval ->
        val visualEnd = maxOf(
            interval.endExclusive,
            interval.start.plusSecondsPrecise(minimumCardHeightPx * secondsPerPixel),
        )
        IntervalSpan(interval, visualEnd, visualEnd.plusSecondsPrecise(cardGapPx * secondsPerPixel))
    }
        .sortedWith(compareBy({ it.interval.start }, { it.interval.key }))
        .toList()
    val groups = mutableListOf<TimelinePackedGroup>()
    var cursor = 0
    while (cursor < spans.size) {
        val laneEnds = mutableListOf<Instant>()
        val lanes = mutableListOf<Int>()
        var reach = spans[cursor].occupiedUntil
        var end = cursor
        while (end < spans.size && (end == cursor || spans[end].interval.start < reach)) {
            val span = spans[end]
            val lane = laneEnds.indexOfFirst { it <= span.interval.start }
            if (lane < 0) {
                lanes += laneEnds.size
                laneEnds += span.occupiedUntil
            } else {
                lanes += lane
                laneEnds[lane] = span.occupiedUntil
            }
            reach = maxOf(reach, span.occupiedUntil)
            end++
        }
        groups += TimelinePackedGroup(
            spans.subList(cursor, end), lanes, laneEnds.size,
            laneEnds.size > maximumLanes.coerceIn(1, 4),
        )
        cursor = end
    }
    return TimelinePacking(groups, days, minimumCardHeightPx, cardGapPx)
}

internal class TimelinePacking(
    private val groups: List<TimelinePackedGroup>,
    private val days: List<TimelinePackedDay>,
    private val minimumCardHeightPx: Float,
    private val cardGapPx: Float,
) {
    fun project(
        viewport: TimelineViewport,
        viewportHeightPx: Float,
        expandedDays: Set<Instant> = emptySet(),
        allDayPreviewHeightPx: Float = 48f,
    ): TimelineLayoutResult {
        require(allDayPreviewHeightPx.isFinite() && allDayPreviewHeightPx > 0f)
        return TimelineLayoutResult(
            timed = projectTimed(viewport, viewportHeightPx),
            allDay = projectAllDay(viewport, viewportHeightPx, expandedDays, allDayPreviewHeightPx),
        )
    }

    private fun projectTimed(viewport: TimelineViewport, viewportHeightPx: Float): TimelineLayer {
        val cards = mutableListOf<TimelineCardPlacement>()
        val density = mutableListOf<TimelineDensityPlacement>()
        groups.forEach { group ->
            if (group.binned) {
                val members = group.spans.map { it.interval }
                val bounds = TimelineCardBounds(
                    viewport.preciseYAt(members.first().start, viewportHeightPx),
                    viewport.preciseYAt(group.spans.maxOf { it.visualEnd }, viewportHeightPx),
                )
                if (bounds.intersects(viewportHeightPx)) {
                    density += densityPlacement(members, bounds)
                }
            } else {
                group.spans.zip(group.lanes).forEach { (span, lane) ->
                    val bounds = TimelineCardBounds(
                        viewport.preciseYAt(span.interval.start, viewportHeightPx),
                        viewport.preciseYAt(span.visualEnd, viewportHeightPx),
                    )
                    if (bounds.intersects(viewportHeightPx)) {
                        cards += TimelineCardPlacement(span.interval, bounds, lane, group.laneCount)
                    }
                }
            }
        }
        return TimelineLayer(cards, density)
    }

    private fun projectAllDay(
        viewport: TimelineViewport,
        viewportHeightPx: Float,
        expandedDays: Set<Instant>,
        previewHeightPx: Float,
    ): TimelineAllDayLayer = TimelineAllDayLayer(days.mapNotNull { day ->
        if (day.intervals.isEmpty()) return@mapNotNull null
        val rangeBounds = TimelineCardBounds(
            viewport.preciseYAt(day.range.start, viewportHeightPx),
            viewport.preciseYAt(day.range.endExclusive, viewportHeightPx),
        )
        if (!rangeBounds.intersects(viewportHeightPx)) return@mapNotNull null

        val availableHeight = minOf(rangeBounds.heightPx, viewportHeightPx.toDouble())
        if (availableHeight < previewHeightPx) return@mapNotNull null
        val minimumExpandedHeight = previewHeightPx.toDouble() + cardGapPx + minimumCardHeightPx
        val expanded = day.range.start in expandedDays && availableHeight >= minimumExpandedHeight
        val height = if (expanded) {
            val listHeight = day.intervals.size.toDouble() * minimumCardHeightPx +
                    (day.intervals.size - 1).toDouble() * cardGapPx
            minOf(
                previewHeightPx + cardGapPx + listHeight,
                maxOf(viewportHeightPx / 2.0, minimumExpandedHeight),
                availableHeight
            )
        } else {
            previewHeightPx.toDouble()
        }
        val bounds = rangeBounds.stickyCard(height)
        TimelineAllDayPlacement(day.range, day.intervals, bounds, expanded)
            .takeIf { bounds.intersects(viewportHeightPx) }
    })
}

private fun densityPlacement(
    intervals: List<TimelineIndexedInterval>,
    bounds: TimelineCardBounds,
): TimelineDensityPlacement = TimelineDensityPlacement(
    key = "density:${intervals.first().key}", bounds = bounds, lane = 0, laneCount = 1,
    intervals = intervals,
    range = TimelineInstantRange(intervals.first().start, intervals.maxOf { it.endExclusive }),
)

internal data class TimelinePackedDay(
    val range: TimelineInstantRange,
    val intervals: List<TimelineIndexedInterval>,
)

internal data class TimelinePackedGroup(
    val spans: List<IntervalSpan>,
    val lanes: List<Int>,
    val laneCount: Int,
    val binned: Boolean,
)

internal data class IntervalSpan(
    val interval: TimelineIndexedInterval,
    val visualEnd: Instant,
    val occupiedUntil: Instant,
)

/** Physical bounds remain independent of viewport clipping and Compose measurement limits. */
data class TimelineCardBounds(val topPx: Double, val bottomPx: Double) {
    val heightPx: Double get() = bottomPx - topPx

    fun intersects(viewportHeightPx: Float): Boolean = topPx < viewportHeightPx && bottomPx > 0.0

    internal fun stickyContentTop(contentHeightPx: Double, minimumTop: Double = 0.0): Double =
        maxOf(topPx, minimumTop).coerceAtMost(maxOf(topPx, bottomPx - contentHeightPx))

    internal fun stickyCard(heightPx: Double): TimelineCardBounds {
        val top = stickyContentTop(heightPx)
        return TimelineCardBounds(top, top + heightPx)
    }

    internal fun slice(viewportHeightPx: Float): TimelineCardSlice? {
        if (!intersects(viewportHeightPx)) return null
        val top = floor(maxOf(topPx, 0.0))
        val bottom = ceil(minOf(bottomPx, viewportHeightPx.toDouble()))
        return TimelineCardSlice(top.toFloat(), (bottom - top).toFloat(), topPx - top, bottomPx - top)
    }
}

internal data class TimelineCardSlice(
    val topPx: Float,
    val heightPx: Float,
    val physicalTopPx: Double,
    val physicalBottomPx: Double,
) {
    internal fun contour(widthPx: Float, cornerRadiusPx: Float, borderWidthPx: Float = 0f): TimelineCardContour {
        val radius = minOf(cornerRadiusPx.toDouble(), widthPx / 2.0, (physicalBottomPx - physicalTopPx) / 2.0)
        // Distant edges have the same clipped contour; keep their stroke outside the slice too.
        val clearance = maxOf(2 * radius, 2.0 * borderWidthPx)
        return TimelineCardContour(
            maxOf(physicalTopPx, -clearance).toFloat(),
            minOf(physicalBottomPx, heightPx + clearance).toFloat(),
            radius.toFloat(),
        )
    }
}

internal data class TimelineCardContour(val topPx: Float, val bottomPx: Float, val radiusPx: Float)

internal fun layoutTimelineAnnotations(
    data: TimelineVisibleData,
    viewport: TimelineViewport,
    viewportHeightPx: Float,
    touchTargetHeightPx: Float,
    gapPx: Float,
): List<TimelineAnnotationPlacement> {
    val annotations = buildList<TimelineAnnotation> {
        data.intervals.filter { it.kind == TimelineItemKind.Marker }.forEach { marker ->
            add(TimelineAnnotation.Marker(marker))
        }
        data.signals.forEach { signal -> add(TimelineAnnotation.Signal(signal)) }
    }.map { annotation ->
        val anchor = viewport.preciseYAt(annotation.start, viewportHeightPx)
        val top = when (annotation) {
            is TimelineAnnotation.Marker -> TimelineCardBounds(
                anchor,
                maxOf(viewport.preciseYAt(annotation.endExclusive, viewportHeightPx), anchor + touchTargetHeightPx),
            ).stickyContentTop(touchTargetHeightPx.toDouble())

            is TimelineAnnotation.Signal -> anchor - touchTargetHeightPx / 2
        }
        TimelineAnnotationPlacement(top.toFloat(), listOf(annotation))
    }.filter { it.topPx < viewportHeightPx && it.topPx + touchTargetHeightPx > 0f }
        .sortedWith(compareBy({ it.topPx }, { it.annotations.first().start }, { it.key }))

    val placements = mutableListOf<TimelineAnnotationPlacement>()
    annotations.forEach { placement ->
        val top = placement.topPx
        val previous = placements.lastOrNull()
        if (previous != null && top < previous.topPx + touchTargetHeightPx + gapPx) {
            placements[placements.lastIndex] = previous.copy(annotations = previous.annotations + placement.annotations)
        } else {
            placements += placement
        }
    }
    return placements
}
