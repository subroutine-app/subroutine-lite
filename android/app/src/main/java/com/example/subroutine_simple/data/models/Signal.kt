package com.example.subroutine_simple.data.models

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import java.time.Instant

@Serializable
data class Signal(
    val id: String,
    @SerialName("lineage_id") val lineageId: String,
    @SerialName("template_id") val templateId: String? = null,
    val title: String,
    val content: String? = null,
    val datetime: String,
    val recurrence: JsonElement? = null,
)

val Signal.datetimeInstant: Instant?
    get() = parseSchedulePoint(datetime)

@Serializable
data class SignalTemplate(
    val id: String,
    @SerialName("lineage_id") val lineageId: String,
    val title: String,
    val content: String? = null,
    val recurrence: JsonElement? = null,
)
