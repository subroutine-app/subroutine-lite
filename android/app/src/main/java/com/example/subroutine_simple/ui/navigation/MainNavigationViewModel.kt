package com.example.subroutine_simple.ui.navigation

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import java.time.LocalDate
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

// Names, rather than tab positions, are persisted across navigation changes.
enum class MainTab { Home, Timeline, Upcoming, Browse }

data class MainNavigationUiState(
    val selectedTopLevelTab: MainTab = MainTab.Home,
    val timelineDate: LocalDate,
)

class MainNavigationViewModel(private val savedState: SavedStateHandle) : ViewModel() {
    private val _uiState = MutableStateFlow(
        MainNavigationUiState(
            selectedTopLevelTab = MainTab.entries.firstOrNull { it.name == savedState.get<String>("tab") }
                ?: MainTab.Home,
            timelineDate = savedState.get<String>("timelineDate")
                ?.let { runCatching { LocalDate.parse(it) }.getOrNull() } ?: LocalDate.now(),
        ),
    )
    val uiState: StateFlow<MainNavigationUiState> = _uiState.asStateFlow()

    fun selectTopLevelTab(tab: MainTab) = update(_uiState.value.copy(selectedTopLevelTab = tab))

    fun setTimelineDate(date: LocalDate) = update(_uiState.value.copy(timelineDate = date))

    fun showTimeline(date: LocalDate) = update(MainNavigationUiState(MainTab.Timeline, date))

    private fun update(state: MainNavigationUiState) {
        savedState["tab"] = state.selectedTopLevelTab.name
        savedState["timelineDate"] = state.timelineDate.toString()
        _uiState.value = state
    }
}
