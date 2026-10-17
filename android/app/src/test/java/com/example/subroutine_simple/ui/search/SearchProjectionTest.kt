package com.example.subroutine_simple.ui.search

import com.example.subroutine_simple.data.models.Action
import java.time.ZoneOffset
import org.junit.Assert.assertEquals
import org.junit.Test

class SearchProjectionTest {
    @Test
    fun showingTheCollectionIsOptInAndDoesNotChangeGlobalSearchPromptsOrSmartFilters() {
        val corpus = SearchCorpus(actions = listOf(Action("action", "recurrence", title = "Read")))
        val utc = ZoneOffset.UTC
        for (query in listOf("", " \n\t ", "---")) {
            val request = SearchRequest(query = query)
            assertEquals(SearchProjection.Prompt, projectSearch(corpus, request, zoneId = utc))
            assertEquals(SearchProjection.Empty, projectSearch(SearchCorpus(), request, zoneId = utc))
            val collection = projectSearch(corpus, request, zoneId = utc, showAll = true)
            assertEquals(listOf("action:action"), (collection as SearchProjection.Results).items.map { it.key })
            assertEquals(
                collection,
                projectSearch(corpus, request.copy(smartFilters = setOf(SearchSmartFilter.Unqueued)), zoneId = utc),
            )
        }
        val request = SearchRequest(query = "missing")
        assertEquals(SearchProjection.NoMatches, projectSearch(corpus, request, zoneId = utc, showAll = true))
    }
}
