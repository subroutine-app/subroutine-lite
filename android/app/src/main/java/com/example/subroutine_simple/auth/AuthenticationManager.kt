package com.example.subroutine_simple.auth

import android.app.Activity
import android.content.Context
import android.content.Intent
import com.example.subroutine_simple.data.models.AccountInfo
import java.io.IOException
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext

class AuthenticationUnavailableException(message: String = "Authentication service unavailable. Retrying shortly.") :
    IOException(message)

internal class AuthenticationRequiredException : IOException("Sign in again to synchronize.")

internal class AuthenticationIndeterminateException : IOException("Token request outcome is unknown. Sign in again to synchronize.")

interface AccessTokenProvider {
    suspend fun accessToken(forceRefresh: Boolean = false, rejectedAccessToken: String? = null): String?
    fun rejectSession()
    fun rejectSession(rejectedAccessToken: String?) = rejectSession()
}

enum class AuthenticationRecovery { Reconnect, SignOut }

sealed interface AuthenticationState {
    data object Initializing : AuthenticationState
    data object SignedOut : AuthenticationState
    data class InteractionRequired(val username: String?, val accountId: String, val sessionGeneration: Long) :
        AuthenticationState

    data class SignedIn(val username: String?, val accountId: String, val sessionGeneration: Long) : AuthenticationState
    data class Error(
        val message: String,
        val accountId: String? = null,
        val sessionGeneration: Long? = null,
        val retryable: Boolean = false,
        val recovery: AuthenticationRecovery = AuthenticationRecovery.Reconnect,
    ) : AuthenticationState
}

internal interface AuthenticationBackend {
    suspend fun authorizationRequest(): String
    suspend fun exchange(request: String, response: String): Credentials
    suspend fun refresh(credentials: Credentials, beforeSubmit: () -> Unit): Credentials
    suspend fun account(credentials: Credentials): AccountInfo
}

class AuthenticationManager internal constructor(
    private val backend: AuthenticationBackend? = null,
    private val storage: AuthenticationStorage? = null,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO),
    private val now: () -> Long = System::currentTimeMillis,
) : AccessTokenProvider {
    private constructor(context: Context, configuration: RauthyConfiguration?) : this(
        configuration?.let { AppAuthBackend(context.applicationContext, it) },
        configuration?.let { KeystoreAuthenticationStorage(context.applicationContext, it.storageBinding) },
    )

    constructor(context: Context) : this(context, RauthyConfiguration.fromBuild())

    private enum class Persistence { Durable, RotationPending, DeletionPending }

    private val lock = Any()
    private var persistence = Persistence.Durable
    private var epoch = 0L
    private var saved = SavedAuthentication()
    private var loaded = backend == null
    private var needsVerification = false
    private var restoreFlight: Deferred<Unit>? = null
    private var tokenFlight: Pair<Long, Deferred<String?>>? = null
    private var completionFlight: Pair<String, Deferred<Unit>>? = null
    private var activeAttempt: String? = null

    private val mutableState = MutableStateFlow<AuthenticationState>(
        if (backend == null) AuthenticationState.SignedOut else AuthenticationState.Initializing,
    )
    val state: StateFlow<AuthenticationState> = mutableState.asStateFlow()
    val supportsSignIn: Boolean get() = synchronized(lock) {
        backend != null && persistence != Persistence.DeletionPending
    }

    suspend fun initialize() {
        restore()
        try {
            accessToken()
        } catch (_: AuthenticationUnavailableException) {
        }
    }

    private suspend fun restore() {
        val flight = synchronized(lock) {
            if (loaded) return
            restoreFlight ?: scope.async {
                synchronized(lock) {
                    if (!loaded) {
                        try {
                            saved = requireNotNull(storage).read()
                            if (activeAttempt == null) activeAttempt = saved.pending?.id
                            needsVerification = saved.session?.credentials != null
                            publishRestored()
                        } catch (_: IOException) {
                            mutableState.value = AuthenticationState.Error(
                                "Saved sign-in is unavailable. Sign in again; offline workspaces are retained.",
                            )
                        }
                        loaded = true
                    }
                }
            }.also { restoreFlight = it }
        }
        flight.await()
    }

    private fun publishRestored() {
        val session = saved.session
        mutableState.value = when {
            session == null -> AuthenticationState.SignedOut
            session.credentials == null -> AuthenticationState.InteractionRequired(null, session.account.accountId, epoch)
            else -> AuthenticationState.Error(
                "Reconnecting to your account. Saved data remains available.",
                session.account.accountId, epoch, retryable = true,
            )
        }
    }

    suspend fun signIn(activity: Activity? = null) {
        currentCoroutineContext().ensureActive()
        if (!supportsSignIn) throw AuthenticationUnavailableException("Sign-in is unavailable.")
        val host = activity ?: throw AuthenticationUnavailableException("Sign-in needs an active window.")
        host.startActivity(
            Intent(host, AuthorizationActivity::class.java)
                .putExtra(AuthorizationActivity.ATTEMPT_ID, UUID.randomUUID().toString()),
        )
    }

    internal suspend fun prepareSignIn(id: String): String {
        val captured = synchronized(lock) {
            checkNotDeleting()
            activeAttempt = id
            epoch
        }
        restore()
        return scope.async {
            synchronized(lock) {
                checkEpoch(captured)
                if (activeAttempt != id) throw CancellationException("Sign-in was superseded")
                saved.pending?.takeIf { it.id == id && it.isFresh(now()) }?.request?.let { return@async it }
                persist(saved.copy(pending = PendingAuthorization(id, null, now())))
            }
            val request = requireNotNull(backend).authorizationRequest()
            synchronized(lock) {
                checkAttempt(id, captured)
                persist(saved.copy(pending = PendingAuthorization(id, request, now())))
            }
            request
        }.await()
    }

    internal fun authorizationIntent(request: String): Intent =
        (backend as AppAuthBackend).authorizationIntent(request)

    internal suspend fun hasPendingSignIn(id: String): Boolean {
        restore()
        return synchronized(lock) {
            activeAttempt == id && saved.pending?.let { it.id == id && it.isFresh(now()) } == true
        }
    }

    internal suspend fun cancelSignIn(id: String) {
        restore()
        withContext(Dispatchers.IO) {
            synchronized(lock) {
                if (activeAttempt == id) activeAttempt = null
                if (saved.pending?.id == id) persist(saved.copy(pending = null))
            }
        }
    }

    internal suspend fun completeSignIn(id: String, response: String) {
        restore()
        val flight = synchronized(lock) {
            completionFlight?.takeIf { it.first == id }?.second ?: scope.async {
                val (captured, request) = synchronized(lock) {
                    checkAttempt(id, epoch)
                    val request = saved.pending?.request ?: throw AuthenticationRequiredException()
                    persist(saved.copy(pending = null))
                    epoch to request
                }
                val credentials = requireNotNull(backend).exchange(request, response)
                val account = backend.account(credentials)
                synchronized(lock) {
                    checkEpoch(captured)
                    if (activeAttempt != id) throw CancellationException("Sign-in was superseded")
                    persist(SavedAuthentication(SavedSession(account, credentials)))
                    epoch += 1
                    activeAttempt = null
                    needsVerification = false
                    mutableState.value = AuthenticationState.SignedIn(null, account.accountId, epoch)
                }
            }.also { completionFlight = id to it }
        }
        flight.await()
    }

    suspend fun signOut() {
        currentCoroutineContext().ensureActive()
        val captured = synchronized(lock) {
            if (persistence != Persistence.DeletionPending) {
                epoch += 1
                loaded = true
                saved = SavedAuthentication(session = saved.session?.copy(credentials = null))
                activeAttempt = null
                needsVerification = false
                persistence = Persistence.DeletionPending
            }
            mutableState.value = AuthenticationState.Error(
                "Sign-out is incomplete. Sync is blocked, but saved credentials may remain on this device. Retry Sign out.",
                saved.session?.account?.accountId, epoch, retryable = true,
                recovery = AuthenticationRecovery.SignOut,
            )
            epoch
        }
        withContext(Dispatchers.IO) {
            synchronized(lock) {
                checkEpoch(captured)
                if (persistence == Persistence.DeletionPending) {
                    storage?.write(SavedAuthentication())
                    saved = SavedAuthentication()
                    persistence = Persistence.Durable
                    mutableState.value = AuthenticationState.SignedOut
                }
            }
        }
    }

    override suspend fun accessToken(forceRefresh: Boolean, rejectedAccessToken: String?): String? {
        val captured = sessionEpoch()
        restore()
        return acquire(captured, forceRefresh, rejectedAccessToken)
    }

    private suspend fun acquire(captured: Long, force: Boolean, rejected: String?): String? {
        currentCoroutineContext().ensureActive()
        val flight = synchronized(lock) {
            checkEpoch(captured)
            if (persistence == Persistence.DeletionPending) return null
            tokenFlight?.takeIf { it.first == captured }?.second?.let { return@synchronized it }
            val credentials = saved.session?.credentials ?: return null
            val rejectedCurrent = force && (rejected == null || rejected == credentials.accessToken)
            val fresh = credentials.isFresh(now())
            if (persistence == Persistence.Durable && rejectedCurrent && fresh) {
                expireSession()
                return null
            }
            if (persistence == Persistence.Durable && !needsVerification && fresh &&
                mutableState.value is AuthenticationState.SignedIn
            ) return credentials.accessToken
            scope.async {
                try {
                    refreshAndVerify(captured)
                } finally {
                    synchronized(lock) {
                        if (tokenFlight?.first == captured) tokenFlight = null
                    }
                }
            }.also { tokenFlight = captured to it }
        }
        val token = flight.await() ?: return null
        synchronized(lock) { checkEpoch(captured) }
        return token
    }

    private suspend fun refreshAndVerify(captured: Long): String? {
        try {
            val session = synchronized(lock) {
                checkEpoch(captured)
                checkNotDeleting()
                if (persistence == Persistence.RotationPending) persist(saved)
                requireNotNull(saved.session)
            }
            var credentials = session.credentials ?: return null
            if (!credentials.isFresh(now())) {
                if (!credentials.canRefresh) throw AuthenticationRequiredException()
                credentials = refresh(session, captured)
                synchronized(lock) {
                    checkEpoch(captured)
                    saved = saved.copy(session = session.copy(credentials = credentials))
                    persistence = Persistence.RotationPending
                    needsVerification = true
                    persist(saved)
                }
            }
            val account = requireNotNull(backend).account(credentials)
            if (account.accountId != session.account.accountId) throw AuthenticationRequiredException()
            return synchronized(lock) {
                checkEpoch(captured)
                persist(saved.copy(session = SavedSession(account, credentials)))
                needsVerification = false
                mutableState.value = AuthenticationState.SignedIn(null, account.accountId, captured)
                credentials.accessToken
            }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (error: Exception) {
            synchronized(lock) {
                checkEpoch(captured)
                if (error is AuthenticationRequiredException || error is AuthenticationIndeterminateException ||
                    saved.session?.credentials == null
                ) {
                    expireSession()
                    return null
                }
                val account = saved.session?.account?.accountId
                val message = if (persistence == Persistence.RotationPending) {
                    "Could not save refreshed credentials. Sync is blocked until secure storage succeeds. Saved data remains editable."
                } else {
                    "Authentication is temporarily unavailable. Saved data remains editable."
                }
                mutableState.value = AuthenticationState.Error(message, account, captured, retryable = true)
            }
            throw AuthenticationUnavailableException()
        }
    }

    private suspend fun refresh(session: SavedSession, captured: Long): Credentials {
        return try {
            requireNotNull(backend).refresh(requireNotNull(session.credentials)) {
                synchronized(lock) {
                    checkEpoch(captured)
                    persist(saved.copy(session = session.copy(credentials = null)))
                    needsVerification = true
                }
            }
        } catch (unavailable: AuthenticationUnavailableException) {
            synchronized(lock) {
                checkEpoch(captured)
                if (saved.session?.credentials == null) persist(saved.copy(session = session))
            }
            throw unavailable
        }
    }

    override fun rejectSession() = rejectSession(null)

    override fun rejectSession(rejectedAccessToken: String?) {
        reject(sessionEpoch(), rejectedAccessToken)
    }

    private fun reject(captured: Long, rejected: String?) {
        synchronized(lock) {
            if (epoch != captured || persistence == Persistence.DeletionPending) return
            if (rejected != null && saved.session?.credentials?.accessToken != rejected) return
            expireSession()
        }
    }

    private fun expireSession() {
        epoch += 1
        saved = saved.copy(session = saved.session?.copy(credentials = null))
        persistence = Persistence.Durable
        needsVerification = false
        publishRestored()
        try {
            storage?.write(saved)
        } catch (_: IOException) {
            mutableState.value = AuthenticationState.Error(
                "Credentials were blocked but could not be removed from storage. Sign out and retry.",
                saved.session?.account?.accountId, epoch,
            )
        }
    }

    fun sessionEpoch(): Long = synchronized(lock) { epoch }

    fun isCurrentSession(accountId: String, generation: Long): Boolean = synchronized(lock) {
        epoch == generation && (mutableState.value as? AuthenticationState.SignedIn)?.accountId == accountId
    }

    fun sessionBoundProvider(): AccessTokenProvider {
        val captured = synchronized(lock) { saved.session?.let { epoch } }
        return object : AccessTokenProvider {
            override suspend fun accessToken(forceRefresh: Boolean, rejectedAccessToken: String?): String? {
                currentCoroutineContext().ensureActive()
                return captured?.let { acquire(it, forceRefresh, rejectedAccessToken) }
            }

            override fun rejectSession() = rejectSession(null)
            override fun rejectSession(rejectedAccessToken: String?) {
                if (captured != null) reject(captured, rejectedAccessToken)
            }
        }
    }

    private fun checkEpoch(captured: Long) {
        if (epoch != captured) throw CancellationException("Authentication session changed")
    }

    private fun checkAttempt(id: String, captured: Long) {
        checkEpoch(captured)
        if (activeAttempt != id || saved.pending?.let { it.id == id && it.isFresh(now()) } != true) {
            throw CancellationException("Sign-in expired or was superseded")
        }
    }

    private fun checkNotDeleting() {
        if (persistence == Persistence.DeletionPending) {
            throw AuthenticationUnavailableException("Finish signing out before signing in again.")
        }
    }

    private fun persist(value: SavedAuthentication) {
        checkNotDeleting()
        requireNotNull(storage).write(value)
        saved = value
        persistence = Persistence.Durable
    }
}
