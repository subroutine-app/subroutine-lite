package com.example.subroutine_simple.ui.timeline

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CheckBoxOutlineBlank
import androidx.compose.material.icons.outlined.Event
import androidx.compose.material.icons.outlined.Flag
import androidx.compose.ui.graphics.vector.ImageVector
import java.time.ZoneId
import java.time.format.DateTimeFormatter

internal val TimelineTimeFormatter: DateTimeFormatter = DateTimeFormatter.ofPattern("EEE, MMM d HH:mm")
private val TimelineDateFormatter = DateTimeFormatter.ofPattern("MMM d, yyyy")

internal fun timelineTypeIcon(kind: TimelineItemKind): ImageVector = when (kind) {
    TimelineItemKind.Action -> Icons.Outlined.CheckBoxOutlineBlank
    TimelineItemKind.Event -> Icons.Outlined.Event
    TimelineItemKind.Marker -> Icons.Outlined.Flag
}

internal fun TimelineIndexedInterval.timeLabel(zoneId: ZoneId): String {
    if (isDateSpan) {
        val first = start.atZone(zoneId).toLocalDate()
        val last = endExclusive.atZone(zoneId).toLocalDate().minusDays(1)
        val dates = if (first == last) first.format(TimelineDateFormatter)
        else "${first.format(TimelineDateFormatter)} – ${last.format(TimelineDateFormatter)}"
        return if (kind == TimelineItemKind.Action) "$dates · Any time" else dates
    }
    val from = start.atZone(zoneId).format(TimelineTimeFormatter)
    return if (hasExplicitDuration) "$from – ${endExclusive.atZone(zoneId).format(TimelineTimeFormatter)}" else from
}

internal fun TimelineAnnotation.description(zoneId: ZoneId): String = when (this) {
    is TimelineAnnotation.Marker -> "Marker: $title, ${interval.timeLabel(zoneId)}"
    is TimelineAnnotation.Signal -> "Signal: $title, ${start.atZone(zoneId).format(TimelineTimeFormatter)}"
}
