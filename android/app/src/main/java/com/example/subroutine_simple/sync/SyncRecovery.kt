package com.example.subroutine_simple.sync

import java.io.IOException
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.auth.AuthenticationRecovery
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import retrofit2.HttpException

internal fun isRetryableSyncFailure(error: Throwable): Boolean = when (error) {
    is HttpException -> error.code() == 408 || error.code() == 429 || error.code() in 500..599
    is IOException -> true
    else -> false
}

internal fun syncRetryDelay(attempt: Long): Long = 1_000L shl attempt.coerceIn(0, 4).toInt()

internal fun shouldRetryBackgroundSync(state: AuthenticationState, expectedAccountId: String?): Boolean {
    val accountId = when (state) {
        is AuthenticationState.SignedIn -> state.accountId
        is AuthenticationState.Error -> state.accountId.takeIf {
            state.retryable && state.recovery == AuthenticationRecovery.Reconnect
        }

        else -> null
    } ?: return false
    return expectedAccountId == null || expectedAccountId == accountId
}

internal fun canRetryInitialization(state: AuthenticationState, networkAvailable: Boolean): Boolean =
    networkAvailable && state is AuthenticationState.Error && state.retryable &&
            state.recovery == AuthenticationRecovery.Reconnect

internal suspend fun periodicallyRecoverSync(
    pause: suspend (Long) -> Unit = { delay(it) },
    recover: () -> Unit,
) {
    while (true) {
        pause(60_000L)
        recover()
    }
}

internal class SyncRecovery(
    private val scope: CoroutineScope,
    private val pause: suspend (Long) -> Unit = { delay(it) },
    private val onIdle: () -> Unit = {},
    private val canRetry: () -> Boolean = { true },
) {
    private var job: Job? = null
    private var pending: (suspend () -> Boolean)? = null

    fun request(attempt: suspend () -> Boolean) {
        pending = attempt
        if (job?.isActive == true) return
        val owner = scope.launch(start = CoroutineStart.LAZY) {
            try {
                var current = attempt
                repeat(4) { index ->
                    if (index > 0 && !canRetry()) return@launch
                    current = pending ?: current
                    pending = null
                    val retry = current()
                    if (!retry && pending == null) return@launch
                    if (index < 3 && retry) pause(syncRetryDelay(index.toLong()))
                }
            } finally {
                if (job === kotlinx.coroutines.currentCoroutineContext()[Job]) {
                    job = null
                    pending = null
                    onIdle()
                }
            }
        }
        job = owner
        owner.start()
    }

    fun stop() {
        job?.cancel()
        job = null
        pending = null
        onIdle()
    }
}
