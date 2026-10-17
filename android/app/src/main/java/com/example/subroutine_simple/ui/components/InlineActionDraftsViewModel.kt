package com.example.subroutine_simple.ui.components

import android.os.Bundle
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel

val LocalInlineActionDrafts = staticCompositionLocalOf<InlineActionDraftsViewModel> {
    error("Provide activity-scoped inline action drafts above authenticated navigation")
}

class InlineActionDraftsViewModel(private val savedState: SavedStateHandle) : ViewModel() {
    private val drafts = mutableMapOf<String, InlineActionCreatorState>()

    // Keys include the workspace and dataset, plus the creation surface/date.
    fun draft(key: String): InlineActionCreatorState = drafts.getOrPut(key) {
        val storageKey = "inline-draft:$key"
        InlineActionCreatorState(key, savedState.get<Bundle>(storageKey)) { value ->
            if (value.getBoolean("expanded") || !value.getString("title").isNullOrEmpty()) {
                savedState[storageKey] = value
            } else {
                savedState.remove<Bundle>(storageKey)
            }
        }
    }
}
