package com.example.subroutine_simple.ui.search

import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.ActionTemplate
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.EventTemplate
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.Routine
import com.example.subroutine_simple.data.models.RoutineStep
import com.example.subroutine_simple.data.models.Signal
import java.time.LocalDate
import java.time.ZoneId
import java.time.ZoneOffset
import org.junit.Assert.assertEquals
import org.junit.Test

class BrowseProjectionTest {
    private val utc = ZoneOffset.UTC

    @Test
    fun actionEligibilityCrossesQueueDateAndCompletionWithoutDuplicatingTheUnion() {
        val starts = listOf("anytime" to null, "date" to "2026-10-15", "time" to "2026-10-15T12:00:00Z")
        val actions = buildList {
            for ((name, start) in starts) {
                for (queued in listOf(false, true)) {
                    for (completion in listOf(null, "2026-10-15T13:00:00Z")) {
                        val id =
                            "${if (queued) "queued" else "unqueued"}-$name-${if (completion == null) "open" else "done"}"
                        add(action(id).copy(start = start, queued = queued, completion = completion))
                    }
                }
            }
        }
        val corpus = SearchCorpus(actions = actions)
        val expected = mapOf(
            BrowseFilter.All to listOf(
                "queued-anytime-open",
                "unqueued-anytime-open",
                "unqueued-date-open",
                "unqueued-time-open"
            ),
            BrowseFilter.Unscheduled to listOf("queued-anytime-open", "unqueued-anytime-open"),
            BrowseFilter.Unqueued to listOf("unqueued-anytime-open", "unqueued-date-open", "unqueued-time-open"),
        )
        for ((filter, ids) in expected) {
            for (query in listOf("", "open")) {
                val keys = results(projectBrowse(corpus, query, filter, utc)).map { it.key }
                assertEquals("$filter with query '$query'", ids.map { "action:$it" }, keys)
            }
        }
    }

    @Test
    fun blankBrowseSortsAlphabeticallyWithStableTypedIdsAndSeparatesCollections() {
        val corpus = SearchCorpus(
            actions = listOf(
                action("z", "Alpha"),
                action("shared", "Alpha"),
                action("a", "Alpha"),
                action("last", "zebra")
            ),
            events = listOf(
                Event(
                    "shared",
                    "lineage",
                    title = "Alpha",
                    start = "2026-10-15T12:00:00Z",
                    duration = "PT1H"
                )
            ),
            routines = listOf(
                Routine("untargeted", title = "beta"),
                Routine("shared", title = "Alpha", target = "2026-10-15"),
            ),
            markers = listOf(Marker("shared", "lineage", title = "Alpha", date = "2026-10-15")),
            signals = listOf(Signal("shared", "lineage", title = "Alpha", datetime = "2026-10-15T12:00:00Z")),
            actionTemplates = listOf(ActionTemplate("shared", title = "Alpha")),
            eventTemplates = listOf(EventTemplate("shared", "lineage", title = "Alpha", duration = "PT1H")),
        )
        val all = projectBrowse(corpus, " \n\t ", BrowseFilter.All, utc)
        assertEquals(
            listOf(
                "action:a",
                "action:shared",
                "action:z",
                "routine:shared",
                "saved-action:shared",
                "saved-event:shared",
                "routine:untargeted",
                "action:last"
            ),
            results(all).map { it.key },
        )
        assertEquals(
            all,
            projectBrowse(
                corpus.copy(actions = corpus.actions.reversed(), routines = corpus.routines.reversed()),
                "",
                BrowseFilter.All,
                utc
            ),
        )
        assertEquals(
            listOf("routine:shared", "routine:untargeted"),
            results(projectBrowse(corpus, "", BrowseFilter.Routines, utc)).map { it.key },
        )
        assertEquals(
            listOf("saved-action:shared", "saved-event:shared"),
            results(projectBrowse(corpus, "", BrowseFilter.Saved, utc)).map { it.key },
        )
    }

    @Test
    fun emptyCollectionIsDistinctFromANonemptyCollectionWithNoMatches() {
        val corpus = SearchCorpus(actions = listOf(action("action", "Read")))
        for (query in listOf("", " \n ", "missing")) {
            for (filter in BrowseFilter.entries) {
                assertEquals(SearchProjection.Empty, projectBrowse(SearchCorpus(), query, filter, utc))
            }
            assertEquals(SearchProjection.Empty, projectBrowse(corpus, query, BrowseFilter.Routines, utc))
            assertEquals(SearchProjection.Empty, projectBrowse(corpus, query, BrowseFilter.Saved, utc))
            val ineligible = corpus.copy(
                actions = listOf(
                    action("completed").copy(completion = "2026-10-15T13:00:00Z"),
                    action("scheduled").copy(queued = true, start = "2026-10-15"),
                )
            )
            assertEquals(SearchProjection.Empty, projectBrowse(ineligible, query, BrowseFilter.All, utc))
        }
        assertEquals(SearchProjection.NoMatches, projectBrowse(corpus, "missing", BrowseFilter.All, utc))
        assertEquals(SearchProjection.NoMatches, projectBrowse(corpus, "missing", BrowseFilter.Unscheduled, utc))
        assertEquals(listOf("action:action"), results(projectBrowse(corpus, "", BrowseFilter.All, utc)).map { it.key })
    }

    @Test
    fun browseSharesSearchRankingForTitlesNotesAndRoutineStepsAfterFiltering() {
        val eligible = SearchCorpus(
            actions = listOf(
                action("title", "Needle").copy(start = "2026-10-16T01:00:00Z"),
                action("notes", "Alpha").copy(content = "Needle in notes"),
            ),
            routines = listOf(
                Routine("steps", title = "Beta", steps = listOf(RoutineStep("Needle in a step"))),
                Routine("notes", title = "Gamma", content = "Needle in notes"),
            ),
            actionTemplates = listOf(ActionTemplate("notes", title = "Delta", content = "Needle in notes")),
            eventTemplates = listOf(
                EventTemplate(
                    "notes",
                    "lineage",
                    title = "Epsilon",
                    content = "Needle in notes",
                    duration = "PT1H"
                )
            ),
        )
        val corpus = eligible.copy(
            actions = eligible.actions + listOf(
                action("scheduled", "Needle").copy(queued = true, start = "2026-10-15"),
                action("completed", "Needle").copy(completion = "2026-10-15T13:00:00Z"),
            )
        )
        val zone = ZoneId.of("America/Los_Angeles")
        val projection = projectBrowse(corpus, " nEeDlE ", BrowseFilter.All, zone)
        assertEquals(projectSearch(eligible, SearchRequest(query = " nEeDlE "), zoneId = zone), projection)
        val matches = results(projection)
        assertEquals(
            listOf(
                "action:title",
                "action:notes",
                "saved-action:notes",
                "saved-event:notes",
                "routine:notes",
                "routine:steps"
            ),
            matches.map { it.key },
        )
        assertEquals(
            LocalDate.of(2026, 10, 15),
            (matches.first().schedule as SearchScheduleMetadata.Timed).local.toLocalDate()
        )
        assertEquals(
            listOf("routine:steps"),
            results(projectBrowse(corpus, "beta needle", BrowseFilter.Routines, zone)).map { it.key },
        )
        assertEquals(
            listOf("saved-action:notes", "saved-event:notes"),
            results(projectBrowse(corpus, "needle", BrowseFilter.Saved, zone)).map { it.key },
        )
        assertEquals(SearchProjection.NoMatches, projectBrowse(corpus, "needle absent", BrowseFilter.All, zone))
    }

    private fun action(id: String, title: String = id) = Action(id = id, recurrenceId = id, title = title)

    private fun results(projection: SearchProjection): List<SearchResult> =
        (projection as SearchProjection.Results).items
}
