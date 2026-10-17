package com.example.subroutine_simple.ui.timeline

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ExpandLess
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp

internal fun timelineAllDayPreviewHeightPx(density: Density): Float = with(density) {
    48.dp.toPx() * fontScale.coerceAtLeast(1f)
}

/** Item bounds are row-local (0..minimumCardHeightPx); measure item-to-viewport coordinates for dragging. */
@Composable
internal fun TimelineAllDayPreview(
    placement: TimelineAllDayPlacement,
    leftPx: Float,
    widthPx: Float,
    viewportHeightPx: Float,
    minimumCardHeightPx: Float,
    onToggle: () -> Unit,
    enabled: Boolean,
    itemContent: @Composable (TimelineCardPlacement) -> Unit,
) {
    if (placement.intervals.isEmpty() || !placement.bounds.intersects(viewportHeightPx)) return
    val density = LocalDensity.current
    val previewHeightPx = timelineAllDayPreviewHeightPx(density)
    val cardHeight = with(density) { minimumCardHeightPx.toDp() }
    val panelHeight = with(density) { placement.heightPx.toFloat().toDp() }
    key(placement.key) {
        val listState = rememberLazyListState()
        val titles = remember(placement.intervals) { placement.intervals.joinToString { it.title } }
        val count = placement.intervals.size
        TimelineCardSurface(
            bounds = placement.bounds,
            viewportHeightPx = viewportHeightPx,
            leftPx = leftPx,
            widthPx = widthPx,
            modifier = Modifier.testTag("timeline:${placement.key}"),
        ) {
            Column(Modifier.fillMaxWidth().height(panelHeight).clipToBounds()) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(if (placement.expanded) with(density) { previewHeightPx.toDp() } else panelHeight)
                        .testTag("timeline:${placement.key}:toggle")
                        .semantics { stateDescription = if (placement.expanded) "Expanded" else "Collapsed" }
                        .clickable(
                            enabled = enabled,
                            role = Role.Button,
                            onClickLabel = if (placement.expanded) "Collapse all-day actions" else "Expand all-day actions",
                            onClick = onToggle,
                        )
                        .padding(horizontal = 8.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(
                            text = if (count == 1) "1 all-day action" else "$count all-day actions",
                            style = MaterialTheme.typography.titleSmall,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Text(
                            text = titles,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    Icon(
                        imageVector = if (placement.expanded) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(start = 8.dp).size(24.dp),
                    )
                }
                if (placement.expanded) {
                    Spacer(Modifier.height(8.dp))
                    // LazyColumn consumes touch scrolling in Main before the viewport's gesture handler.
                    LazyColumn(
                        state = listState,
                        modifier = Modifier.fillMaxWidth().weight(1f).clipToBounds()
                            .testTag("timeline:${placement.key}:items"),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                        userScrollEnabled = enabled,
                    ) {
                        items(placement.intervals, key = { it.key }) { interval ->
                            Box(Modifier.fillMaxWidth().height(cardHeight)) {
                                itemContent(
                                    TimelineCardPlacement(
                                        interval = interval,
                                        bounds = TimelineCardBounds(0.0, minimumCardHeightPx.toDouble()),
                                        lane = 0,
                                        laneCount = 1,
                                    ),
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}
