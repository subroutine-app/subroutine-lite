package com.example.subroutine_simple.sync

import android.content.Context
import com.example.subroutine_simple.auth.AuthenticationManager
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.data.local.DatasetChangedWithPendingIntentException
import com.example.subroutine_simple.data.local.DatasetMismatchException
import com.example.subroutine_simple.data.local.ReminderCommandStatus

import com.example.subroutine_simple.data.network.RetrofitClient
import com.example.subroutine_simple.data.network.SubroutineApi
import com.example.subroutine_simple.data.repository.RepositorySession
import com.example.subroutine_simple.data.repository.SubroutineRepository
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor

import com.example.subroutine_simple.reminders.ReminderCommandResult
import com.example.subroutine_simple.reminders.ReminderOperation

import com.example.subroutine_simple.reminders.LocalReminderStore
import com.example.subroutine_simple.reminders.ReminderScheduler
import java.time.Instant
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow

object AppServices {
    @Volatile private var instance: Services? = null
    private val changes = MutableSharedFlow<Unit>(extraBufferCapacity = 1, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val workspaceChanges = changes.asSharedFlow()

    private fun services(context: Context): Services = instance ?: synchronized(this) {
        instance ?: Services(context.applicationContext).also { instance = it }
    }

    fun authentication(context: Context): AuthenticationManager = services(context).authentication
    fun network(context: Context): RetrofitClient = services(context).network
    fun repository(context: Context): SubroutineRepository = services(context).repository
    fun notifyWorkspaceChanged() { changes.tryEmit(Unit) }


    internal suspend fun executeLocalReminder(
        context: Context,
        workspace: WorkspaceDescriptor,
        actionId: String,
        expectedStart: String,
        operation: ReminderOperation,
        commandId: String,
        requestedAt: Instant,
    ): ReminderCommandResult {
        val auth = authentication(context)
        if (auth.state.value == AuthenticationState.Initializing) auth.initialize()
        val identity = if (workspace is WorkspaceDescriptor.Remote) {
            accountIdentity(auth.state.value) ?: return ReminderCommandResult.AuthenticationRequired
        } else null
        val recorded = LocalReminderStore.get(context).localCommand(commandId)?.command
            ?: return ReminderCommandResult.Superseded
        if (workspace is WorkspaceDescriptor.Remote && recorded.datasetId == null) {
            return ReminderCommandResult.Superseded
        }
        return execute(context, workspace, identity, commandId, recorded.datasetId, actionId, expectedStart,
            operation, requestedAt) { ReminderScheduler.isCurrentCommand(context, commandId) }
    }

    suspend fun reminderCommandStatus(
        context: Context,
        commandId: String,
        workspace: WorkspaceDescriptor,
        authenticationIdentity: String?,
    ): ReminderCommandStatus? {
        val auth = authentication(context)
        return try {
            if (auth.state.value == AuthenticationState.Initializing) auth.initialize()
            val identity = if (workspace is WorkspaceDescriptor.Remote) {
                val current = accountIdentity(auth.state.value) ?: return null
                if (authenticationIdentity != null && authenticationIdentity != current) return null
                current
            } else {
                if (auth.state.value != AuthenticationState.SignedOut) return null
                null
            }
            repository(context).reminderCommandStatus(identity, workspace, commandId)
        } catch (cancelled: CancellationException) {
            currentCoroutineContext().ensureActive()
            null
        } catch (_: Exception) {
            null
        }
    }

    private suspend fun execute(
        context: Context,
        workspace: WorkspaceDescriptor,
        identity: String?,
        commandId: String,
        datasetId: String?,
        actionId: String,
        expectedStart: String,
        operation: ReminderOperation,
        requestedAt: Instant,

        isCurrent: () -> Boolean,
    ): ReminderCommandResult {
        val auth = authentication(context)
        try {
            if (auth.state.value == AuthenticationState.Initializing) auth.initialize()
            if (!isCurrent()) return ReminderCommandResult.Superseded
            if (workspace is WorkspaceDescriptor.Remote && accountIdentity(auth.state.value) != identity) {
                return ReminderCommandResult.AuthenticationRequired
            }
            if (workspace is WorkspaceDescriptor.Local && auth.state.value != AuthenticationState.SignedOut) {
                return ReminderCommandResult.Superseded
            }
            val repo = repository(context)
            val status = repo.executeReminderCommand(
                authenticationIdentity = identity, workspace = workspace, commandId = commandId,
                datasetId = datasetId, actionId = actionId, expectedStart = expectedStart,
                completedAt = requestedAt, complete = operation == ReminderOperation.Complete,
                isCurrent = isCurrent,
            )
            if (!isCurrent()) return ReminderCommandResult.Superseded
            notifyWorkspaceChanged()
            if (status == ReminderCommandStatus.QUEUED) {
                BackgroundSyncWorker.requestImmediate(context)
                if (auth.state.value !is AuthenticationState.SignedIn) return ReminderCommandResult.AuthenticationRequired
                if (identity != null) {
                    runCatching { repo.fetchAll(identity) }.onFailure {
                        if (it is CancellationException) throw it
                    }
                    if (!isCurrent()) return ReminderCommandResult.Superseded
                    val refreshed = repo.executeReminderCommand(
                        identity, workspace, commandId, datasetId, actionId, expectedStart,
                        requestedAt, operation == ReminderOperation.Complete, isCurrent,
                    )
                    notifyWorkspaceChanged()
                    return refreshed.toReminderResult()
                }
            }
            return status.toReminderResult()
        } catch (cancelled: CancellationException) {
            currentCoroutineContext().ensureActive()
            if (!isCurrent()) return ReminderCommandResult.Superseded
            if (auth.state.value !is AuthenticationState.SignedIn) return ReminderCommandResult.AuthenticationRequired
            throw cancelled
        } catch (_: DatasetMismatchException) {
            return ReminderCommandResult.Superseded
        } catch (_: DatasetChangedWithPendingIntentException) {
            return ReminderCommandResult.Conflict
        } catch (_: Exception) {
            return if (identity != null && auth.state.value !is AuthenticationState.SignedIn) {
                ReminderCommandResult.AuthenticationRequired
            } else ReminderCommandResult.Retryable
        } finally {
            notifyWorkspaceChanged()
        }
    }

    private fun ReminderCommandStatus.toReminderResult(): ReminderCommandResult = when (this) {
        ReminderCommandStatus.QUEUED -> ReminderCommandResult.Queued
        ReminderCommandStatus.CONFIRMED -> ReminderCommandResult.Confirmed
        ReminderCommandStatus.SUPERSEDED -> ReminderCommandResult.Superseded
        ReminderCommandStatus.CONFLICT -> ReminderCommandResult.Conflict
    }

    internal fun accountIdentity(state: AuthenticationState): String? = when (state) {
        is AuthenticationState.SignedIn -> state.accountId
        is AuthenticationState.InteractionRequired -> state.accountId
        is AuthenticationState.Error -> state.accountId
        else -> null
    }

    private fun generation(state: AuthenticationState): Long? = when (state) {
        is AuthenticationState.SignedIn -> state.sessionGeneration
        is AuthenticationState.InteractionRequired -> state.sessionGeneration
        is AuthenticationState.Error -> state.sessionGeneration
        else -> null
    }

    private class Services(context: Context) {
        val authentication = AuthenticationManager(context)
        val network = RetrofitClient(authentication)
        private var boundKey: Pair<String?, Long?>? = null
        private var boundNetwork: RetrofitClient? = null
        val repository = SubroutineRepository(network.api, context, network.baseUrl) { identity ->
            val capturedEpoch = authentication.sessionEpoch()
            val captured = authentication.state.value
            val expectedGeneration = generation(captured)
            fun checkCurrent() {
                val current = authentication.state.value
                val valid = if (identity == null) {
                    captured == AuthenticationState.SignedOut && current == AuthenticationState.SignedOut
                } else {
                    accountIdentity(captured) == identity && expectedGeneration != null &&
                        accountIdentity(current) == identity && generation(current) == expectedGeneration
                }
                if (!valid || authentication.sessionEpoch() != capturedEpoch) {
                    throw CancellationException("Workspace session changed")
                }
            }
            checkCurrent()
            val tokens = authentication.sessionBoundProvider()
            val transport = synchronized(this) {
                val key = identity to expectedGeneration
                if (boundKey != key || boundNetwork == null) {
                    boundNetwork = RetrofitClient(tokens, requireAuthentication = true,
                        callTimeoutSeconds = 20)
                    boundKey = key
                }
                requireNotNull(boundNetwork)
            }
            val api = object : SubroutineApi by transport.api {
                override suspend fun getAccount() = transport.api.getAccount().also { account ->
                    checkCurrent()
                    if (account.accountId != identity) {
                        tokens.rejectSession()
                        throw CancellationException("The API account no longer matches this workspace session")
                    }
                }
            }
            RepositorySession(api, ::checkCurrent)
        }
    }
}
