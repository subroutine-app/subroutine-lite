package com.example.subroutine_simple.data.models

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

@Serializable
data class RoutineStep(
    val title: String,
    val duration: String? = null,
)

@Serializable
data class Routine(
    val id: String,
    @SerialName("recurrence_id") val recurrenceId: String = "00000000-0000-0000-0000-000000000000",
    val title: String,
    val content: String? = null,
    val target: String? = null,
    val steps: List<RoutineStep> = emptyList(),
    val recurrence: JsonElement? = null,
)
