package com.example.subroutine_simple.ui.home

import com.example.subroutine_simple.auth.AuthenticationRecovery
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor

internal fun homeWorkspaceWritable(
    authentication: AuthenticationState,
    workspace: WorkspaceDescriptor?,
    localOnly: Boolean,
): Boolean = when (workspace) {
    is WorkspaceDescriptor.Local -> localOnly && authentication == AuthenticationState.SignedOut
    is WorkspaceDescriptor.Remote -> !localOnly && when (authentication) {
        is AuthenticationState.SignedIn -> authentication.accountId == workspace.accountId
        is AuthenticationState.InteractionRequired -> authentication.accountId == workspace.accountId
        is AuthenticationState.Error -> authentication.accountId == workspace.accountId &&
                authentication.recovery != AuthenticationRecovery.SignOut

        else -> false
    }

    null -> false
}

internal fun homeCompletionAvailable(action: Action, localOnly: Boolean): Boolean =
    action.completion == null && (!localOnly || action.recurrence == null)
