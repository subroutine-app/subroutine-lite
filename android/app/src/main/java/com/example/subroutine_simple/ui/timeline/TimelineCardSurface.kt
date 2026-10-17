package com.example.subroutine_simple.ui.timeline

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.width
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import com.example.subroutine_simple.ui.components.CardAppearance


@Composable
internal fun TimelineCardSurface(
    bounds: TimelineCardBounds,
    viewportHeightPx: Float,
    leftPx: Float,
    widthPx: Float,
    modifier: Modifier = Modifier,
    stickyContextTopPx: Double = 0.0,
    content: @Composable () -> Unit,
) {
    val slice = bounds.slice(viewportHeightPx) ?: return
    val density = LocalDensity.current
    val shape = TimelineSliceShape(
        slice, with(density) { CardAppearance.radius.toPx() }, with(density) { CardAppearance.borderWidth.toPx() },
    )
    val borderColor = CardAppearance.borderColor(MaterialTheme.colorScheme)
    CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onSurface) {
        Box(
            modifier = Modifier
                .offset { IntOffset(leftPx.roundToInt(), slice.topPx.roundToInt()) }
                .width(with(density) { widthPx.toDp() })
                .height(with(density) { slice.heightPx.toDp() })
                .clipToBounds()
                .clip(shape)
                .background(MaterialTheme.colorScheme.surfaceContainer)
                .drawWithCache {
                    val path = shape.path(size.width)
                    onDrawWithContent {
                        drawContent()
                        drawPath(path, borderColor, style = Stroke(2.dp.toPx()))
                    }
                }
                .then(modifier),
        ) {
            Layout(content = content) { measurables, constraints ->
                // Measure context independently of the visible slice, so it exits by clipping, not reflow.
                val context = measurables.single().measure(Constraints.fixedWidth(constraints.maxWidth))
                val contentTop = bounds.stickyContentTop(context.height.toDouble(), stickyContextTopPx)
                layout(constraints.maxWidth, constraints.maxHeight) {
                    context.placeRelative(0, (contentTop - slice.topPx).roundToInt())
                }
            }
        }
    }
}

private class TimelineSliceShape(
    private val slice: TimelineCardSlice,
    private val radiusPx: Float,
    private val borderWidthPx: Float,
) : Shape {
    fun path(widthPx: Float): Path {
        val contour = slice.contour(widthPx, radiusPx, borderWidthPx)
        return Path().apply {
            addRoundRect(
                RoundRect(
                    left = 0f, top = contour.topPx, right = widthPx, bottom = contour.bottomPx,
                    cornerRadius = CornerRadius(contour.radiusPx),
                ),
            )
        }
    }

    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline =
        Outline.Generic(path(size.width))
}
