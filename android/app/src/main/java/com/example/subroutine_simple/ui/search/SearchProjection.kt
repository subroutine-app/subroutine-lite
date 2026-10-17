package com.example.subroutine_simple.ui.search

import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.ActionTemplate
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.EventTemplate
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.Routine
import com.example.subroutine_simple.data.models.Signal
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale

enum class SearchKind(
    val filterLabel: String,
    val typeLabel: String,
    val keyPrefix: String,
) {
    Action("Actions", "Action", "action"),
    Event("Events", "Event", "event"),
    Routine("Routines", "Routine", "routine"),
    Marker("Markers", "Marker", "marker"),
    Signal("Signals", "Signal", "signal"),
    SavedAction("Saved actions", "Saved action", "saved-action"),
    SavedEvent("Saved events", "Saved event", "saved-event"),
}

enum class SearchSmartFilter(val label: String, val key: String) {
    Unqueued("Unqueued", "unqueued"),
    Overdue("Overdue", "overdue"),
    RecentlyCompleted("Recently completed", "recently-completed"),
    AllCompleted("All completed", "all-completed"),
}

enum class SearchSort(val label: String, val key: String) {
    BestMatch("Best match", "best-match"),
    Time("Time", "time"),
    Title("Title", "title"),
    Type("Type", "type"),
}

data class SearchRequest(
    val query: String = "",
    val kinds: Set<SearchKind> = emptySet(),
    val smartFilters: Set<SearchSmartFilter> = emptySet(),
    val sort: SearchSort = SearchSort.BestMatch,
)

data class SearchCorpus(
    val actions: List<Action> = emptyList(),
    val events: List<Event> = emptyList(),
    val routines: List<Routine> = emptyList(),
    val markers: List<Marker> = emptyList(),
    val signals: List<Signal> = emptyList(),
    val actionTemplates: List<ActionTemplate> = emptyList(),
    val eventTemplates: List<EventTemplate> = emptyList(),
) {
    val isEmpty: Boolean
        get() = actions.isEmpty() &&
                events.isEmpty() &&
                routines.isEmpty() &&
                markers.isEmpty() &&
                signals.isEmpty() &&
                actionTemplates.isEmpty() &&
                eventTemplates.isEmpty()
}

sealed interface SearchResource {
    val id: String
    val kind: SearchKind

    sealed interface Live : SearchResource
    sealed interface Saved : SearchResource

    data class LiveAction(val action: Action) : Live {
        override val id: String = action.id
        override val kind: SearchKind = SearchKind.Action
    }

    data class LiveEvent(val event: Event) : Live {
        override val id: String = event.id
        override val kind: SearchKind = SearchKind.Event
    }

    data class LiveRoutine(val routine: Routine) : Live {
        override val id: String = routine.id
        override val kind: SearchKind = SearchKind.Routine
    }

    data class LiveMarker(val marker: Marker) : Live {
        override val id: String = marker.id
        override val kind: SearchKind = SearchKind.Marker
    }

    data class LiveSignal(val signal: Signal) : Live {
        override val id: String = signal.id
        override val kind: SearchKind = SearchKind.Signal
    }

    data class SavedAction(val template: ActionTemplate) : Saved {
        override val id: String = template.id
        override val kind: SearchKind = SearchKind.SavedAction
    }

    data class SavedEvent(val template: EventTemplate) : Saved {
        override val id: String = template.id
        override val kind: SearchKind = SearchKind.SavedEvent
    }
}

sealed interface SearchScheduleMetadata {
    val sortInstant: Instant?

    data class Timed(
        val original: String,
        val instant: Instant,
        val local: ZonedDateTime,
    ) : SearchScheduleMetadata {
        override val sortInstant: Instant = instant
    }

    data class DateRange(
        val startDate: LocalDate,
        val endDate: LocalDate = startDate,
        val zoneId: ZoneId,
    ) : SearchScheduleMetadata {
        override val sortInstant: Instant = startDate.atStartOfDay(zoneId).toInstant()
    }

    data class TimeOfDay(val localTime: LocalTime) : SearchScheduleMetadata {
        override val sortInstant: Instant? = null
    }
}

data class SearchResult(
    val key: String,
    val displayTitle: String,
    val supportingText: String?,
    val typeLabel: String,
    val schedule: SearchScheduleMetadata?,
    val completedAt: ZonedDateTime?,
    val resource: SearchResource,
    val relevanceScore: Int,
)

sealed interface SearchProjection {
    data object Empty : SearchProjection

    data object Prompt : SearchProjection

    data object NoMatches : SearchProjection

    data class Results(val items: List<SearchResult>) : SearchProjection
}

private const val RecentCompletionDays = 7L
private val SearchTokenRegex = Regex("[\\p{L}\\p{N}]+")
private val DisplayDate = DateTimeFormatter.ofPattern("MMM d, uuuu", Locale.ENGLISH)
private val DisplayDateTime = DateTimeFormatter.ofPattern("MMM d, uuuu, h:mm a", Locale.ENGLISH)
private val DisplayTime = DateTimeFormatter.ofPattern("h:mm a", Locale.ENGLISH)

fun projectSearch(
    corpus: SearchCorpus,
    request: SearchRequest,
    now: Instant = Instant.now(),
    zoneId: ZoneId = ZoneId.systemDefault(),
    showAll: Boolean = false,
): SearchProjection {
    if (corpus.isEmpty) return SearchProjection.Empty

    val queryTokens = tokenize(request.query)
    if (!showAll && queryTokens.isEmpty() && request.smartFilters.isEmpty()) {
        return SearchProjection.Prompt
    }

    val normalizedQuery = queryTokens.joinToString(" ")
    val ranked = corpus.resources().asSequence()
        .filter { request.kinds.isEmpty() || it.kind in request.kinds }
        .filter { it.matchesSmartFilters(request.smartFilters, now, zoneId) }
        .mapNotNull { resource ->
            val text = resource.searchText()
            val relevance = relevanceScore(text, queryTokens, normalizedQuery) ?: return@mapNotNull null
            RankedResult(
                result = resource.toResult(relevance, zoneId),
                normalizedTitle = normalize(resource.title()),
                sortInstant = resource.schedule(zoneId)?.sortInstant ?: resource.completion(zoneId)?.toInstant(),
            )
        }
        .toMutableList()

    ranked.sortWith(rankedComparator(request.sort))
    if (ranked.isEmpty()) return SearchProjection.NoMatches
    return SearchProjection.Results(ranked.map(RankedResult::result))
}

private data class RankedResult(
    val result: SearchResult,
    val normalizedTitle: String,
    val sortInstant: Instant?,
)

private data class SearchText(
    val title: String,
    val content: String,
    val routineSteps: List<String>,
)

private fun SearchCorpus.resources(): List<SearchResource> = buildList {
    actions.forEach { add(SearchResource.LiveAction(it)) }
    events.forEach { add(SearchResource.LiveEvent(it)) }
    routines.forEach { add(SearchResource.LiveRoutine(it)) }
    markers.forEach { add(SearchResource.LiveMarker(it)) }
    signals.forEach { add(SearchResource.LiveSignal(it)) }
    actionTemplates.forEach { add(SearchResource.SavedAction(it)) }
    eventTemplates.forEach { add(SearchResource.SavedEvent(it)) }
}

private fun SearchResource.matchesSmartFilters(
    filters: Set<SearchSmartFilter>,
    now: Instant,
    zoneId: ZoneId,
): Boolean {
    if (filters.isEmpty()) {
        return this !is SearchResource.LiveAction || action.completion == null
    }
    val action = (this as? SearchResource.LiveAction)?.action ?: return false
    return filters.any { filter ->
        when (filter) {
            SearchSmartFilter.Unqueued -> action.completion == null && !action.queued
            SearchSmartFilter.Overdue -> action.isOverdue(now, zoneId)
            SearchSmartFilter.RecentlyCompleted -> action.isRecentlyCompleted(now)
            SearchSmartFilter.AllCompleted -> action.completion != null
        }
    }
}

private fun Action.isOverdue(now: Instant, zoneId: ZoneId): Boolean {
    if (completion != null) return false
    val value = start ?: return false
    return if ('T' in value) {
        parseTimestamp(value)?.let { it < now } == true
    } else {
        parseDate(value)?.let { it < now.atZone(zoneId).toLocalDate() } == true
    }
}

private fun Action.isRecentlyCompleted(now: Instant): Boolean {
    val completed = parseTimestamp(completion ?: return false) ?: return false
    val cutoff = now.minusSeconds(RecentCompletionDays * 24L * 60L * 60L)
    return completed >= cutoff && completed <= now
}

private fun SearchResource.searchText(): SearchText = SearchText(
    title = normalize(title()),
    content = normalize(content().orEmpty()),
    routineSteps = when (this) {
        is SearchResource.LiveRoutine -> routine.steps.map { normalize(it.title) }
        else -> emptyList()
    },
)

private fun relevanceScore(
    text: SearchText,
    tokens: List<String>,
    normalizedQuery: String,
): Int? {
    if (tokens.isEmpty()) return 0

    var tokenScore = 0
    for (token in tokens) {
        val score = maxOf(
            fieldTokenScore(text.title, token, exact = 4_000, prefix = 3_000, contains = 2_000),
            fieldTokenScore(text.content, token, exact = 1_000, prefix = 750, contains = 500),
            text.routineSteps.maxOfOrNull {
                fieldTokenScore(it, token, exact = 400, prefix = 300, contains = 200)
            } ?: 0,
        )
        if (score == 0) return null
        tokenScore += score
    }

    val phraseScore = when {
        text.title == normalizedQuery -> 100_000
        text.title.startsWith(normalizedQuery) -> 80_000
        text.title.contains(normalizedQuery) -> 60_000
        text.content.contains(normalizedQuery) -> 30_000
        text.routineSteps.any { it.contains(normalizedQuery) } -> 20_000
        else -> 0
    }
    return phraseScore + tokenScore
}

private fun fieldTokenScore(
    field: String,
    token: String,
    exact: Int,
    prefix: Int,
    contains: Int,
): Int {
    if (field.isEmpty()) return 0
    val words = field.split(' ')
    return when {
        words.any { it == token } -> exact
        words.any { it.startsWith(token) } -> prefix
        field.contains(token) -> contains
        else -> 0
    }
}

private fun rankedComparator(sort: SearchSort): Comparator<RankedResult> = Comparator { left, right ->
    val stable = compareStable(left, right)
    when (sort) {
        SearchSort.BestMatch -> compareValues(right.result.relevanceScore, left.result.relevanceScore)
            .takeUnless { it == 0 } ?: stable

        SearchSort.Time -> compareTime(left.sortInstant, right.sortInstant)
            .takeUnless { it == 0 } ?: stable

        SearchSort.Title -> stable
        SearchSort.Type -> compareValues(left.result.resource.kind.ordinal, right.result.resource.kind.ordinal)
            .takeUnless { it == 0 } ?: stable
    }
}

private fun compareStable(left: RankedResult, right: RankedResult): Int =
    compareValues(left.normalizedTitle, right.normalizedTitle)
        .takeUnless { it == 0 }
        ?: compareValues(left.result.resource.kind.ordinal, right.result.resource.kind.ordinal)
            .takeUnless { it == 0 }
        ?: compareValues(left.result.key, right.result.key)

private fun compareTime(left: Instant?, right: Instant?): Int = when {
    left != null && right != null -> left.compareTo(right)
    left != null -> -1
    right != null -> 1
    else -> 0
}

private fun SearchResource.toResult(relevance: Int, zoneId: ZoneId): SearchResult {
    val schedule = schedule(zoneId)
    val completedAt = completion(zoneId)
    return SearchResult(
        key = "${kind.keyPrefix}:$id",
        displayTitle = title(),
        supportingText = supportingText(schedule, completedAt),
        typeLabel = kind.typeLabel,
        schedule = schedule,
        completedAt = completedAt,
        resource = this,
        relevanceScore = relevance,
    )
}

private fun SearchResource.title(): String = when (this) {
    is SearchResource.LiveAction -> action.title
    is SearchResource.LiveEvent -> event.title
    is SearchResource.LiveRoutine -> routine.title
    is SearchResource.LiveMarker -> marker.title
    is SearchResource.LiveSignal -> signal.title
    is SearchResource.SavedAction -> template.title
    is SearchResource.SavedEvent -> template.title
}

private fun SearchResource.content(): String? = when (this) {
    is SearchResource.LiveAction -> action.content
    is SearchResource.LiveEvent -> event.content
    is SearchResource.LiveRoutine -> routine.content
    is SearchResource.LiveMarker -> marker.content
    is SearchResource.LiveSignal -> signal.content
    is SearchResource.SavedAction -> template.content
    is SearchResource.SavedEvent -> template.content
}

private fun SearchResource.schedule(zoneId: ZoneId): SearchScheduleMetadata? = when (this) {
    is SearchResource.LiveAction -> parseSchedule(action.start, zoneId)
    is SearchResource.LiveEvent -> parseSchedule(event.start, zoneId)
    is SearchResource.LiveRoutine -> parseSchedule(routine.target, zoneId)
    is SearchResource.LiveMarker -> {
        val start = parseDate(marker.date)
        val end = marker.endDate?.let(::parseDate) ?: start
        if (start == null || end == null || end < start) null
        else SearchScheduleMetadata.DateRange(start, end, zoneId)
    }

    is SearchResource.LiveSignal -> parseSchedule(signal.datetime, zoneId)
    is SearchResource.SavedAction -> template.naiveTime?.let(::parseLocalTime)
        ?.let(SearchScheduleMetadata::TimeOfDay)

    is SearchResource.SavedEvent -> null
}

private fun SearchResource.completion(zoneId: ZoneId): ZonedDateTime? = when (this) {
    is SearchResource.LiveAction -> action.completion?.let(::parseTimestamp)?.atZone(zoneId)
    else -> null
}

private fun SearchResource.supportingText(
    schedule: SearchScheduleMetadata?,
    completedAt: ZonedDateTime?,
): String? {
    val details = buildList {
        content()?.trim()?.takeIf(String::isNotEmpty)?.let(::add)
        if (this@supportingText is SearchResource.LiveRoutine) {
            add(
                when (routine.steps.size) {
                    0 -> "No steps"
                    1 -> "1 step"
                    else -> "${routine.steps.size} steps"
                },
            )
        }
        completedAt?.let { add("Completed ${it.format(DisplayDateTime)}") }
            ?: schedule?.displayLabel(this@supportingText)?.let(::add)
    }
    return details.joinToString(" · ").ifEmpty { null }
}

private fun SearchScheduleMetadata.displayLabel(resource: SearchResource): String = when (this) {
    is SearchScheduleMetadata.Timed -> {
        val prefix = when (resource) {
            is SearchResource.LiveAction -> if (resource.action.queued) "Scheduled" else "Due"
            is SearchResource.LiveRoutine -> "Target"
            is SearchResource.LiveSignal -> "At"
            else -> "Starts"
        }
        "$prefix ${local.format(DisplayDateTime)}"
    }

    is SearchScheduleMetadata.DateRange -> {
        val range = if (endDate == startDate) {
            startDate.format(DisplayDate)
        } else {
            "${startDate.format(DisplayDate)}–${endDate.format(DisplayDate)}"
        }
        when (resource) {
            is SearchResource.LiveAction -> "Due $range"
            is SearchResource.LiveRoutine -> "Target $range"
            else -> range
        }
    }

    is SearchScheduleMetadata.TimeOfDay -> "At ${localTime.format(DisplayTime)}"
}

private fun parseSchedule(value: String?, zoneId: ZoneId): SearchScheduleMetadata? {
    if (value.isNullOrBlank()) return null
    if ('T' !in value) {
        return parseDate(value)?.let { SearchScheduleMetadata.DateRange(it, zoneId = zoneId) }
    }
    val instant = parseTimestamp(value) ?: return null
    return SearchScheduleMetadata.Timed(value, instant, instant.atZone(zoneId))
}

private fun parseTimestamp(value: String): Instant? = runCatching { Instant.parse(value) }
    .recoverCatching { OffsetDateTime.parse(value).toInstant() }
    .getOrNull()

private fun parseDate(value: String): LocalDate? = runCatching { LocalDate.parse(value) }.getOrNull()

private fun parseLocalTime(value: String): LocalTime? = runCatching { LocalTime.parse(value) }.getOrNull()

private fun tokenize(value: String): List<String> = SearchTokenRegex.findAll(value.lowercase(Locale.ROOT))
    .map { it.value }
    .toList()

private fun normalize(value: String): String = tokenize(value).joinToString(" ")
