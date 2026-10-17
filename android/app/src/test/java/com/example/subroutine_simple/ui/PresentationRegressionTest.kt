package com.example.subroutine_simple.ui

import com.example.subroutine_simple.ui.components.CompoundAgendaChecks
import com.example.subroutine_simple.ui.creator.ItemCreatorStateSaverChecks
import com.example.subroutine_simple.ui.home.HomeProjectionChecks
import com.example.subroutine_simple.ui.calendar.CalendarItem
import com.example.subroutine_simple.ui.calendar.projectCalendar
import com.example.subroutine_simple.data.models.Event
import java.time.LocalDate
import java.time.YearMonth
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Test

class PresentationRegressionTest {
    @Test
    fun upcomingAndCompoundCards() = CompoundAgendaChecks.main(emptyArray())
    @Test
    fun creatorDraftRoundTrips() = ItemCreatorStateSaverChecks.main(emptyArray())
    @Test
    fun homeEligibility() = HomeProjectionChecks.main(emptyArray())

    @Test
    fun calendarShowsOvernightEventsOnEveryOccupiedDayButNotTheirExclusiveEnd() {
        val event = Event("event", "lineage", title = "Overnight", start = "2026-10-15T22:00:00Z", duration = "PT2H")
        fun items(date: String, duration: String) = projectCalendar(
            YearMonth.of(2026, 10), LocalDate.parse(date), emptyList(),
            listOf(event.copy(duration = duration)), emptyList(), emptyList(), ZoneId.of("UTC"),
        ).selectedDay.items.filterIsInstance<CalendarItem.EventItem>()
        assertEquals(1, items("2026-10-15", "PT2H").size)
        assertEquals(0, items("2026-10-16", "PT2H").size)
        assertEquals(1, items("2026-10-16", "PT3H").size)
    }
}
