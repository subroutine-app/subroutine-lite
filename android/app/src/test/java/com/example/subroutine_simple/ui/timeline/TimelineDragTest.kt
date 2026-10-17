package com.example.subroutine_simple.ui.timeline

import java.time.Instant
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TimelineDragTest {
    private val zone = ZoneId.of("Europe/Berlin")
    private val start = Instant.parse("2026-10-15T08:00:00Z")

    @Test
    fun edgeScrollIsBoundedSymmetricAndDensityIndependent() {
        assertEquals(0f, timelineAutoScrollVelocity(300f, 600f, 1f), 0f)
        assertEquals(-480f, timelineAutoScrollVelocity(-50f, 600f, 1f), 0f)
        assertEquals(480f, timelineAutoScrollVelocity(650f, 600f, 1f), 0f)
        assertEquals(-120f, timelineAutoScrollVelocity(36f, 600f, 1f), 0f)
        assertEquals(240f, timelineAutoScrollVelocity(1128f, 1200f, 2f), 0f)
        assertEquals(0f, timelineAutoScrollVelocity(30f, 60f, 1f), 0f)
    }

    @Test
    fun stationaryPointerTracksScrollingWithoutChangingMoveDuration() {
        val interval = TimelineIndexedInterval(
            "action:a", TimelineItemKind.Action, "a", "Action", null,
            start, start.plusSeconds(1200), false,
        )
        val viewport = TimelineViewport(start, 10.0)
        val source = TimelineDragSource.Card(TimelineCardPlacement(interval, TimelineCardBounds(300.0, 420.0), 0, 1))
        val drag =
            TimelineDrag(source, TimelineDivision.FiveMinutes, 120.0, 312f, source.original(TimelineEditMode.Move))
        val moved = drag.at(312f, viewport.panBy(-60f), 600f, zone).preview as TimelineEdit.Interval
        assertEquals(start.plusSeconds(600), moved.start)
        assertEquals(interval.endExclusive.plusSeconds(600), moved.endExclusive)
        assertEquals(312f, drag.pointerYPx, 0f)
    }

    @Test
    fun bottomHandleUsesGrabOffsetRatherThanMinimumCardHeightAsDuration() {
        val interval = TimelineIndexedInterval(
            "action:a", TimelineItemKind.Action, "a", "Action", null,
            start, start.plusSeconds(60), false,
        )
        val viewport = TimelineViewport(start, 10.0)
        val source = TimelineDragSource.Card(TimelineCardPlacement(interval, TimelineCardBounds(300.0, 372.0), 0, 1))
        val drag = TimelineDrag(source, TimelineDivision.Minute, 660.0, 372f, source.original(TimelineEditMode.End))
        val resized = drag.at(378f, viewport, 600f, zone).preview as TimelineEdit.Interval
        assertEquals(start, resized.start)
        assertEquals(start.plusSeconds(120), resized.endExclusive)
    }

    @Test
    fun accessibleStepsUseCalendarBoundariesAndDistinguishRepeatedHours() {
        val january = Instant.parse("2026-01-31T23:00:00Z")
        val march = timelineStepInstant(january, TimelineDivision.Month, zone, 1)
        assertEquals(Instant.parse("2026-02-28T23:00:00Z"), march)
        assertEquals(january, timelineStepInstant(march, TimelineDivision.Month, zone, -1))
        val firstTwo = Instant.parse("2026-10-25T00:00:00Z")
        val secondTwo = timelineStepInstant(firstTwo, TimelineDivision.Hour, zone, 1)
        assertEquals(firstTwo.plusSeconds(3600), secondTwo)
        assertTrue(secondTwo.atZone(zone).hour == firstTwo.atZone(zone).hour)
    }
}
