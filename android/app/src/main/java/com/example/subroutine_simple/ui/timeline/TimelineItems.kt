package com.example.subroutine_simple.ui.timeline


import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Bolt
import androidx.compose.material.icons.outlined.CheckBoxOutlineBlank
import androidx.compose.material.icons.outlined.Flag
import androidx.compose.material.icons.outlined.MoreHoriz
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import java.time.ZoneId
import kotlin.math.roundToInt

@Composable
internal fun TimelineIntervalCard(
    placement: TimelineCardPlacement,
    zoneId: ZoneId,
    leftPx: Float,
    contentWidthPx: Float,
    viewportHeightPx: Float,
    onClick: () -> Unit,
    onComplete: (() -> Unit)?,
    completing: Boolean,
    actionsEnabled: Boolean,
    stickyContextTopPx: Double = 0.0,
    modifier: Modifier = Modifier,
    onMoveStart: ((localPosition: Offset) -> Unit)? = null,
    moveEarlier: (() -> Unit)? = null,
    moveLater: (() -> Unit)? = null,
) {
    val density = LocalDensity.current
    val gapPx = with(density) { 8.dp.toPx() }
    val laneWidthPx = (contentWidthPx - gapPx * (placement.laneCount - 1)) / placement.laneCount
    val interval = placement.interval
    val timeLabel = interval.timeLabel(zoneId)
    TimelineCardSurface(
        bounds = placement.bounds,
        viewportHeightPx = viewportHeightPx,
        leftPx = leftPx + (laneWidthPx + gapPx) * placement.lane,
        widthPx = laneWidthPx,
        stickyContextTopPx = stickyContextTopPx,
        modifier = modifier
            .timelineBodyDrag(interval.key, onMoveStart, moveEarlier, moveLater)
            .testTag(TimelineTestTags.interval(interval.key))
            .semantics { contentDescription = "${interval.kind}: ${interval.title}, $timeLabel" }
            .clickable(onClickLabel = "Edit ${interval.kind.name.lowercase()}", role = Role.Button, onClick = onClick),
    ) {
        Column(modifier = Modifier.padding(vertical = 8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (interval.kind == TimelineItemKind.Action && onComplete != null) {
                    TimelineCompletionButton(
                        title = interval.title,
                        testTag = TimelineTestTags.complete(interval.key),
                        onComplete = onComplete,
                        completing = completing,
                        enabled = actionsEnabled,
                    )
                } else {
                    Icon(
                        timelineTypeIcon(interval.kind),
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(start = 8.dp).size(18.dp),
                    )
                }
                Text(
                    text = interval.title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f).padding(horizontal = 8.dp).clearAndSetSemantics {},
                )
            }
            if (placement.heightPx >= with(density) { 104.dp.toPx() * density.fontScale }) {
                Text(
                    text = timeLabel,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(horizontal = 8.dp).clearAndSetSemantics {},
                )
            }

        }
    }
}

@Composable
internal fun TimelineDensityCard(
    placement: TimelineDensityPlacement,
    leftPx: Float,
    contentWidthPx: Float,
    viewportHeightPx: Float,
    stickyContextTopPx: Double = 0.0,
    onClick: () -> Unit,
) {
    val allDay = placement.intervals.all { it.kind == TimelineItemKind.Action && it.isDateSpan }
    val title = if (allDay) "${placement.count} all-day actions" else "${placement.count} items"
    TimelineCardSurface(
        bounds = placement.bounds,
        viewportHeightPx = viewportHeightPx,
        leftPx = leftPx,
        widthPx = contentWidthPx,
        stickyContextTopPx = stickyContextTopPx,
        modifier = Modifier
            .testTag(TimelineTestTags.density(placement.key))
            .semantics {
                contentDescription = if (allDay) title else "${placement.count} overlapping actions and events"
            }
            .clickable(onClickLabel = "Show all items", role = Role.Button, onClick = onClick),
    ) {
        Column(
            modifier = Modifier.padding(8.dp).clearAndSetSemantics {},
            verticalArrangement = Arrangement.Center,
        ) {
            Text(title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                placement.intervals.joinToString { it.title },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
internal fun TimelineDetailsRow(
    title: String,
    description: String,
    testTag: String,
    onClick: () -> Unit,
    onComplete: (() -> Unit)? = null,
    completing: Boolean = false,
    actionsEnabled: Boolean = true,
) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (onComplete != null) {
            TimelineCompletionButton(title, "$testTag:complete", onComplete, completing, actionsEnabled)
        }
        Column(
            modifier = Modifier.weight(1f)
                .heightIn(min = 48.dp)
                .testTag(testTag)
                .clickable(onClickLabel = "Edit item", role = Role.Button, onClick = onClick)
                .padding(8.dp),
            verticalArrangement = Arrangement.Center,
        ) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(
                description,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

@Composable
private fun TimelineCompletionButton(
    title: String,
    testTag: String,
    onComplete: () -> Unit,
    completing: Boolean,
    enabled: Boolean,
) {
    IconButton(
        onClick = onComplete,
        enabled = enabled && !completing,
        modifier = Modifier.size(48.dp).testTag(testTag),
    ) {
        if (completing) {
            CircularProgressIndicator(
                modifier = Modifier.size(20.dp).semantics { contentDescription = "Completing $title" },
                strokeWidth = 2.dp,
            )
        } else {
            Icon(Icons.Outlined.CheckBoxOutlineBlank, contentDescription = "Complete $title")
        }
    }
}

@Composable
internal fun TimelineAnnotationChip(
    placement: TimelineAnnotationPlacement,
    zoneId: ZoneId,
    leftPx: Float,
    widthPx: Float,
    targetHeightPx: Float,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    onMoveStart: ((Offset) -> Unit)? = null,
    onMoveBy: ((Int) -> Unit)? = null,
) {
    val density = LocalDensity.current
    val single = placement.annotations.singleOrNull()
    val description = single?.description(zoneId) ?: run {
        val markers = placement.annotations.count { it is TimelineAnnotation.Marker }
        val signals = placement.annotations.size - markers
        listOfNotNull(
            markers.takeIf { it > 0 }?.let { "$it markers" },
            signals.takeIf { it > 0 }?.let { "$it signals" },
        ).joinToString(", ")
    }
    val testTag = when (single) {
        is TimelineAnnotation.Marker -> TimelineTestTags.marker(single.key)
        is TimelineAnnotation.Signal -> TimelineTestTags.signal(single.key)
        null -> TimelineTestTags.annotations(placement.key)
    }
    val moveBy = onMoveBy.takeIf { single != null }
    Box(
        modifier = modifier
            .offset { IntOffset(leftPx.roundToInt(), placement.topPx.roundToInt()) }
            .width(with(density) { widthPx.toDp() })
            .height(with(density) { targetHeightPx.toDp() })
            .timelineBodyDrag(
                key = single?.key ?: placement.key,
                onMoveStart = onMoveStart.takeIf { single != null },
                moveEarlier = moveBy?.let { move -> { move(-1) } },
                moveLater = moveBy?.let { move -> { move(1) } },
            )
            .testTag(testTag)
            .semantics { contentDescription = description }
            .clickable(
                onClickLabel = when (single) {
                    is TimelineAnnotation.Marker -> "Edit marker"
                    is TimelineAnnotation.Signal -> "Edit signal"
                    null -> "Show all markers and signals"
                },
                role = Role.Button,
                onClick = onClick,
            ),
        contentAlignment = Alignment.CenterStart,
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 4.dp).clearAndSetSemantics {},
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Icon(
                imageVector = when (single) {
                    is TimelineAnnotation.Marker -> Icons.Outlined.Flag
                    is TimelineAnnotation.Signal -> Icons.Outlined.Bolt
                    null -> Icons.Outlined.MoreHoriz
                },
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(14.dp),
            )
            Text(
                text = single?.title ?: "${placement.annotations.size} annotations",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}
