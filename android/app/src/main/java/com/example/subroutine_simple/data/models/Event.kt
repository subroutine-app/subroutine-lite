package com.example.subroutine_simple.data.models

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import java.time.Instant

@Serializable
data class Event(
    val id: String,
    @SerialName("lineage_id") val lineageId: String,
    @SerialName("template_id") val templateId: String? = null,
    val title: String,
    val content: String? = null,
    val start: String,
    val duration: String,
    val recurrence: JsonElement? = null,
    @SerialName("source_provider") val sourceProvider: String? = null,
    @SerialName("source_external_id") val sourceExternalId: String? = null,
    @SerialName("source_busy") val sourceBusy: Boolean? = null,
    @SerialName("busy_override") val busyOverride: Boolean? = null,
)

val Event.blocksTime: Boolean
    get() = busyOverride ?: sourceBusy ?: true

fun Event.asTemplate(id: String): EventTemplate = EventTemplate(
    id = id,
    lineageId = lineageId,
    title = title,
    content = content,
    duration = duration,
    recurrence = recurrence,
    sourceProvider = sourceProvider,
    sourceExternalId = sourceExternalId,
    sourceBusy = sourceBusy,
    busyOverride = busyOverride,
)

val Event.startInstant: Instant?
    get() = parseSchedulePoint(start)

val Event.durationSecs: Long
    get() = parseIsoDurationSeconds(duration) ?: 0L

val Event.isNotFullyPassed: Boolean
    get() {
        val end = startInstant?.plusSeconds(durationSecs) ?: return false
        return end > Instant.now()
    }
