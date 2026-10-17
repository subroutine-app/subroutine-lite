package com.example.subroutine_simple

import androidx.navigation3.runtime.NavKey
import kotlinx.serialization.Serializable

@Serializable
data object MainRoute : NavKey

@Serializable
data class FocusRoute(val mode: com.example.subroutine_simple.ui.focus.FocusMode = com.example.subroutine_simple.ui.focus.FocusMode.Actions) :
    NavKey

@Serializable
data object BacklogRoute : NavKey


@Serializable
data object SettingsRoute : NavKey

@Serializable
data object SearchRoute : NavKey

@Serializable
data object RoutinesRoute : NavKey

@Serializable
data object TemplatesRoute : NavKey

@Serializable
data class CalendarRoute(val initialDate: String? = null) : NavKey

@Serializable
data class EditActionRoute(val actionId: String) : NavKey

@Serializable
data class EditEventRoute(val eventId: String) : NavKey

@Serializable
data class EditMarkerRoute(val markerId: String) : NavKey

@Serializable
data class EditSignalRoute(val signalId: String) : NavKey

@Serializable
data class EditRoutineRoute(val routineId: String) : NavKey

@Serializable
data class EditActionTemplateRoute(val templateId: String) : NavKey

@Serializable
data class EditEventTemplateRoute(val templateId: String) : NavKey

internal fun <T> MutableList<T>.popIfCurrent(expected: T): Boolean {
    if (size <= 1 || lastOrNull() != expected) return false
    removeAt(lastIndex)
    return true
}
