package com.example.subroutine_simple.ui

import com.example.subroutine_simple.auth.AuthenticationState

internal fun shouldShowWorkspaceLoading(
    authentication: AuthenticationState,
    hasContent: Boolean,
    syncStatus: SyncStatus,
    initializing: Boolean,
): Boolean = authentication == AuthenticationState.Initializing ||
        (!hasContent && (initializing || syncStatus == SyncStatus.InitialLoading || syncStatus == SyncStatus.Refreshing))
