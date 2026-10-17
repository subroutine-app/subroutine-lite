package com.example.subroutine_simple.ui.timeline

import java.time.Duration
import java.time.Instant
import kotlin.math.roundToLong

const val DEFAULT_TIMELINE_SECONDS_PER_PIXEL = 12.0
const val RESET_TIMELINE_SECONDS_PER_PIXEL = 16.0
const val MIN_TIMELINE_SECONDS_PER_PIXEL = 0.05
const val MAX_TIMELINE_SECONDS_PER_PIXEL = 2.0 * 24.0 * 60.0 * 60.0

data class TimelineViewport(
    val center: Instant,
    val secondsPerPixel: Double,
) {
    init {
        require(secondsPerPixel.isFinite() && secondsPerPixel > 0.0)
    }

    fun timeAt(yPx: Float, viewportHeightPx: Float): Instant =
        center.plusSecondsPrecise((yPx - viewportHeightPx / 2f) * secondsPerPixel)

    fun yAt(instant: Instant, viewportHeightPx: Float): Float =
        preciseYAt(instant, viewportHeightPx).toFloat()

    internal fun preciseYAt(instant: Instant, viewportHeightPx: Float): Double =
        viewportHeightPx / 2.0 + secondsBetween(center, instant) / secondsPerPixel

    fun panBy(screenDeltaYPx: Float): TimelineViewport = copy(
        center = center.plusSecondsPrecise(-screenDeltaYPx * secondsPerPixel),
    )

    fun zoomBy(
        scale: Float,
        anchorYPx: Float,
        viewportHeightPx: Float,
    ): TimelineViewport {
        if (!scale.isFinite() || scale <= 0f) return this
        val anchor = timeAt(anchorYPx, viewportHeightPx)
        val nextSecondsPerPixel = (secondsPerPixel / scale)
            .coerceIn(MIN_TIMELINE_SECONDS_PER_PIXEL, MAX_TIMELINE_SECONDS_PER_PIXEL)
        val anchorOffsetPx = anchorYPx - viewportHeightPx / 2f
        return TimelineViewport(
            center = anchor.plusSecondsPrecise(-anchorOffsetPx * nextSecondsPerPixel),
            secondsPerPixel = nextSecondsPerPixel,
        )
    }

    fun visibleRange(viewportHeightPx: Float, overscanPx: Float = 0f): TimelineInstantRange {
        val halfHeight = viewportHeightPx / 2f + overscanPx
        return TimelineInstantRange(
            start = center.plusSecondsPrecise(-halfHeight * secondsPerPixel),
            endExclusive = center.plusSecondsPrecise(halfHeight * secondsPerPixel),
        )
    }

    fun fit(
        range: TimelineInstantRange,
        viewportHeightPx: Float,
        fillFraction: Double = 0.8,
    ): TimelineViewport {
        val usableHeight = (viewportHeightPx * fillFraction).coerceAtLeast(120.0)
        val spanSeconds = secondsBetween(range.start, range.endExclusive).coerceAtLeast(60.0)
        return TimelineViewport(
            center = range.start.plusSecondsPrecise(spanSeconds / 2.0),
            secondsPerPixel = (spanSeconds / usableHeight)
                .coerceIn(MIN_TIMELINE_SECONDS_PER_PIXEL, MAX_TIMELINE_SECONDS_PER_PIXEL),
        )
    }
}

data class TimelineRealtimeAnchor(
    val centerOffsetMillis: Long,
) {
    fun centerAt(nowEpochMillis: Long): Instant =
        Instant.ofEpochMilli(Math.addExact(nowEpochMillis, centerOffsetMillis))

    companion object {
        fun fromCenter(nowEpochMillis: Long, center: Instant): TimelineRealtimeAnchor =
            TimelineRealtimeAnchor(Math.subtractExact(center.toEpochMilli(), nowEpochMillis))
    }
}

internal fun realtimeFrameIntervalMillis(powerSaveMode: Boolean): Long =
    if (powerSaveMode) 1_000L else 40L

data class TimelineInstantRange(
    val start: Instant,
    val endExclusive: Instant,
) {
    init {
        require(endExclusive > start)
    }

    fun overlaps(start: Instant, endExclusive: Instant): Boolean =
        start < this.endExclusive && endExclusive > this.start

    fun contains(instant: Instant): Boolean = instant >= start && instant < endExclusive
}

internal fun secondsBetween(start: Instant, end: Instant): Double {
    val duration = Duration.between(start, end)
    return duration.seconds.toDouble() + duration.nano / 1_000_000_000.0
}

internal fun Instant.plusSecondsPrecise(seconds: Double): Instant {
    if (!seconds.isFinite()) return this
    val wholeSeconds = seconds.toLong()
    return plusSeconds(wholeSeconds)
        .plusNanos(((seconds - wholeSeconds) * 1_000_000_000.0).roundToLong())
}
