package com.example.subroutine_simple.ui.creator

import androidx.compose.ui.geometry.Rect
import org.junit.Assert.assertEquals
import org.junit.Test

class ParserHighlightGeometryTest {
    @Test
    fun visualRunsKeepBidiGapsWhileMergingAdjacentGlyphs() {
        val result = mergeHighlightGlyphs(
            listOf(
                Rect(50f, 0f, 60f, 20f), Rect(10f, 0f, 20f, 20f),
                Rect(20f, 0f, 30f, 20f), Rect(30f, 0f, 30f, 20f),
            )
        )
        assertEquals(listOf(Rect(10f, 0f, 30f, 20f), Rect(50f, 0f, 60f, 20f)), result)
    }
}
