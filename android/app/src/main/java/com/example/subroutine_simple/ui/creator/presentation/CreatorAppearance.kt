package com.example.subroutine_simple.ui.creator.presentation

import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp

internal object CreatorAppearance {
    val cornerRadius = 20.dp
}

// Compensate the content layer's nonuniform scale so the visible corners remain circular.
internal data class CreatorLayerShape(val radiusPx: Float, val scaleX: Float, val scaleY: Float) : Shape {
    init {
        require(radiusPx.isFinite() && radiusPx >= 0f && scaleX.isFinite() && scaleX > 0f && scaleY.isFinite() && scaleY > 0f)
    }

    fun radii(size: Size): CornerRadius {
        val radius = minOf(radiusPx, size.width * scaleX / 2f, size.height * scaleY / 2f)
        return CornerRadius(radius / scaleX, radius / scaleY)
    }

    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline =
        Outline.Rounded(RoundRect(0f, 0f, size.width, size.height, cornerRadius = radii(size)))
}
