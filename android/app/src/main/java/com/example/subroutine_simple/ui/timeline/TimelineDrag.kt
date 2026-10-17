package com.example.subroutine_simple.ui.timeline

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import java.time.Instant
import java.time.ZoneId

internal sealed interface TimelineDragSource {
    data class Card(val placement: TimelineCardPlacement) : TimelineDragSource
    data class Annotation(val placement: TimelineAnnotationPlacement) : TimelineDragSource {
        val annotation: TimelineAnnotation get() = placement.annotations.single()
    }

    fun original(mode: TimelineEditMode): TimelineEdit = when (this) {
        is Card -> placement.interval.let { TimelineEdit.Interval(it, it.start, it.endExclusive, mode) }
        is Annotation -> when (val item = annotation) {
            is TimelineAnnotation.Marker -> item.interval.let {
                TimelineEdit.Interval(
                    it,
                    it.start,
                    it.endExclusive,
                    mode
                )
            }

            is TimelineAnnotation.Signal -> TimelineEdit.Signal(item.signal, item.start)
        }
    }
}

internal enum class TimelineDragPhase { Dragging, Saving }

internal data class TimelineDrag(
    val source: TimelineDragSource,
    val division: TimelineDivision,
    val grabOffsetSeconds: Double,
    val pointerYPx: Float,
    val preview: TimelineEdit,
    val phase: TimelineDragPhase = TimelineDragPhase.Dragging,
) {
    fun at(pointerYPx: Float, viewport: TimelineViewport, heightPx: Float, zoneId: ZoneId): TimelineDrag {
        val pointerTime = viewport.timeAt(pointerYPx, heightPx)
        val edit = when (val edit = preview) {
            is TimelineEdit.Interval -> editTimelineInterval(
                edit.original, edit.mode, pointerTime, grabOffsetSeconds, division, zoneId,
            )

            is TimelineEdit.Signal -> editTimelineSignal(
                edit.original,
                pointerTime,
                grabOffsetSeconds,
                division,
                zoneId
            )
        }
        return copy(pointerYPx = pointerYPx, preview = edit)
    }
}

internal val TimelineEdit.anchor: Instant
    get() = when (this) {
        is TimelineEdit.Interval -> if (mode == TimelineEditMode.End) endExclusive else start
        is TimelineEdit.Signal -> instant
    }

internal fun TimelineEdit.Interval.previewInterval(): TimelineIndexedInterval = original.copy(
    start = start,
    endExclusive = endExclusive,
    hasExplicitDuration = original.hasExplicitDuration || mode != TimelineEditMode.Move,
)

internal fun timelineAutoScrollVelocity(pointerYPx: Float, heightPx: Float, density: Float): Float {
    if (heightPx <= 0f) return 0f
    val region = minOf(72f * density, heightPx / 3f)
    val penetration = when {
        pointerYPx < region -> (pointerYPx - region) / region
        pointerYPx > heightPx - region -> (pointerYPx - (heightPx - region)) / region
        else -> 0f
    }.coerceIn(-1f, 1f)
    return penetration * kotlin.math.abs(penetration) * 480f * density
}

// Capture lives on the viewport, not the moving item: culling or relayout cannot end a drag.
@Composable
internal fun timelineDragCapture(
    isDragging: () -> Boolean,
    onPosition: (Float) -> Unit,
    onFinish: () -> Unit,
    onCancel: () -> Unit,
): Modifier {
    val active by rememberUpdatedState(isDragging)
    val position by rememberUpdatedState(onPosition)
    val finish by rememberUpdatedState(onFinish)
    val cancel by rememberUpdatedState(onCancel)
    return Modifier.pointerInput(Unit) {
        try {
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                var captured = false
                do {
                    val event = awaitPointerEvent(PointerEventPass.Initial)
                    if (active() || captured) {
                        captured = true
                        val pointer = event.changes.firstOrNull { it.id == down.id }
                        if (event.changes.any { it.isConsumed || (it.id != down.id && it.pressed) } || pointer == null) {
                            cancel()
                        } else if (active()) {
                            if (pointer.position != pointer.previousPosition) position(pointer.position.y)
                            if (!pointer.pressed) finish()
                        }
                        event.changes.forEach { it.consume() }
                    }
                } while (event.changes.any { it.pressed })
            }
        } finally {
            cancel()
        }
    }
}
