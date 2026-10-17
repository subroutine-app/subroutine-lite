package com.example.subroutine_simple.ui.timeline

import com.example.subroutine_simple.data.models.Action
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId

class TimelineGeometryTest {
    private val origin = Instant.parse("2026-10-15T00:00:00Z")
    private val viewport = TimelineViewport(origin.plusSeconds(300), 1.0)

    @Test
    fun timedCardMovesRigidlyThroughBothViewportEdges() {
        val item = interval("long", -100, 800)
        val packing = pack(listOf(item))
        val first = packing.project(viewport, 600f).timed.cards.single()
        val moved = packing.project(viewport.panBy(-150f), 600f).timed.cards.single()

        assertEquals(-100.0, first.topPx, 0.0001)
        assertEquals(900.0, first.heightPx, 0.0001)
        assertEquals(first.topPx - 150, moved.topPx, 0.0001)
        assertEquals(first.heightPx, moved.heightPx, 0.0001)
        assertEquals(item, moved.interval)
    }

    @Test
    fun minimumHeightTailRemainsUntilItsLastPhysicalPixelLeaves() {
        val packing = pack(listOf(interval("short", 0, 1)))
        val partial = packing.project(viewport.panBy(-71.75f), 600f).timed.cards.single()
        val slice = partial.bounds.slice(600f)!!

        assertEquals(72.0, partial.heightPx, 0.0001)
        assertEquals(1f, slice.heightPx, 0f)
        assertEquals(0.25, slice.physicalBottomPx, 0.0001)
        assertTrue(packing.project(viewport.panBy(-72f), 600f).timed.cards.isEmpty())
        assertTrue(packing.project(viewport.panBy(600f), 600f).timed.cards.isEmpty())
        assertEquals(1, packing.project(viewport.panBy(599.75f), 600f).timed.cards.size)
    }

    @Test
    fun clippingPreservesActualRoundedEdgesAndBoundsHugeSpans() {
        val huge = TimelineCardBounds(-1e15, 1e15)
        val slice = huge.slice(600f)!!
        val contour = slice.contour(200f, 24f)
        assertEquals(600f, slice.heightPx, 0f)
        assertEquals(-48f, contour.topPx, 0f)
        assertEquals(648f, contour.bottomPx, 0f)
        assertEquals(24f, contour.radiusPx, 0f)

        val entering = TimelineCardBounds(590.5, 900.0).slice(600f)!!
        assertEquals(590f, entering.topPx, 0f)
        assertEquals(0.5f, entering.contour(200f, 24f).topPx, 0f)
        val exiting = TimelineCardBounds(-500.0, 12.5).slice(600f)!!
        assertEquals(12.5f, exiting.contour(200f, 24f).bottomPx, 0f)
        assertNull(TimelineCardBounds(-500.0, 0.0).slice(600f))
    }

    @Test
    fun contextSticksOnlyWhileItsOwnPhysicalCardContainsIt() {
        assertEquals(100.0, TimelineCardBounds(100.0, 400.0).stickyContentTop(64.0), 0.0)
        assertEquals(0.0, TimelineCardBounds(-100.0, 200.0).stickyContentTop(64.0), 0.0)
        assertEquals(-44.0, TimelineCardBounds(-280.0, 20.0).stickyContentTop(64.0), 0.0)
        assertEquals(-10.0, TimelineCardBounds(-10.0, 20.0).stickyContentTop(64.0), 0.0)
    }

    @Test
    fun stickyAllDayPreviewsClearOnlyTimedContext() {
        val event = interval("long", -100, 800)
        val overlap = interval("overlap", -50, 750)
        val physical = TimelineCardBounds(-100.0, 800.0)
        for (actions in listOf(listOf(allDay("one")), listOf(allDay("one"), allDay("two")))) {
            val layout = pack(actions + event).project(viewport, 600f)
            val clearance = layout.allDay.topClearance(8f)
            val card = layout.timed.cards.single()
            assertEquals(56.0, clearance, 0.0)
            assertEquals(physical, card.bounds)
            assertEquals(56.0, card.bounds.stickyContentTop(64.0, clearance), 0.0)

            val dense = pack(actions + listOf(event, overlap), lanes = 1).project(viewport, 600f)
            val bin = dense.timed.density.single()
            assertEquals(physical, bin.bounds)
            assertEquals(56.0, bin.bounds.stickyContentTop(64.0, dense.allDay.topClearance(8f)), 0.0)
            assertEquals(0.0, TimelineCardBounds(0.0, 72.0).stickyContentTop(64.0), 0.0)
        }
    }

    @Test
    fun laterAllDayContentDoesNotMoveUnrelatedContextAboveIt() {
        val event = interval("long", -300, 800)
        val earlierViewport = viewport.panBy(200f)
        for (actions in listOf(listOf(allDay("one")), listOf(allDay("one"), allDay("two")))) {
            val layout = pack(actions + event).project(earlierViewport, 600f)
            val clearance = layout.allDay.topClearance(8f)
            assertEquals(0.0, clearance, 0.0)
            assertEquals(0.0, layout.timed.cards.single().bounds.stickyContentTop(64.0, clearance), 0.0)
        }
    }

    @Test
    fun contextClearanceStillHonorsItsOwnCardAndDisappearsWithTheAllDayTail() {
        assertEquals(100.0, TimelineCardBounds(100.0, 400.0).stickyContentTop(64.0, 80.0), 0.0)
        assertEquals(36.0, TimelineCardBounds(-100.0, 100.0).stickyContentTop(64.0, 80.0), 0.0)
        assertEquals(-44.0, TimelineCardBounds(-280.0, 20.0).stickyContentTop(64.0, 80.0), 0.0)
        assertEquals(-10.0, TimelineCardBounds(-10.0, 20.0).stickyContentTop(64.0, 80.0), 0.0)

        val packing = pack(listOf(allDay("day")))
        val leaving = viewport.panBy(-86380f)
        val layer = packing.project(leaving, 600f).allDay
        assertEquals(-28.0, layer.days.single().topPx, 0.0)
        assertEquals(28.0, layer.topClearance(8f), 0.0)
        assertEquals(0.0, packing.project(leaving.panBy(-20f), 600f).allDay.topClearance(8f), 0.0)
    }

    @Test
    fun lanesKeepWholeGroupWidthWhenEarlyAndLateMembersLeaveTheViewport() {
        val long = interval("long", 0, 1400)
        val items = listOf(long, interval("early", 50, 200), interval("late", 1000, 1200))
        val packing = pack(items)
        val first = packing.project(viewport, 600f).timed.cards.single { it.interval == long }
        val middle = packing.project(viewport.panBy(-400f), 600f).timed.cards.single()
        val last = packing.project(viewport.panBy(-900f), 600f).timed.cards.single { it.interval == long }

        assertEquals(2, first.laneCount)
        assertEquals(first.laneCount, middle.laneCount)
        assertEquals(first.laneCount, last.laneCount)
        assertEquals(first.lane, middle.lane)
        assertEquals(first.lane, last.lane)
        assertEquals(
            first.lane,
            pack(items.reversed()).project(viewport, 600f).timed.cards.single { it.interval == long }.lane,
        )
    }

    @Test
    fun anOffscreenOverlapCannotSwitchASurvivingGroupBetweenCardsAndBins() {
        val items = listOf(interval("long", 0, 1400), interval("early", 50, 200), interval("third", 60, 210))
        val packing = pack(items)
        val first = packing.project(viewport, 600f).timed
        val later = packing.project(viewport.panBy(-700f), 600f).timed

        assertTrue(first.cards.isEmpty())
        assertTrue(later.cards.isEmpty())
        assertEquals(first.density.single().key, later.density.single().key)
        assertEquals(items.toSet(), later.density.single().intervals.toSet())
    }

    @Test
    fun allDayActionCannotBinOrNarrowATimedCardInASingleLane() {
        val event = interval("event", 100, 400)
        val action = allDay("day")
        val baseline = pack(listOf(event), lanes = 1).project(viewport, 600f)
        val mixed = pack(listOf(action, event), lanes = 1).project(viewport, 600f)

        assertEquals(baseline.timed, mixed.timed)
        assertEquals(event, mixed.timed.cards.single().interval)
        assertEquals(1, mixed.timed.cards.single().laneCount)
        assertTrue(mixed.timed.density.isEmpty())
        assertEquals(listOf(action), mixed.allDay.days.single().intervals)
        assertEquals(48.0, mixed.allDay.days.single().heightPx, 0.0)
        assertFalse(mixed.allDay.days.single().expanded)
    }

    @Test
    fun timedDensityRetainsPhysicalBoundsAndItsLastClippedPixel() {
        val items = listOf(interval("one", -100, 800), interval("two", -50, 750))
        val packing = pack(items, lanes = 1)
        val first = packing.project(viewport, 600f).timed.density.single()
        val moved = packing.project(viewport.panBy(-150f), 600f).timed.density.single()
        assertEquals(-100.0, first.topPx, 0.0)
        assertEquals(900.0, first.heightPx, 0.0)
        assertEquals(first.topPx - 150, moved.topPx, 0.0)
        assertEquals(first.heightPx, moved.heightPx, 0.0)
        assertEquals(0.0, first.bounds.stickyContentTop(48.0), 0.0)
        assertEquals(600f, first.bounds.slice(600f)!!.heightPx, 0f)

        val tail = packing.project(viewport.panBy(-799.75f), 600f).timed.density.single()
        val slice = tail.bounds.slice(600f)!!
        assertEquals(900.0, tail.heightPx, 0.0)
        assertEquals(1f, slice.heightPx, 0f)
        assertEquals(0.25f, slice.contour(200f, 24f).bottomPx, 0f)
        assertEquals(-47.75, tail.bounds.stickyContentTop(48.0), 0.0)
        assertTrue(packing.project(viewport.panBy(-800f), 600f).timed.density.isEmpty())
    }

    @Test
    fun allDayPreviewsAreFixedHeightAndGroupedByTheirOwnDate() {
        val today = listOf(allDay("first"), allDay("second"))
        val tomorrow = listOf(allDay("third", 86400), allDay("fourth", 86400))
        val event = interval("event", 100, 400)
        val packing = pack(today + tomorrow + event, lanes = 1)
        val initial = packing.project(viewport, 600f)
        assertEquals(listOf(event), initial.timed.cards.map { it.interval })
        assertTrue(initial.timed.density.isEmpty())
        val preview = initial.allDay.days.single()
        val midday = packing.project(viewport.panBy(-12000f), 600f).allDay.days.single()
        assertEquals(today.toSet(), preview.intervals.toSet())
        assertEquals(preview.key, midday.key)
        assertEquals(48.0, preview.heightPx, 0.0)
        assertEquals(48.0, midday.heightPx, 0.0)
        assertEquals(0.0, midday.topPx, 0.0)

        val nextDay = packing.project(viewport.panBy(-86400f), 600f).allDay.days.single()
        assertEquals(tomorrow.toSet(), nextDay.intervals.toSet())
        assertEquals(48.0, nextDay.heightPx, 0.0)
    }

    @Test
    fun allDayActionHasFixedHeightAndLeavesWithItsActualLocalDay() {
        val zone = ZoneId.of("Europe/Berlin")
        val date = LocalDate.parse("2026-03-29")
        val action = Action("day", "recurrence", title = "Read", queued = true, start = date.toString())
        val index = TimelineDataIndex.build(zone, listOf(action), emptyList(), emptyList(), emptyList())
        val item = index.intervals.single()
        val dayStart = date.atStartOfDay(zone).toInstant()
        val dayEnd = date.plusDays(1).atStartOfDay(zone).toInstant()
        assertEquals(23 * 3600.0, secondsBetween(dayStart, dayEnd), 0.0)

        val packing = pack(index.intervals)
        val midday = TimelineViewport(dayStart.plusSeconds(12 * 3600), 1.0)
        val panel = packing.project(midday, 600f).allDay.days.single()
        assertEquals(0.0, panel.topPx, 0.0)
        assertEquals(48.0, panel.heightPx, 0.0)
        assertEquals(dayStart, panel.range.start)
        assertEquals(dayEnd, panel.range.endExclusive)
        assertTrue(item.timeLabel(zone).endsWith("Any time"))
        assertFalse(item.timeLabel(zone).contains("00:00"))

        val leaving = TimelineViewport(dayEnd.plusSeconds(290), 1.0)
        for (expandedDays in listOf(emptySet(), setOf(dayStart))) {
            val tail = packing.project(leaving, 600f, expandedDays).allDay.days.single()
            val height = if (expandedDays.isEmpty()) 48.0 else 128.0
            assertEquals(10.0 - height, tail.topPx, 0.0)
            assertEquals(height, tail.heightPx, 0.0)
            assertNotNull(tail.bounds.slice(600f))
            assertEquals(18.0, packing.project(leaving, 600f, expandedDays).allDay.topClearance(8f), 0.0)
            assertTrue(packing.project(leaving.panBy(-10f), 600f, expandedDays).allDay.days.isEmpty())
        }
    }

    @Test
    fun expandedAllDayPanelBoundsTheListNotItsMembersAndClearsTimedContext() {
        val actions = (1..50).map { allDay("day-$it") }
        val event = interval("long", -100, 800)
        val packing = pack(actions + event)
        val collapsed = packing.project(viewport, 600f)
        val expanded = packing.project(viewport, 600f, expandedDays = setOf(origin))
        val panel = expanded.allDay.days.single()

        assertTrue(panel.expanded)
        assertEquals(collapsed.allDay.days.single().key, panel.key)
        assertEquals(actions.map { it.key }.sorted(), panel.intervals.map { it.key })
        assertEquals(300.0, panel.heightPx, 0.0)
        assertEquals(308.0, expanded.allDay.topClearance(8f), 0.0)
        assertEquals(collapsed.timed, expanded.timed)
        assertEquals(308.0, expanded.timed.cards.single().bounds.stickyContentTop(64.0, 308.0), 0.0)
    }

    @Test
    fun expansionKeepsOneCardAccessibleAndUsesCallerPixelMetrics() {
        val items = listOf(allDay("first"), allDay("second"))
        val packing = pack(items)
        val expandedDays = mutableSetOf(origin)
        val compact = packing.project(viewport, 200f, expandedDays).allDay.days.single()
        assertEquals(48.0 + 8.0 + 72.0, compact.heightPx, 0.0)
        assertTrue(compact.expanded)
        assertTrue(compact.heightPx < 200.0)
        for (height in listOf(127f, 48f)) {
            val fallback = packing.project(viewport, height, expandedDays).allDay
            assertFalse(fallback.days.single().expanded)
            assertEquals(48.0, fallback.days.single().heightPx, 0.0)
            assertEquals(packing.project(viewport, height).allDay, fallback)
        }
        assertTrue(packing.project(viewport, 47f, expandedDays).allDay.days.isEmpty())
        assertEquals(setOf(origin), expandedDays)
        assertTrue(packing.project(viewport, 128f, expandedDays).allDay.days.single().expanded)

        val scaled = packTimelineIntervals(items, 1.0, 2, 144f, 16f)
        val one = scaled.project(viewport, 1200f, setOf(origin), 96f).allDay.days.single()
        assertEquals(96.0 + 16.0 + 144.0 * 2 + 16.0, one.heightPx, 0.0)
        assertEquals(
            96.0, scaled.project(viewport, 1200f, allDayPreviewHeightPx = 96f)
                .allDay.days.single().heightPx, 0.0
        )
    }

    @Test
    fun neighboringAllDayPanelsCollapseBeforeCullingAtLowZoom() {
        val items = (0L..2L).flatMap { day ->
            (1..10).map { allDay("$day-$it", day * 86400) }
        }
        val expandedDays = items.map { it.start }.toSet()
        for (scale in listOf(540.0, 675.0, 1350.0, 2700.0)) {
            val zoomed = TimelineViewport(origin.plusSeconds((300 * scale).toLong()), scale).panBy(-32f)
            val packing = packTimelineIntervals(items, scale, 2, 72f, 8f)
            for (expanded in listOf(emptySet(), expandedDays)) {
                val panels = packing.project(zoomed, 600f, expanded).allDay.days
                val effectivelyExpanded = expanded.isNotEmpty() && 86400 / scale >= 128.0
                val minimumHeight = if (effectivelyExpanded) 128.0 else 48.0
                if (86400 / scale < 48.0) {
                    assertTrue(panels.isEmpty())
                } else {
                    assertEquals(3, panels.size)
                    panels.forEach { panel ->
                        assertEquals(effectivelyExpanded, panel.expanded)
                        assertTrue(panel.heightPx >= minimumHeight)
                        if (!effectivelyExpanded) assertEquals(48.0, panel.heightPx, 0.0)
                        assertTrue(panel.topPx >= zoomed.preciseYAt(panel.range.start, 600f))
                        assertTrue(panel.bounds.bottomPx <= zoomed.preciseYAt(panel.range.endExclusive, 600f))
                    }
                    panels.zipWithNext().forEach { (earlier, later) ->
                        assertTrue(earlier.bounds.bottomPx <= later.topPx)
                    }
                }
            }
        }
    }

    @Test
    fun dayIdentityAndCallerExpansionSurviveMemberChangesAndTemporaryAbsence() {
        val first = allDay("a")
        val retained = allDay("b")
        val expandedDays = setOf(origin)
        val initial = pack(listOf(first, retained)).project(viewport, 600f, expandedDays).allDay.days.single()
        val changed = pack(listOf(retained.copy(title = "Updated"), allDay("c")))
            .project(viewport, 600f, expandedDays).allDay.days.single()
        assertEquals(initial.key, changed.key)
        assertEquals(listOf(retained.key, "action:c"), changed.intervals.map { it.key })
        assertEquals("Updated", changed.intervals.first().title)
        assertTrue(changed.expanded)
        assertTrue(pack(emptyList()).project(viewport, 600f, expandedDays).allDay.days.isEmpty())
        val restored = pack(listOf(retained)).project(viewport, 600f, expandedDays).allDay.days.single()
        assertEquals(initial.key, restored.key)
        assertTrue(restored.expanded)
        assertEquals(128.0, restored.heightPx, 0.0)
    }

    @Test
    fun enormousTimedIntervalsRemainAttachedWithoutMeasuringTheirEntireHeight() {
        val item = interval("centuries", -10_000_000_000, 10_000_000_000)
        val tinyScale = viewport.copy(secondsPerPixel = MIN_TIMELINE_SECONDS_PER_PIXEL)
        val placement = packTimelineIntervals(listOf(item), tinyScale.secondsPerPixel, 2, 72f, 8f)
            .project(tinyScale, 600f).timed.cards.single()
        assertEquals(item, placement.interval)
        assertEquals(600f, placement.bounds.slice(600f)!!.heightPx, 0f)
        assertTrue(placement.topPx < -1e11)
        assertTrue(placement.heightPx > 1e11)
    }

    private fun pack(items: List<TimelineIndexedInterval>, lanes: Int = 2) =
        packTimelineIntervals(items, 1.0, lanes, 72f, 8f)

    private fun allDay(id: String, start: Long = 0) = interval(id, start, start + 86400).copy(
        key = "action:$id", kind = TimelineItemKind.Action, isDateSpan = true,
    )

    private fun interval(id: String, start: Long, end: Long) = TimelineIndexedInterval(
        key = "event:$id", kind = TimelineItemKind.Event, id = id, title = id, content = null,
        start = origin.plusSeconds(start), endExclusive = origin.plusSeconds(end), isDateSpan = false,
    )
}
