package com.example.subroutine_simple.ui.creator

import androidx.compose.ui.geometry.Size
import com.example.subroutine_simple.ui.creator.presentation.CreatorLayerShape
import java.time.LocalDate
import java.time.ZoneId
import org.junit.Assert.*
import org.junit.Test

class CreatorHandoffTest {
    @Test
    fun moreOptionsPreservesRawTitleDateAndCreationIdentityButStartsFreshControls() {
        val source = InlineCreatorSource("upcoming:workspace:day", "pending-action")
        val date = LocalDate.of(2026, 10, 20)
        val zone = ZoneId.of("Europe/Berlin")
        val first = CreatorSession.fromInline(source, "Read 🧪 tomorrow", date, zone)
        val reopened = CreatorSession.fromInline(source, "Read 🧪 tomorrow", date, zone)
        assertEquals("Read 🧪 tomorrow", first.initial.title)
        assertEquals(CreatorSchedule(date = date, zoneId = zone), first.initial.schedule)
        assertTrue(first.initial.actionQueued)
        assertTrue(first.expandDetails)
        assertEquals(source.creationId, first.creationId)
        assertEquals(first.creationId, reopened.creationId)
        assertNotEquals(first.id, reopened.id)
        assertFalse(CreatorSession(ItemCreatorState()).expandDetails)
    }

    @Test
    fun morphKeepsTheFabRadiusCircularDespiteUnequalContentScales() {
        val content = Size(360f, 280f)
        for (step in 0..10) {
            val fraction = step / 10f
            val width = 56f + (content.width - 56f) * fraction
            val height = 56f + (content.height - 56f) * fraction
            val scaleX = width / content.width
            val scaleY = height / content.height
            val radii = CreatorLayerShape(20f, scaleX, scaleY).radii(content)
            assertEquals(20f, radii.x * scaleX, 0.001f)
            assertEquals(20f, radii.y * scaleY, 0.001f)
        }
    }
}
