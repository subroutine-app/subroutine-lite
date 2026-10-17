package com.example.subroutine_simple.ui.timeline

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlin.math.roundToInt

@Composable
internal fun TimelineDragPreview(
    drag: TimelineDrag,
    viewport: TimelineViewport,
    heightPx: Float,
    itemLeftPx: Float,
    contentWidthPx: Float,
    annotationLeftPx: Float,
    annotationWidthPx: Float,
    annotationHeightPx: Float,
    minimumCardHeightPx: Float,
    zoneId: ZoneId,
) {
    val density = LocalDensity.current
    val tint = MaterialTheme.colorScheme.primary
    val edit = drag.preview
    val anchorY = viewport.yAt(edit.anchor, heightPx)
    Canvas(Modifier.fillMaxSize().testTag("timeline:drag-guides")) {
        drawLine(tint, Offset(itemLeftPx, anchorY), Offset(size.width, anchorY), 2.dp.toPx())
        if (drag.phase == TimelineDragPhase.Dragging) {
            val region = minOf(72.dp.toPx(), size.height / 3f)
            val velocity = timelineAutoScrollVelocity(drag.pointerYPx, heightPx, density.density)
            drawRect(tint.copy(alpha = if (velocity < 0f) 0.14f else 0.04f), size = Size(size.width, region))
            drawRect(
                tint.copy(alpha = if (velocity > 0f) 0.14f else 0.04f),
                topLeft = Offset(0f, size.height - region), size = Size(size.width, region),
            )
        }
    }
    when (val source = drag.source) {
        is TimelineDragSource.Card -> {
            val interval = (edit as TimelineEdit.Interval).previewInterval()
            val start = viewport.preciseYAt(interval.start, heightPx)
            val bounds = TimelineCardBounds(
                start, maxOf(viewport.preciseYAt(interval.endExclusive, heightPx), start + minimumCardHeightPx),
            ).let { if (interval.isDateSpan) it.stickyCard(minimumCardHeightPx.toDouble()) else it }
            TimelineIntervalCard(
                placement = source.placement.copy(interval = interval, bounds = bounds),
                zoneId = zoneId,
                leftPx = itemLeftPx,
                contentWidthPx = contentWidthPx,
                viewportHeightPx = heightPx,
                onClick = {}, onComplete = null, completing = false, actionsEnabled = false,
            )
            if (drag.phase == TimelineDragPhase.Dragging && edit.mode != TimelineEditMode.Move) {
                val gapPx = with(density) { 8.dp.toPx() }
                val laneWidth = (contentWidthPx - gapPx * (source.placement.laneCount - 1)) / source.placement.laneCount
                val centerX = itemLeftPx + (laneWidth + gapPx) * source.placement.lane + laneWidth / 2f
                val edgeY = if (edit.mode == TimelineEditMode.Start) bounds.topPx else bounds.bottomPx
                TimelineResizeGrip(
                    Modifier.offset {
                        IntOffset(
                            (centerX - with(density) { 14.dp.toPx() }).roundToInt(),
                            (edgeY - with(density) { 2.dp.toPx() }).roundToInt(),
                        )
                    }.testTag("timeline:active-handle:${edit.mode.name.lowercase()}:${edit.key}"),
                )
            }
        }

        is TimelineDragSource.Annotation -> {
            val data = when (edit) {
                is TimelineEdit.Interval -> TimelineVisibleData(listOf(edit.previewInterval()), emptyList())
                is TimelineEdit.Signal -> TimelineVisibleData(
                    emptyList(),
                    listOf(edit.original.copy(instant = edit.instant))
                )
            }
            layoutTimelineAnnotations(data, viewport, heightPx, annotationHeightPx, 0f).singleOrNull()
                ?.let { placement ->
                    TimelineAnnotationChip(
                        placement, zoneId, annotationLeftPx, annotationWidthPx, annotationHeightPx, onClick = {},
                    )
                }
            if (drag.phase == TimelineDragPhase.Dragging && edit is TimelineEdit.Interval && edit.mode != TimelineEditMode.Move) {
                TimelineResizeGrip(
                    Modifier.offset {
                        IntOffset(
                            (annotationLeftPx + annotationWidthPx / 2f - with(density) { 14.dp.toPx() }).roundToInt(),
                            (anchorY - with(density) { 2.dp.toPx() }).roundToInt(),
                        )
                    }.testTag("timeline:active-handle:${edit.mode.name.lowercase()}:${edit.key}"),
                )
            }
        }
    }
    val label = when (edit) {
        is TimelineEdit.Interval -> edit.previewInterval().timeLabel(zoneId)
        is TimelineEdit.Signal -> edit.instant.atZone(zoneId).format(DateTimeFormatter.ofPattern("MMM d, HH:mm:ss"))
    }
    Box(Modifier.offset {
        IntOffset(
            itemLeftPx.roundToInt(), (drag.pointerYPx - with(density) { 64.dp.toPx() })
                .coerceIn(0f, maxOf(0f, heightPx - with(density) { 48.dp.toPx() })).roundToInt()
        )
    }) {
        Surface(color = MaterialTheme.colorScheme.inverseSurface, shape = MaterialTheme.shapes.small) {
            Text(
                text = if (drag.phase == TimelineDragPhase.Saving) "Saving… · $label" else label,
                style = MaterialTheme.typography.labelMedium,
                modifier = Modifier.padding(8.dp).testTag("timeline:drag-time")
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
        }
    }
}
