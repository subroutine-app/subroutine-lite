package com.example.subroutine_simple.ui

import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.sync.AvailableNetworkTracker
import org.junit.Assert.*
import org.junit.Test

class WorkspaceLoadingTest {
    @Test
    fun restoringCredentialsAndCacheDoNotShowAnAuthenticationFailure() {
        val restoring = AuthenticationState.Error("Reconnecting", "account", 0, retryable = true)
        assertTrue(shouldShowWorkspaceLoading(restoring, false, SyncStatus.Offline, initializing = true))
        val expired = AuthenticationState.InteractionRequired(null, "account", 0)
        assertTrue(shouldShowWorkspaceLoading(expired, false, SyncStatus.InitialLoading, initializing = false))
        assertFalse(shouldShowWorkspaceLoading(expired, false, SyncStatus.AuthenticationRequired, initializing = false))
        assertFalse(shouldShowWorkspaceLoading(restoring, true, SyncStatus.Offline, initializing = true))
    }

    @Test
    fun loadingAnEmptyLocalOrRemoteWorkspaceUsesTheSameLoadingState() {
        val signedIn = AuthenticationState.SignedIn(null, "account", 0)
        assertTrue(shouldShowWorkspaceLoading(signedIn, false, SyncStatus.InitialLoading, initializing = false))
        assertTrue(shouldShowWorkspaceLoading(AuthenticationState.SignedOut, false, SyncStatus.InitialLoading, false))
        assertFalse(shouldShowWorkspaceLoading(signedIn, true, SyncStatus.Ready, initializing = false))
        assertFalse(shouldShowWorkspaceLoading(signedIn, false, SyncStatus.Offline, initializing = false))
    }

    @Test
    fun initialNetworkSnapshotCannotOverwriteANewerCallback() {
        val tracker = AvailableNetworkTracker<String>()
        tracker.initialize("wifi")
        assertEquals("wifi", tracker.availableNetwork)
        tracker.available("mobile")
        tracker.initialize("wifi")
        tracker.lost("wifi")
        assertEquals("mobile", tracker.availableNetwork)
        tracker.lost("mobile")
        tracker.initialize("wifi")
        assertNull(tracker.availableNetwork)
    }
}
