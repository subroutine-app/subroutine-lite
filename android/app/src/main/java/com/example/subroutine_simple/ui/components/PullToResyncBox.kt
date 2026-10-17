package com.example.subroutine_simple.ui.components

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.imePadding
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.movableContentOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics

@Composable
fun PullToResyncBox(
    isResyncing: Boolean,
    onResync: (() -> Unit)?,
    contentPadding: PaddingValues,
    modifier: Modifier = Modifier,
    content: @Composable BoxScope.() -> Unit,
) {
    val container = modifier
        .fillMaxSize()
        .padding(contentPadding)
        .consumeWindowInsets(contentPadding)
        .imePadding()
    val currentContent by rememberUpdatedState(content)
    val retainedContent = remember {
        movableContentOf<BoxScope> { scope -> currentContent(scope) }
    }
    if (onResync == null) {
        Box(modifier = container) { retainedContent(this) }
        return
    }
    val state = rememberPullToRefreshState()
    PullToRefreshBox(
        isRefreshing = isResyncing,
        onRefresh = { if (!isResyncing) onResync() },
        state = state,
        modifier = container
            .testTag("sync:pull-to-resync")
            .semantics {
                customActions = if (isResyncing) emptyList() else listOf(
                    CustomAccessibilityAction("Resync") {
                        onResync()
                        true
                    },
                )
            },
        indicator = {
            PullToRefreshDefaults.Indicator(
                isRefreshing = isResyncing,
                state = state,
                modifier = Modifier
                    .align(Alignment.TopCenter)
                    .testTag("sync:resync-indicator")
                    .clearAndSetSemantics {
                        contentDescription = if (isResyncing) "Resyncing" else "Pull to resync"
                    },
            )
        },
        content = { retainedContent(this) },
    )
}
