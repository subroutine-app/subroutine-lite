package com.example.subroutine_simple.ui.timeline

import com.example.subroutine_simple.data.models.Action
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant
import java.time.ZoneOffset

class TimelineHierarchyTest {
    private val origin = Instant.parse("2026-10-15T12:00:00Z")
    private val viewport = TimelineViewport(origin.plusSeconds(300), secondsPerPixel = 1.0)

    @Test
    fun markersDoNotConsumePrimaryLanesAndIsolatedCardsUseFullWidth() {
        val action = interval("action", TimelineItemKind.Action, 100, 160)
        val marker = interval("marker", TimelineItemKind.Marker, -500, 900)
        val result = layout(listOf(marker, action), lanes = 3)

        assertEquals(listOf(action), result.cards.map { it.interval })
        assertEquals(1, result.cards.single().laneCount)
        assertTrue(result.density.isEmpty())
    }

    @Test
    fun narrowOverlapsKeepEveryMemberWithoutCoveringAnotherCard() {
        val overlapping = (0 until 12).map { interval("action-$it", TimelineItemKind.Action, 100, 160) }
        val later = interval("later", TimelineItemKind.Event, 250, 350)
        val result = layout(overlapping + later, lanes = 1)

        assertEquals(overlapping.toSet(), result.density.single().intervals.toSet())
        assertEquals(overlapping.size, result.density.single().count)
        assertEquals(listOf(later), result.cards.map { it.interval })
        assertTrue(result.density.single().topPx + result.density.single().heightPx + 8 <= result.cards.single().topPx)
    }

    @Test
    fun clippingAnOverflowGroupDoesNotPushItsTargetOverTheNextGroup() {
        val past = (0..2).map { interval("past-$it", TimelineItemKind.Event, -70, -60) }
        val upcoming = interval("upcoming", TimelineItemKind.Event, 12, 30)
        val result = layout(past + upcoming, lanes = 1)

        assertTrue(result.density.single().topPx + result.density.single().heightPx + 8 <= result.cards.single().topPx)
    }

    @Test
    fun mixedAnnotationsClusterWithoutOverlappingTargetsAndClipAtViewportEdges() {
        val marker = interval("marker", TimelineItemKind.Marker, -1000, 800)
        val signals = listOf(0L, 10L, 100L, 200L, 575L, 590L).map { offset ->
            TimelineIndexedSignal("signal:$offset", "$offset", "Signal $offset", null, origin.plusSeconds(offset))
        }
        val placements = layoutTimelineAnnotations(
            TimelineVisibleData(listOf(marker), signals), viewport, 600f, 48f, 8f,
        )

        assertEquals(7, placements.sumOf { it.annotations.size })
        assertTrue(placements.first().annotations.any { it is TimelineAnnotation.Marker })
        assertTrue(placements.first().annotations.any { it is TimelineAnnotation.Signal })
        assertTrue(placements.all { it.topPx < 600 && it.topPx + 48 > 0 })
        assertEquals(-24f, placements.first().topPx, 0f)
        assertTrue(placements.zipWithNext().all { (first, second) -> first.topPx + 48 + 8 <= second.topPx })
        assertEquals(placements.size, placements.map { it.key }.toSet().size)
    }

    @Test
    fun signalTargetsRemainAttachedUntilTheirEntireTargetScrollsOut() {
        val signal = TimelineIndexedSignal("signal:one", "one", "Signal", null, origin)
        val data = TimelineVisibleData(emptyList(), listOf(signal))
        val partial = layoutTimelineAnnotations(data, viewport.panBy(-23f), 600f, 48f, 8f).single()
        assertEquals(-47f, partial.topPx, 0f)
        assertTrue(layoutTimelineAnnotations(data, viewport.panBy(-24f), 600f, 48f, 8f).isEmpty())
    }

    @Test
    fun markerContextLeavesWithItsOwnSpanInsteadOfStickingOutsideIt() {
        val marker = interval("marker", TimelineItemKind.Marker, -1000, 20)
        val data = TimelineVisibleData(listOf(marker), emptyList())
        val partial = layoutTimelineAnnotations(data, viewport, 600f, 48f, 8f).single()
        assertEquals(-28f, partial.topPx, 0f)
        assertTrue(layoutTimelineAnnotations(data, viewport.panBy(-20f), 600f, 48f, 8f).isEmpty())
    }

    @Test
    fun durationlessActionsUseFallbackOnlyForLayoutNotDisplayedSchedule() {
        val action = Action("action", "recurrence", title = "Read", queued = true, start = origin.toString())
        val index = TimelineDataIndex.build(ZoneOffset.UTC, listOf(action), emptyList(), emptyList(), emptyList())
        val item = index.query(TimelineInstantRange(origin, origin.plusSeconds(600))).intervals.single()

        assertEquals(origin.plusSeconds(300), item.endExclusive)
        assertFalse(item.hasExplicitDuration)
        assertEquals(origin.atZone(ZoneOffset.UTC).format(TimelineTimeFormatter), item.timeLabel(ZoneOffset.UTC))
    }

    private fun layout(items: List<TimelineIndexedInterval>, lanes: Int) = layoutTimelineIntervals(
        items, viewport, viewportHeightPx = 600f, maximumLanes = lanes,
        minimumCardHeightPx = 72f, cardGapPx = 8f,
    ).timed

    private fun interval(id: String, kind: TimelineItemKind, start: Long, end: Long) = TimelineIndexedInterval(
        key = "${kind.name}:$id",
        kind = kind,
        id = id,
        title = id,
        content = null,
        start = origin.plusSeconds(start),
        endExclusive = origin.plusSeconds(end),
        isDateSpan = kind == TimelineItemKind.Marker,
    )
}
