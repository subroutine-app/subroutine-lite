package com.example.subroutine_simple.sync

import com.example.subroutine_simple.auth.AuthenticationRecovery
import com.example.subroutine_simple.auth.AuthenticationState
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BackgroundSyncPolicyTest {
    @Test
    fun transientAuthFailureKeepsWorkOnlyForItsAccount() {
        val state = AuthenticationState.Error("Unavailable", "account-a", 2L, retryable = true)
        assertTrue(shouldRetryBackgroundSync(state, "account-a"))
        assertTrue(shouldRetryBackgroundSync(state, null))
        assertFalse(shouldRetryBackgroundSync(state, "account-b"))
    }

    @Test
    fun revokedOrSigningOutSessionsDoNotRetryAuthenticationInBackground() {
        assertFalse(shouldRetryBackgroundSync(AuthenticationState.SignedOut, null))
        assertFalse(
            shouldRetryBackgroundSync(
                AuthenticationState.InteractionRequired(null, "account-a", 2L),
                "account-a"
            )
        )
        assertFalse(shouldRetryBackgroundSync(AuthenticationState.Error("Storage unavailable"), null))
        assertFalse(
            shouldRetryBackgroundSync(
                AuthenticationState.Error(
                    "Sign out incomplete", "account-a", 2L, retryable = true,
                    recovery = AuthenticationRecovery.SignOut
                ),
                "account-a",
            )
        )
        assertFalse(shouldRetryBackgroundSync(AuthenticationState.SignedIn(null, "account-b", 3L), "account-a"))
    }
}
