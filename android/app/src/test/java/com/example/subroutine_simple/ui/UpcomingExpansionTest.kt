package com.example.subroutine_simple.ui

import com.example.subroutine_simple.ui.agenda.AgendaSection
import com.example.subroutine_simple.ui.agenda.expandedIn
import com.example.subroutine_simple.ui.agenda.toggleUpcomingSections
import java.time.LocalDate
import org.junit.Assert.*
import org.junit.Test

class UpcomingExpansionTest {
    @Test
    fun expandCollapseAllMatchesDesktopForMixedAndClosedSections() {
        val today = LocalDate.of(2026, 10, 20)
        val sections = listOf(
            AgendaSection("any-time", null, emptyList()),
            AgendaSection("today", today, emptyList()),
            AgendaSection("future", today.plusDays(1), emptyList()),
        )
        val choices = mapOf("other-view" to true)
        assertTrue(sections[0].expandedIn(choices, today))
        assertTrue(sections[1].expandedIn(choices, today))
        assertFalse(sections[2].expandedIn(choices, today))
        val collapsed = toggleUpcomingSections(sections, choices, today)
        assertTrue(sections.none { it.expandedIn(collapsed, today) })
        assertEquals(true, collapsed["other-view"])
        val expanded = toggleUpcomingSections(sections, collapsed, today)
        assertTrue(sections.all { it.expandedIn(expanded, today) })
        assertEquals(choices, toggleUpcomingSections(emptyList(), choices, today))
    }
}
