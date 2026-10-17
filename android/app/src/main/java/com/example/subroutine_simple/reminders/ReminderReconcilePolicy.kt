package com.example.subroutine_simple.reminders

import com.example.subroutine_simple.auth.AuthenticationRecovery
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor

internal fun canReconcileRemoteReminders(state: AuthenticationState, accountId: String): Boolean = when (state) {
    is AuthenticationState.SignedIn -> state.accountId == accountId
    is AuthenticationState.InteractionRequired -> state.accountId == accountId
    is AuthenticationState.Error -> state.accountId == accountId && state.sessionGeneration != null &&
            state.recovery == AuthenticationRecovery.Reconnect

    else -> false
}

internal enum class ReminderWorkspaceChange { None, PreviewOnly, Workspace }

internal fun reminderWorkspaceChange(
    previous: WorkspaceDescriptor?,
    next: WorkspaceDescriptor,
    previousShowTitle: Boolean,
    nextShowTitle: Boolean,
): ReminderWorkspaceChange = when {
    !next.sameWorkspaceAs(previous) -> ReminderWorkspaceChange.Workspace
    previousShowTitle != nextShowTitle -> ReminderWorkspaceChange.PreviewOnly
    else -> ReminderWorkspaceChange.None
}
