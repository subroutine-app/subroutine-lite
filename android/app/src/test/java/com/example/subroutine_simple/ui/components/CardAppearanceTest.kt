package com.example.subroutine_simple.ui.components

import androidx.compose.material3.Typography
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [28])
class CardAppearanceTest {
    private val density = Density(2f)
    private val size = Size(640f, 160f)

    @Test
    fun joinedBordersReachBothSidesAndOwnExactlyOneHorizontalSeam() {
        val width = with(density) { CardAppearance.borderWidth.toPx() }
        for (position in ItemCardPosition.entries) {
            val outline = CardAppearance.borderOutline(size, position, LayoutDirection.Ltr, density)
            val bounds = when (outline) {
                is Outline.Rectangle -> outline.rect
                is Outline.Rounded -> with(outline.roundRect) { Rect(left, top, right, bottom) }
                is Outline.Generic -> error("Cards must use ordinary rounded rectangles")
            }
            val extension = CardAppearance.borderExtension(position, density)
            assertEquals(0f, bounds.left, 0f)
            assertEquals(size.width, bounds.right, 0f)
            assertEquals(size.height, bounds.bottom - extension, 0f)
            when (position) {
                ItemCardPosition.First, ItemCardPosition.Standalone -> assertEquals(0f, extension, 0f)
                ItemCardPosition.Middle, ItemCardPosition.Last -> {
                    assertTrue(
                        "The joined top stroke must be wholly outside the row",
                        bounds.top - extension + width <= 0f
                    )
                }
            }
        }
    }

    @Test
    fun rowGeometryReservesBothTextSlotsAtEveryFontScale() {
        val typography = Typography()
        assertEquals(80.dp, CardAppearance.rowGeometry(typography, Density(1f)).height)
        for (scale in listOf(0.85f, 1f, 1.5f, 2f, 3f)) {
            val scaled = Density(2f, fontScale = scale)
            val geometry = CardAppearance.rowGeometry(typography, scaled)
            with(scaled) {
                assertEquals(typography.titleMedium.lineHeight.toDp(), geometry.titleHeight)
                assertEquals(typography.bodySmall.lineHeight.toDp(), geometry.metadataHeight)
            }
            assertTrue(geometry.contentHeight >= CardAppearance.controlSize)
            assertTrue(geometry.contentHeight >= geometry.titleHeight + CardAppearance.textGap + geometry.metadataHeight)
            assertEquals(geometry.contentHeight + CardAppearance.padding * 2, geometry.height)
        }
    }

    @Test
    fun groupingKeepsOnlyOuterCornersRoundedInBothDirections() {
        for (direction in LayoutDirection.entries) {
            for (position in ItemCardPosition.entries) {
                val outline = CardAppearance.shape(position).createOutline(size, direction, density)
                if (position == ItemCardPosition.Middle) {
                    assertEquals(Outline.Rectangle(Rect(0f, 0f, size.width, size.height)), outline)
                    continue
                }
                val rect = (outline as Outline.Rounded).roundRect
                val rounded = CornerRadius(40f)
                val top = if (position == ItemCardPosition.Last) CornerRadius.Zero else rounded
                val bottom = if (position == ItemCardPosition.First) CornerRadius.Zero else rounded
                assertEquals(top, rect.topLeftCornerRadius)
                assertEquals(top, rect.topRightCornerRadius)
                assertEquals(bottom, rect.bottomLeftCornerRadius)
                assertEquals(bottom, rect.bottomRightCornerRadius)
            }
        }
    }
}
