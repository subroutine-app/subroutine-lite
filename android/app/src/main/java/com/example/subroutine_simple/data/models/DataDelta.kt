package com.example.subroutine_simple.data.models

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class DataDelta(
    @SerialName("dataset_id")
    val datasetId: String = NIL_UUID,
    val seq: Long = 0L,
    val actions: List<Action> = emptyList(),
    val events: List<Event> = emptyList(),
    val routines: List<Routine> = emptyList(),
    @SerialName("routine_order")
    val routineOrder: List<String> = emptyList(),
    val markers: List<Marker> = emptyList(),
    val signals: List<Signal> = emptyList(),
    @SerialName("action_templates")
    val actionTemplates: List<ActionTemplate> = emptyList(),
    @SerialName("event_templates")
    val eventTemplates: List<EventTemplate> = emptyList(),
    @SerialName("marker_templates")
    val markerTemplates: List<MarkerTemplate> = emptyList(),
    @SerialName("signal_templates")
    val signalTemplates: List<SignalTemplate> = emptyList(),
    val tombstones: Tombstones = Tombstones(),
)

@Serializable
data class Tombstones(
    val actions: List<String> = emptyList(),
    val events: List<String> = emptyList(),
    val routines: List<String> = emptyList(),
    val markers: List<String> = emptyList(),
    val signals: List<String> = emptyList(),
    @SerialName("action_templates")
    val actionTemplates: List<String> = emptyList(),
    @SerialName("event_templates")
    val eventTemplates: List<String> = emptyList(),
    @SerialName("marker_templates")
    val markerTemplates: List<String> = emptyList(),
    @SerialName("signal_templates")
    val signalTemplates: List<String> = emptyList(),
)

private const val NIL_UUID = "00000000-0000-0000-0000-000000000000"
