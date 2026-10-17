package com.example.subroutine_simple.ui.search

import java.time.ZoneId

enum class BrowseFilter(val label: String, val key: String) {
    All("All", "all"),
    Unscheduled("Unscheduled", "unscheduled"),
    Unqueued("Unqueued", "unqueued"),
    Routines("Routines", "routines"),
    Saved("Saved", "saved"),
}

fun projectBrowse(
    corpus: SearchCorpus,
    query: String,
    filter: BrowseFilter,
    zoneId: ZoneId = ZoneId.systemDefault(),
): SearchProjection {
    val collection = when (filter) {
        BrowseFilter.All -> SearchCorpus(
            actions = corpus.actions.filter { it.completion == null && (it.start == null || !it.queued) },
            routines = corpus.routines,
            actionTemplates = corpus.actionTemplates,
            eventTemplates = corpus.eventTemplates,
        )

        BrowseFilter.Unscheduled -> SearchCorpus(
            actions = corpus.actions.filter { it.completion == null && it.start == null },
        )

        BrowseFilter.Unqueued -> SearchCorpus(
            actions = corpus.actions.filter { it.completion == null && !it.queued },
        )

        BrowseFilter.Routines -> SearchCorpus(routines = corpus.routines)
        BrowseFilter.Saved -> SearchCorpus(
            actionTemplates = corpus.actionTemplates,
            eventTemplates = corpus.eventTemplates,
        )
    }
    return projectSearch(collection, SearchRequest(query = query), zoneId = zoneId, showAll = true)
}
