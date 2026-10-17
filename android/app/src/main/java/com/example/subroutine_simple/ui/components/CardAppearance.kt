package com.example.subroutine_simple.ui.components

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp

internal object CardAppearance {
    val radius = 20.dp
    val borderWidth = 1.dp
    val padding = 16.dp
    val controlSize = 48.dp
    val textGap = 4.dp

    fun rowGeometry(typography: Typography, density: Density): CardRowGeometry = with(density) {
        CardRowGeometry(
            titleHeight = typography.titleMedium.lineHeight.toDp(),
            metadataHeight = typography.bodySmall.lineHeight.toDp(),
        )
    }

    fun borderColor(colors: ColorScheme): Color = lerp(colors.outlineVariant, colors.outline, 0.5f)

    fun shape(position: ItemCardPosition): RoundedCornerShape {
        val top = if (position == ItemCardPosition.Standalone || position == ItemCardPosition.First) radius else 0.dp
        val bottom = if (position == ItemCardPosition.Standalone || position == ItemCardPosition.Last) radius else 0.dp
        return RoundedCornerShape(topStart = top, topEnd = top, bottomStart = bottom, bottomEnd = bottom)
    }

    fun borderExtension(position: ItemCardPosition, density: Density): Float = when (position) {
        ItemCardPosition.Standalone, ItemCardPosition.First -> 0f
        ItemCardPosition.Middle, ItemCardPosition.Last -> with(density) { borderWidth.toPx() }
    }

    fun borderOutline(size: Size, position: ItemCardPosition, direction: LayoutDirection, density: Density): Outline =
        shape(position).createOutline(
            Size(size.width, size.height + borderExtension(position, density)), direction, density,
        )
}

internal data class CardRowGeometry(val titleHeight: Dp, val metadataHeight: Dp) {
    val contentHeight: Dp
        get() = maxOf(
            CardAppearance.controlSize,
            titleHeight + CardAppearance.textGap + metadataHeight
        )
    val height: Dp get() = contentHeight + CardAppearance.padding * 2
}

@Composable
internal fun ItemCardRow(
    modifier: Modifier = Modifier,
    leading: @Composable () -> Unit,
    title: @Composable () -> Unit,
    metadata: @Composable () -> Unit = {},
    trailing: @Composable () -> Unit = {},
) {
    val geometry = CardAppearance.rowGeometry(MaterialTheme.typography, LocalDensity.current)
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = modifier.fillMaxWidth().height(geometry.height).padding(CardAppearance.padding),
    ) {
        Box(Modifier.size(CardAppearance.controlSize), contentAlignment = Alignment.Center) { leading() }
        Column(
            verticalArrangement = Arrangement.spacedBy(CardAppearance.textGap, Alignment.CenterVertically),
            modifier = Modifier.weight(1f).height(geometry.contentHeight),
        ) {
            Box(
                Modifier.fillMaxWidth().height(geometry.titleHeight),
                contentAlignment = Alignment.CenterStart
            ) { title() }
            Box(
                Modifier.fillMaxWidth().height(geometry.metadataHeight),
                contentAlignment = Alignment.CenterStart
            ) { metadata() }
        }
        trailing()
    }
}

internal fun Modifier.itemCardBorder(position: ItemCardPosition, color: Color): Modifier {
    val shape = CardAppearance.shape(position)
    if (position == ItemCardPosition.Standalone || position == ItemCardPosition.First) {
        return border(CardAppearance.borderWidth, color, shape)
    }
    return clip(shape).drawWithContent {
        drawContent()
        val width = CardAppearance.borderWidth.toPx()
        val extension = CardAppearance.borderExtension(position, this)
        val outline = CardAppearance.borderOutline(size, position, layoutDirection, this)
        // Each lazy row owns its bottom seam. Extend joined tops outside the clip, keeping both sides continuous.
        clipRect {
            translate(top = -extension) {
                drawOutline(outline, color, style = Stroke(width * 2))
            }
        }
    }
}
