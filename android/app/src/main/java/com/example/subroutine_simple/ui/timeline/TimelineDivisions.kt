package com.example.subroutine_simple.ui.timeline

import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.temporal.IsoFields
import java.util.Locale

private const val MinimumDivisionHeightDp = 64.0
private const val MaximumVisibleDivisions = 1_000
private val TimeFormatter = DateTimeFormatter.ofPattern("h:mm", Locale.ENGLISH)
private val HourFormatter = DateTimeFormatter.ofPattern("h a", Locale.ENGLISH)
private val DayFormatter = DateTimeFormatter.ofPattern("EEE d", Locale.ENGLISH)
private val MonthFormatter = DateTimeFormatter.ofPattern("MMM", Locale.ENGLISH)

enum class TimelineDivision(
    val approximateSeconds: Double,
    internal val fixedSeconds: Long? = null,
) {
    Second(1.0, 1),
    FiveSeconds(5.0, 5),
    FifteenSeconds(15.0, 15),
    ThirtySeconds(30.0, 30),
    Minute(60.0, 60),
    FiveMinutes(300.0, 300),
    TenMinutes(600.0, 600),
    FifteenMinutes(900.0, 900),
    ThirtyMinutes(1_800.0, 1_800),
    Hour(3_600.0, 3_600),
    TwoHours(7_200.0, 7_200),
    FourHours(14_400.0, 14_400),
    SixHours(21_600.0, 21_600),
    TwelveHours(43_200.0, 43_200),
    Day(86_400.0),
    Week(604_800.0),
    Month(2_629_746.0),
    Quarter(7_889_238.0),
    Year(31_556_952.0),
}

data class TimelineTick(
    val instant: Instant,
    val label: String,
    val major: Boolean,
)

fun divisionFor(secondsPerPixel: Double, density: Float): TimelineDivision {
    require(secondsPerPixel.isFinite() && secondsPerPixel > 0.0)
    require(density.isFinite() && density > 0f)
    val minimumSeconds = secondsPerPixel * density * MinimumDivisionHeightDp
    return TimelineDivision.entries.firstOrNull { it.approximateSeconds >= minimumSeconds }
        ?: TimelineDivision.Year
}

fun timelineTicks(
    range: TimelineInstantRange,
    zoneId: ZoneId,
    division: TimelineDivision,
): List<TimelineTick> {
    val base = division.basePeriod
    val baseTicks = divisionTicks(range, zoneId, base.division).map { tick ->
        tick.copy(label = base.label(tick.instant.atZone(zoneId)), major = true)
    }
    return (baseTicks + divisionTicks(range, zoneId, division))
        .distinctBy { it.instant }
        .sortedBy { it.instant }
}

private fun divisionTicks(
    range: TimelineInstantRange,
    zoneId: ZoneId,
    division: TimelineDivision,
): List<TimelineTick> = division.fixedSeconds?.let {
    fixedTimelineTicks(range, zoneId, division, it)
} ?: calendarTimelineTicks(range, zoneId, division)

private fun fixedTimelineTicks(
    range: TimelineInstantRange,
    zoneId: ZoneId,
    division: TimelineDivision,
    stepSeconds: Long,
): List<TimelineTick> {
    val startLocal = range.start.atZone(zoneId)
    val endLocal = range.endExclusive.atZone(zoneId)
    val ticks = ArrayList<TimelineTick>()
    var date = startLocal.toLocalDate()
    while (date <= endLocal.toLocalDate() && ticks.size < MaximumVisibleDivisions) {
        val firstSecond = if (date == startLocal.toLocalDate()) {
            startLocal.toLocalTime().toSecondOfDay() / stepSeconds.toInt() * stepSeconds.toInt()
        } else {
            0
        }
        val finalSecond = if (date == endLocal.toLocalDate()) {
            endLocal.toLocalTime().toSecondOfDay()
        } else {
            24 * 60 * 60 - 1
        }
        var secondOfDay = firstSecond
        while (secondOfDay <= finalSecond && ticks.size < MaximumVisibleDivisions) {
            val localDateTime = LocalDateTime.of(date, LocalTime.ofSecondOfDay(secondOfDay.toLong()))
            zoneId.rules.getValidOffsets(localDateTime).forEach { offset ->
                val instant = localDateTime.toInstant(offset)
                if (instant >= range.start && instant < range.endExclusive) {
                    val local = instant.atZone(zoneId)
                    ticks += TimelineTick(
                        instant = instant,
                        label = timelineTickLabel(division, local),
                        major = false,
                    )
                }
            }
            secondOfDay += stepSeconds.toInt()
        }
        date = date.plusDays(1)
    }
    return ticks.sortedBy(TimelineTick::instant)
}

private fun calendarTimelineTicks(
    range: TimelineInstantRange,
    zoneId: ZoneId,
    division: TimelineDivision,
): List<TimelineTick> {
    var boundary = floorCalendarBoundary(range.start.atZone(zoneId), division)
    while (boundary.toInstant() < range.start) {
        boundary = nextCalendarBoundary(boundary, division)
    }

    val ticks = ArrayList<TimelineTick>()
    while (boundary.toInstant() < range.endExclusive && ticks.size < MaximumVisibleDivisions) {
        ticks += TimelineTick(
            instant = boundary.toInstant(),
            label = timelineTickLabel(division, boundary),
            major = false,
        )
        boundary = nextCalendarBoundary(boundary, division)
    }
    return ticks
}

private fun floorCalendarBoundary(
    value: ZonedDateTime,
    division: TimelineDivision,
): ZonedDateTime {
    val date = value.toLocalDate()
    val boundaryDate = when (division) {
        TimelineDivision.Day -> date
        TimelineDivision.Week -> date.minusDays((date.dayOfWeek.value - DayOfWeek.MONDAY.value).toLong())
        TimelineDivision.Month -> date.withDayOfMonth(1)
        TimelineDivision.Quarter -> LocalDate.of(value.year, ((value.monthValue - 1) / 3) * 3 + 1, 1)
        TimelineDivision.Year -> LocalDate.of(value.year, 1, 1)
        else -> date
    }
    return boundaryDate.atStartOfDay(value.zone)
}

private fun nextCalendarBoundary(
    value: ZonedDateTime,
    division: TimelineDivision,
): ZonedDateTime {
    val nextDate = when (division) {
        TimelineDivision.Day -> value.toLocalDate().plusDays(1)
        TimelineDivision.Week -> value.toLocalDate().plusWeeks(1)
        TimelineDivision.Month -> value.toLocalDate().plusMonths(1).withDayOfMonth(1)
        TimelineDivision.Quarter -> value.toLocalDate().plusMonths(3).withDayOfMonth(1)
        TimelineDivision.Year -> LocalDate.of(value.year + 1, 1, 1)
        else -> value.toLocalDate().plusDays(1)
    }
    return ZonedDateTime.of(nextDate, LocalTime.MIDNIGHT, value.zone)
}

internal fun timelineTickLabel(division: TimelineDivision, time: ZonedDateTime): String = when (division) {
    TimelineDivision.Second, TimelineDivision.FiveSeconds,
    TimelineDivision.FifteenSeconds, TimelineDivision.ThirtySeconds -> ""

    TimelineDivision.Minute, TimelineDivision.FiveMinutes, TimelineDivision.TenMinutes,
    TimelineDivision.FifteenMinutes, TimelineDivision.ThirtyMinutes ->
        if (time.minute == 0) hourLabel(time) else time.format(TimeFormatter)

    TimelineDivision.Hour, TimelineDivision.TwoHours, TimelineDivision.FourHours -> hourLabel(time)
    TimelineDivision.SixHours, TimelineDivision.TwelveHours -> when (time.hour) {
        0 -> "Midnight"
        12 -> "Noon"
        in 1..11 -> "Morning"
        in 13..17 -> "Afternoon"
        else -> "Evening"
    }

    TimelineDivision.Day -> time.format(DayFormatter)
    TimelineDivision.Week -> "W${time.get(IsoFields.WEEK_OF_WEEK_BASED_YEAR)}"
    TimelineDivision.Month -> time.format(MonthFormatter)
    TimelineDivision.Quarter -> "Q${(time.monthValue - 1) / 3 + 1}"
    TimelineDivision.Year -> time.year.toString()
}

private fun hourLabel(time: ZonedDateTime): String =
    if (time.hour == 12) "Noon" else time.format(HourFormatter)

internal enum class TimelinePeriod(val division: TimelineDivision) {
    Minute(TimelineDivision.Minute),
    FiveMinutes(TimelineDivision.FiveMinutes),
    Hour(TimelineDivision.Hour),
    Day(TimelineDivision.Day),
    Month(TimelineDivision.Month),
    Year(TimelineDivision.Year);

    val outer: TimelinePeriod?
        get() = when (this) {
            Minute, FiveMinutes -> Hour
            Hour -> Day
            Day -> Month
            Month -> Year
            Year -> null
        }

    fun label(time: ZonedDateTime): String = timelineTickLabel(division, time)

    fun rangeAt(time: ZonedDateTime): TimelineInstantRange {
        val start = when (this) {
            Minute -> time.withSecond(0).withNano(0)
            FiveMinutes -> time.withMinute(time.minute / 5 * 5).withSecond(0).withNano(0)
            Hour -> time.withMinute(0).withSecond(0).withNano(0)
            Day, Month, Year -> floorCalendarBoundary(time, division)
        }
        val end = when (this) {
            Day, Month, Year -> nextCalendarBoundary(start, division)
            else -> shift(start, 1)
        }
        return TimelineInstantRange(start.toInstant(), end.toInstant())
    }

    fun shift(time: ZonedDateTime, direction: Int): ZonedDateTime = when (this) {
        Minute -> time.plusMinutes(direction.toLong())
        FiveMinutes -> time.plusMinutes(direction * 5L)
        Hour -> time.plusHours(direction.toLong())
        Day -> time.plusDays(direction.toLong())
        Month -> time.plusMonths(direction.toLong())
        Year -> time.plusYears(direction.toLong())
    }
}

internal val TimelineDivision.basePeriod: TimelinePeriod
    get() = TimelinePeriod.entries.firstOrNull {
        it.division.approximateSeconds > approximateSeconds
    } ?: TimelinePeriod.Year

internal data class TimelineOuterPeriod(
    val range: TimelineInstantRange,
    val label: String,
    val description: String,
)

internal fun timelineOuterPeriod(
    viewport: TimelineViewport,
    viewportHeightPx: Float,
    zoneId: ZoneId,
    density: Float,
): TimelineOuterPeriod? {
    val outer = divisionFor(viewport.secondsPerPixel, density).basePeriod.outer ?: return null
    val range = outer.rangeAt(viewport.timeAt(0f, viewportHeightPx).atZone(zoneId))
    val start = range.start.atZone(zoneId)
    val label = when (outer) {
        TimelinePeriod.Hour -> start.format(HourFormatter)
        TimelinePeriod.Day -> start.format(DayFormatter)
        TimelinePeriod.Month -> start.format(DateTimeFormatter.ofPattern("MMMM", Locale.ENGLISH))
        TimelinePeriod.Year -> start.year.toString()
        TimelinePeriod.Minute, TimelinePeriod.FiveMinutes -> error("Not an outer period")
    }
    val description = when (outer) {
        TimelinePeriod.Hour -> start.format(DateTimeFormatter.ofPattern("EEE, MMM d, yyyy h a XXX", Locale.ENGLISH))
        TimelinePeriod.Day -> start.format(DateTimeFormatter.ofPattern("EEEE, MMMM d, yyyy", Locale.ENGLISH))
        TimelinePeriod.Month -> start.format(DateTimeFormatter.ofPattern("MMMM yyyy", Locale.ENGLISH))
        else -> label
    }
    return TimelineOuterPeriod(range, label, description)
}

internal fun TimelineViewport.stepPeriod(direction: Int, zoneId: ZoneId, density: Float): TimelineViewport = copy(
    center = divisionFor(secondsPerPixel, density).basePeriod.shift(center.atZone(zoneId), direction).toInstant(),
)

internal fun timelineStepInstant(
    instant: Instant,
    division: TimelineDivision,
    zoneId: ZoneId,
    direction: Int,
): Instant {
    require(direction == -1 || direction == 1)
    val radius = division.approximateSeconds * 2.0
    val ticks = divisionTicks(
        TimelineInstantRange(instant.plusSecondsPrecise(-radius), instant.plusSecondsPrecise(radius)),
        zoneId, division,
    )
    return if (direction < 0) ticks.last { it.instant < instant }.instant
    else ticks.first { it.instant > instant }.instant
}

internal fun snapTimelineInstant(
    instant: Instant,
    division: TimelineDivision,
    zoneId: ZoneId,
): Instant {
    val radiusSeconds = division.approximateSeconds * 2.0
    val candidates = divisionTicks(
        range = TimelineInstantRange(
            instant.plusSecondsPrecise(-radiusSeconds),
            instant.plusSecondsPrecise(radiusSeconds),
        ),
        zoneId = zoneId,
        division = division,
    )
    return candidates.minByOrNull { tick ->
        kotlin.math.abs(secondsBetween(tick.instant, instant))
    }?.instant ?: instant
}
