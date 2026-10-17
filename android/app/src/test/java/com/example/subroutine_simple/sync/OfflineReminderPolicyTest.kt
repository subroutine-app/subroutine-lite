package com.example.subroutine_simple.sync

import com.example.subroutine_simple.auth.AuthenticationRecovery
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.reminders.canReconcileRemoteReminders
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OfflineReminderPolicyTest {
    @Test
    fun expiryAndTemporaryAuthFailureKeepRemindersBoundToTheSameOfflineAccount() {
        val expired = AuthenticationState.InteractionRequired(null, "account-a", 2L)
        val unavailable = AuthenticationState.Error("Offline", "account-a", 2L, retryable = true)
        assertTrue(canReconcileRemoteReminders(expired, "account-a"))
        assertTrue(canReconcileRemoteReminders(unavailable, "account-a"))
        assertFalse(canReconcileRemoteReminders(expired, "account-b"))
        assertFalse(canReconcileRemoteReminders(unavailable, "account-b"))
    }

    @Test
    fun signingOutDoesNotReactivateRetainedAccountReminders() {
        val signingOut = AuthenticationState.Error(
            "Sign out incomplete", "account-a", 3L,
            retryable = true, recovery = AuthenticationRecovery.SignOut
        )
        assertFalse(canReconcileRemoteReminders(signingOut, "account-a"))
        assertFalse(canReconcileRemoteReminders(AuthenticationState.SignedOut, "account-a"))
        assertFalse(canReconcileRemoteReminders(AuthenticationState.Error("Unavailable"), "account-a"))
    }
}
