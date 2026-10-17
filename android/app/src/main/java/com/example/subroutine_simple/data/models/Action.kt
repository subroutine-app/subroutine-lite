package com.example.subroutine_simple.data.models

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import java.time.Instant

@Serializable
data class Action(
    val id: String,
    @SerialName("recurrence_id") val recurrenceId: String,
    @SerialName("routine_id") val routineId: String? = null,
    @SerialName("template_id") val templateId: String? = null,
    val title: String,
    val content: String? = null,
    val queued: Boolean = false,
    val pinned: Boolean = false,
    val start: String? = null,
    val duration: String? = null,
    val completion: String? = null,
    val recurrence: JsonElement? = null,
    @SerialName("source_provider") val sourceProvider: String? = null,
    @SerialName("source_external_id") val sourceExternalId: String? = null,
)

val Action.isCompleted: Boolean
    get() = completion != null

val Action.isQueued: Boolean
    get() = queued && !isCompleted

val Action.isBacklogged: Boolean
    get() = !queued && !isCompleted

val Action.isScheduled: Boolean
    get() = isQueued && start != null

val Action.scheduledStart: String?
    get() = start?.takeIf { isQueued }

val Action.startInstant: Instant?
    get() = parseSchedulePoint(start)

val Action.durationSecs: Long?
    get() = parseIsoDurationSeconds(duration)

@Serializable
data class ActionTemplate(
    val id: String,
    @SerialName("sort_order") val sortOrder: Long = Long.MAX_VALUE,
    val title: String,
    val content: String? = null,
    @SerialName("naive_time") val naiveTime: String? = null,
    val duration: String? = null,
    val recurrence: JsonElement? = null,
)

@Serializable
data class EventTemplate(
    val id: String,
    @SerialName("lineage_id") val lineageId: String,
    @SerialName("sort_order") val sortOrder: Long = Long.MAX_VALUE,
    val title: String,
    val content: String? = null,
    val duration: String,
    val recurrence: JsonElement? = null,
    @SerialName("source_provider") val sourceProvider: String? = null,
    @SerialName("source_external_id") val sourceExternalId: String? = null,
    @SerialName("source_busy") val sourceBusy: Boolean? = null,
    @SerialName("busy_override") val busyOverride: Boolean? = null,
)

@Serializable
data class AllData(
    @SerialName("dataset_id") val datasetId: String = "00000000-0000-0000-0000-000000000000",
    val seq: Long = 0L,
    val actions: List<Action> = emptyList(),
    val events: List<Event> = emptyList(),
    val routines: List<Routine> = emptyList(),
    val markers: List<Marker> = emptyList(),
    val signals: List<Signal> = emptyList(),
    @SerialName("action_templates") val actionTemplates: List<ActionTemplate> = emptyList(),
    @SerialName("event_templates") val eventTemplates: List<EventTemplate> = emptyList(),
    @SerialName("marker_templates") val markerTemplates: List<MarkerTemplate> = emptyList(),
    @SerialName("signal_templates") val signalTemplates: List<SignalTemplate> = emptyList(),
)

@Serializable
data class CompleteResult(
    val completed: Action,
    val next: Action? = null,
)
