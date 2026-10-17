package com.example.subroutine_simple.auth

import com.example.subroutine_simple.data.models.AccountInfo
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.supervisorScope
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AuthenticationSessionTest {
    @Test
    fun rejectionRetainsOfflineIdentityAndRevokesOldProvider() = authTest {
        val backend = Backend()
        val storage = MemoryStorage(SavedAuthentication(SavedSession(backend.identity, fresh)))
        val manager = AuthenticationManager(backend, storage, this) { 0L }
        manager.initialize()
        val bound = manager.sessionBoundProvider()
        val signedIn = manager.state.value as AuthenticationState.SignedIn

        bound.rejectSession(fresh.accessToken)

        val offline = manager.state.value as AuthenticationState.InteractionRequired
        assertEquals(signedIn.accountId, offline.accountId)
        assertTrue(offline.sessionGeneration > signedIn.sessionGeneration)
        assertNull(storage.value.session?.credentials)
        assertNull(manager.accessToken())
        assertTrue(runCatching { bound.accessToken() }.exceptionOrNull() is CancellationException)
    }

    @Test
    fun cancellingBrowserCompletionNeverInstallsItsLateAccount() = authTest {
        val backend = Backend()
        val storage = MemoryStorage()
        val manager = AuthenticationManager(backend, storage, this) { 0L }
        val submitted = CompletableDeferred<Unit>()
        val response = CompletableDeferred<Credentials>()
        backend.exchange = {
            submitted.complete(Unit)
            response.await()
        }
        manager.prepareSignIn("attempt")
        val completion = async { runCatching { manager.completeSignIn("attempt", "response") } }
        submitted.await()
        manager.cancelSignIn("attempt")
        response.complete(fresh)

        assertTrue(completion.await().exceptionOrNull() is CancellationException)
        assertEquals(AuthenticationState.SignedOut, manager.state.value)
        assertNull(storage.value.session)
        assertNull(storage.value.pending)
    }

    @Test
    fun rotatedTokenArrivingAfterSignOutCannotRestoreCredentials() = authTest {
        val backend = Backend()
        val storage = MemoryStorage(SavedAuthentication(SavedSession(backend.identity, expired)))
        val manager = AuthenticationManager(backend, storage, this) { 0L }
        val submitted = CompletableDeferred<Unit>()
        val response = CompletableDeferred<Credentials>()
        backend.refresh = {
            submitted.complete(Unit)
            response.await()
        }
        val acquisition = async { runCatching { manager.accessToken() } }
        submitted.await()
        assertNull(storage.value.session?.credentials)
        manager.signOut()
        response.complete(fresh)

        assertTrue(acquisition.await().exceptionOrNull() is CancellationException)
        assertEquals(SavedAuthentication(), storage.value)
        assertEquals(AuthenticationState.SignedOut, manager.state.value)
        assertNull(manager.accessToken())
    }

    @Test
    fun cancellingOneWaiterDoesNotLoseSharedRefreshRotation() = authTest {
        val backend = Backend()
        val storage = MemoryStorage(SavedAuthentication(SavedSession(backend.identity, expired)))
        val manager = AuthenticationManager(backend, storage, this) { 0L }
        val submitted = CompletableDeferred<Unit>()
        val response = CompletableDeferred<Credentials>()
        backend.refresh = {
            submitted.complete(Unit)
            response.await()
        }
        val waiter = async { manager.accessToken() }
        submitted.await()
        waiter.cancelAndJoin()
        response.complete(fresh)

        assertEquals(fresh.accessToken, manager.accessToken())
        assertEquals(fresh.accessToken, storage.value.session?.credentials?.accessToken)
        assertEquals(1, backend.refreshCount)
    }

    @Test
    fun explicitTemporaryRefreshRefusalRemainsRetryableWithoutDroppingOfflineAccount() = authTest {
        val backend = Backend()
        val storage = MemoryStorage(SavedAuthentication(SavedSession(backend.identity, expired)))
        val manager = AuthenticationManager(backend, storage, this) { 0L }
        backend.refresh = { throw AuthenticationUnavailableException() }
        manager.initialize()

        val unavailable = manager.state.value as AuthenticationState.Error
        assertEquals(backend.identity.accountId, unavailable.accountId)
        assertTrue(unavailable.retryable)
        assertEquals(expired.accessToken, storage.value.session?.credentials?.accessToken)
        backend.refresh = { fresh }
        manager.initialize()
        assertEquals(fresh.accessToken, manager.accessToken())
        assertEquals(2, backend.refreshCount)
    }

    private class MemoryStorage(var value: SavedAuthentication = SavedAuthentication()) : AuthenticationStorage {
        override fun read() = value
        override fun write(value: SavedAuthentication) {
            this.value = value
        }
    }

    private class Backend : AuthenticationBackend {
        val identity = AccountInfo(UUID.randomUUID().toString(), UUID.randomUUID().toString())
        var refreshCount = 0
        var exchange: suspend () -> Credentials = { fresh }
        var refresh: suspend () -> Credentials = { fresh }
        override suspend fun authorizationRequest() = "request"
        override suspend fun exchange(request: String, response: String) = exchange()
        override suspend fun refresh(credentials: Credentials, beforeSubmit: () -> Unit): Credentials {
            refreshCount += 1
            beforeSubmit()
            return refresh()
        }

        override suspend fun account(credentials: Credentials) = identity
    }

    private companion object {
        val fresh = Credentials("serialized-fresh", "fresh", 120_000L, true)
        val expired = Credentials("serialized-expired", "expired", 0L, true)
        fun authTest(block: suspend CoroutineScope.() -> Unit) = runBlocking {
            withTimeout(5_000) { supervisorScope(block) }
        }
    }
}
