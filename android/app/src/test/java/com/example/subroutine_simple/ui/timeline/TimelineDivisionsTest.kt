package com.example.subroutine_simple.ui.timeline

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.ZoneOffset

class TimelineDivisionsTest {
    private val utc = ZoneOffset.UTC
    private val height = 600f

    @Test
    fun divisionsAndTickSpacingAgreeAtOneAndThreeTimesDensity() {
        val center = Instant.parse("2024-02-15T12:00:00Z")
        for (unit in TimelineDivision.entries) {
            for (factor in listOf(1.0, 1.01)) {
                val secondsPerDp = unit.approximateSeconds / 64.0 * factor
                val baseline = TimelineViewport(center, secondsPerDp)
                val division = divisionFor(secondsPerDp, density = 1f)
                val ticks = timelineTicks(baseline.visibleRange(height), utc, division)
                for (density in listOf(1f, 3f)) {
                    val viewport = TimelineViewport(center, secondsPerDp / density)
                    val heightPx = height * density
                    val actualDivision = divisionFor(viewport.secondsPerPixel, density)
                    assertEquals("$unit at $secondsPerDp s/dp, density $density", division, actualDivision)
                    val actualTicks = timelineTicks(viewport.visibleRange(heightPx), utc, actualDivision)
                    assertEquals(ticks, actualTicks)
                    ticks.forEach { tick ->
                        assertEquals(
                            baseline.yAt(tick.instant, height),
                            viewport.yAt(tick.instant, heightPx) / density,
                            0.001f
                        )
                    }
                    // Calendar months and DST can shorten the nominal 64dp interval.
                    actualTicks.filterNot { it.major }.zipWithNext().forEach { (first, second) ->
                        val spacingDp =
                            secondsBetween(first.instant, second.instant) / viewport.secondsPerPixel / density
                        assertTrue("$actualDivision: $spacingDp dp", spacingDp >= 48.0)
                    }
                }
                if (division != TimelineDivision.Year) {
                    assertTrue(division.approximateSeconds / secondsPerDp >= 64.0)
                    assertTrue(division.basePeriod.division.approximateSeconds >= division.approximateSeconds * 2)
                }
            }
        }
    }

    @Test
    fun denseDisplayUsesDpMinimumRatherThanNearestPhysicalPixelSpacing() {
        val density = 3f
        val scale = DEFAULT_TIMELINE_SECONDS_PER_PIXEL
        val division = divisionFor(scale, density)
        assertEquals(TimelineDivision.Hour, division)
        assertEquals(100.0, division.approximateSeconds / scale / density, 0.0)
        assertEquals(TimelineDivision.FiveMinutes, divisionFor(2.0, density = 1f))
        assertEquals(TimelineDivision.Year, divisionFor(MAX_TIMELINE_SECONDS_PER_PIXEL, density))
    }

    @Test
    fun snappingSteppingAndOuterFitUseTheSameDensityAwareHierarchy() {
        val top = Instant.parse("2026-10-15T13:25:00Z")
        val instant = Instant.parse("2026-10-15T13:41:00Z")
        for (unit in listOf(TimelineDivision.FifteenMinutes, TimelineDivision.Hour, TimelineDivision.Day)) {
            val baseline = atTop(top, unit)
            val period = timelineOuterPeriod(baseline, height, utc, density = 1f)!!
            val fitted = baseline.fit(period.range, height)
            val snapped = snapTimelineInstant(instant, unit, utc)
            for (density in listOf(1f, 3f)) {
                val viewport = baseline.copy(secondsPerPixel = baseline.secondsPerPixel / density)
                val heightPx = height * density
                val division = divisionFor(viewport.secondsPerPixel, density)
                assertEquals(unit, division)
                assertEquals(snapped, snapTimelineInstant(instant, division, utc))
                assertEquals(
                    baseline.stepPeriod(1, utc, density = 1f).center,
                    viewport.stepPeriod(1, utc, density).center,
                )
                assertEquals(viewport, viewport.stepPeriod(1, utc, density).stepPeriod(-1, utc, density))
                val actualPeriod = timelineOuterPeriod(viewport, heightPx, utc, density)!!
                assertEquals(period, actualPeriod)
                val actualFit = viewport.fit(actualPeriod.range, heightPx)
                assertEquals(fitted.center, actualFit.center)
                assertEquals(fitted.secondsPerPixel, actualFit.secondsPerPixel * density, 0.001)
                assertEquals(60f, actualFit.yAt(period.range.start, heightPx) / density, 0.001f)
                assertEquals(540f, actualFit.yAt(period.range.endExclusive, heightPx) / density, 0.001f)
                assertEquals(
                    timelineOuterPeriod(fitted, height, utc, density = 1f),
                    timelineOuterPeriod(actualFit, heightPx, utc, density),
                )
            }
        }
    }

    @Test
    fun tickLabelsMatchDesktopTimeAndCalendarHierarchy() {
        fun label(division: TimelineDivision, time: String) =
            timelineTickLabel(division, Instant.parse(time).atZone(utc))

        assertEquals("", label(TimelineDivision.Second, "2026-10-15T12:00:05Z"))
        assertEquals("12 AM", label(TimelineDivision.Hour, "2026-10-15T00:00:00Z"))
        assertEquals("Noon", label(TimelineDivision.Hour, "2026-10-15T12:00:00Z"))
        assertEquals("1 PM", label(TimelineDivision.TwoHours, "2026-10-15T13:00:00Z"))
        assertEquals("1:15", label(TimelineDivision.FifteenMinutes, "2026-10-15T13:15:00Z"))
        assertEquals("Evening", label(TimelineDivision.SixHours, "2026-10-15T18:00:00Z"))
        assertEquals("Thu 15", label(TimelineDivision.Day, "2026-10-15T00:00:00Z"))
        assertEquals("W1", label(TimelineDivision.Week, "2025-12-29T00:00:00Z"))
        assertEquals("Oct", label(TimelineDivision.Month, "2026-10-01T00:00:00Z"))
        assertEquals("Q4", label(TimelineDivision.Quarter, "2026-10-01T00:00:00Z"))
        assertEquals("2026", label(TimelineDivision.Year, "2026-01-01T00:00:00Z"))
    }

    @Test
    fun baseDateBoundariesReplaceSubdivisionLabelsWithoutDuplicateTicks() {
        val start = Instant.parse("2026-10-15T00:00:00Z")
        val ticks = timelineTicks(TimelineInstantRange(start, start.plusSeconds(24 * 3600)), utc, TimelineDivision.Hour)
        assertEquals("Thu 15", ticks.first().label)
        assertTrue(ticks.first().major)
        assertEquals("Noon", ticks.single { it.instant == start.plusSeconds(12 * 3600) }.label)
        assertEquals(ticks.size, ticks.map { it.instant }.toSet().size)

        val yearStart = Instant.parse("2026-01-01T00:00:00Z")
        val quarters = timelineTicks(
            TimelineInstantRange(yearStart, Instant.parse("2027-01-01T00:00:00Z")), utc, TimelineDivision.Quarter,
        )
        assertEquals(listOf("2026", "Q2", "Q3", "Q4"), quarters.map { it.label })
    }

    @Test
    fun outerPeriodChangesWithScaleLikeDesktop() {
        val top = Instant.parse("2026-10-15T13:25:00Z")
        assertEquals("1 PM", outer(top, TimelineDivision.Second)!!.label)
        assertEquals("1 PM", outer(top, TimelineDivision.Minute)!!.label)
        assertEquals("Thu 15", outer(top, TimelineDivision.FifteenMinutes)!!.label)
        assertEquals("October", outer(top, TimelineDivision.Hour)!!.label)
        assertEquals("2026", outer(top, TimelineDivision.Day)!!.label)
        assertNull(outer(top, TimelineDivision.Month))
        assertNull(outer(top, TimelineDivision.Year))
    }

    @Test
    fun outerPeriodBelongsToTheViewportTopNotItsCenteredDate() {
        val top = Instant.parse("2026-10-15T23:30:00Z")
        val viewport = atTop(top, TimelineDivision.FifteenMinutes)
        assertEquals(LocalDate.parse("2026-10-16"), viewport.center.atZone(utc).toLocalDate())
        assertEquals("Thu 15", timelineOuterPeriod(viewport, height, utc, density = 1f)!!.label)
        val boundary = Instant.parse("2026-10-16T00:00:00Z")
        assertEquals("Thu 15", outer(boundary.minusSeconds(1), TimelineDivision.FifteenMinutes)!!.label)
        assertEquals("Fri 16", outer(boundary, TimelineDivision.FifteenMinutes)!!.label)
    }

    @Test
    fun outerFitUsesExactCalendarBoundariesIncludingShortDaysAndLeapMonths() {
        val berlin = ZoneId.of("Europe/Berlin")
        val top = Instant.parse("2026-03-29T12:00:00Z")
        val viewport = atTop(top, TimelineDivision.FifteenMinutes)
        val period = timelineOuterPeriod(viewport, height, berlin, density = 1f)!!
        assertEquals(23 * 3600.0, secondsBetween(period.range.start, period.range.endExclusive), 0.0)
        val fitted = viewport.fit(period.range, height)
        assertEquals(60f, fitted.yAt(period.range.start, height), 0.001f)
        assertEquals(540f, fitted.yAt(period.range.endExclusive, height), 0.001f)

        val february = outer(Instant.parse("2024-02-15T00:00:00Z"), TimelineDivision.Hour)!!
        assertEquals("February", february.label)
        assertEquals(29 * 86400.0, secondsBetween(february.range.start, february.range.endExclusive), 0.0)
        assertEquals(Instant.parse("2024-02-01T00:00:00Z"), february.range.start)
        assertEquals(Instant.parse("2024-03-01T00:00:00Z"), february.range.endExclusive)
    }

    @Test
    fun repeatedLocalHoursHaveDifferentFitRanges() {
        val zone = ZoneId.of("America/New_York")
        val first =
            timelineOuterPeriod(
                atTop(Instant.parse("2026-11-01T05:30:00Z"), TimelineDivision.Second),
                height,
                zone,
                density = 1f
            )!!
        val second =
            timelineOuterPeriod(
                atTop(Instant.parse("2026-11-01T06:30:00Z"), TimelineDivision.Second),
                height,
                zone,
                density = 1f
            )!!
        assertEquals("1 AM", first.label)
        assertEquals(first.label, second.label)
        assertEquals(first.range.endExclusive, second.range.start)
        assertFalse(first.description == second.description)
        assertEquals(3600.0, secondsBetween(second.range.start, second.range.endExclusive), 0.0)
    }

    @Test
    fun earlierAndLaterPreserveScaleAndMoveByTheBasePeriod() {
        val viewport = atTop(Instant.parse("2026-10-15T00:00:00Z"), TimelineDivision.FifteenMinutes)
        val next = viewport.stepPeriod(1, utc, density = 1f)
        assertEquals(viewport.center.plusSeconds(3600), next.center)
        assertEquals(viewport.secondsPerPixel, next.secondsPerPixel, 0.0)
        assertEquals(viewport, next.stepPeriod(-1, utc, density = 1f))
    }

    @Test
    fun fittingCenturiesDoesNotSaturateNanosecondArithmetic() {
        val range = TimelineInstantRange(Instant.parse("1800-01-01T00:00:00Z"), Instant.parse("2600-01-01T00:00:00Z"))
        val fitted = TimelineViewport(range.start, 1.0).fit(range, height)
        assertEquals(
            secondsBetween(range.start, range.endExclusive) / 2,
            secondsBetween(range.start, fitted.center),
            0.001
        )
        assertTrue(fitted.secondsPerPixel <= MAX_TIMELINE_SECONDS_PER_PIXEL)
    }

    private fun atTop(top: Instant, division: TimelineDivision): TimelineViewport {
        val scale = division.approximateSeconds / 64.0
        return TimelineViewport(top.plusSecondsPrecise(height / 2 * scale), scale)
    }

    private fun outer(top: Instant, division: TimelineDivision) =
        timelineOuterPeriod(atTop(top, division), height, utc, density = 1f)
}
