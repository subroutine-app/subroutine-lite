package com.example.subroutine_simple.data.models

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import java.time.LocalDate
import java.time.temporal.ChronoUnit

@Serializable
data class Marker(
    val id: String,
    @SerialName("lineage_id") val lineageId: String,
    @SerialName("template_id") val templateId: String? = null,
    val title: String,
    val content: String? = null,
    val date: String,
    @SerialName("end_date") val endDate: String? = null,
    val recurrence: JsonElement? = null,
    @SerialName("source_provider") val sourceProvider: String? = null,
    @SerialName("source_external_id") val sourceExternalId: String? = null,
)

val Marker.isMultiDay: Boolean
    get() = endDate != null

fun Marker.daysUntil(today: LocalDate = LocalDate.now()): Long {
    return try {
        val markerDate = LocalDate.parse(date)
        ChronoUnit.DAYS.between(today, markerDate)
    } catch (_: Exception) {
        0L
    }
}

@Serializable
data class MarkerTemplate(
    val id: String,
    @SerialName("lineage_id") val lineageId: String,
    val title: String,
    val content: String? = null,
    @SerialName("span_days") val spanDays: Int = 1,
    val recurrence: JsonElement? = null,
    @SerialName("source_provider") val sourceProvider: String? = null,
    @SerialName("source_external_id") val sourceExternalId: String? = null,
)
