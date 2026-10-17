package com.example.subroutine_simple.reminders

import android.content.Context

import androidx.work.CoroutineWorker
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor

import com.example.subroutine_simple.sync.AppServices
import java.time.Instant
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.withTimeoutOrNull

class ActionReminderWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val workspace = inputData.workspaceDescriptor() ?: return Result.failure()
        val actionId = inputData.getString(EXTRA_ACTION_ID) ?: return Result.failure()
        val expectedStart = inputData.getString(INPUT_EXPECTED_START) ?: return Result.failure()
        val epoch = inputData.getString(INPUT_REMINDER_EPOCH) ?: return Result.success()
        val ledger = ReminderLedger(applicationContext)
        val record = ledger.records()[actionId] ?: return Result.success()
        if (record.expectedStart != expectedStart || !workspace.sameWorkspaceAs(ledger.activeWorkspace()) || ledger.epoch() != epoch) return Result.success()
        return try {
            val action =
                ReminderWorkspaceAccess.loadActions(applicationContext, workspace).firstOrNull { it.id == actionId }
            synchronized(ReminderScheduler) {
                if (!workspace.sameWorkspaceAs(ledger.activeWorkspace()) || ledger.epoch() != epoch || ledger.records()[actionId] != record) return@synchronized
                val settings = ledger.settings()
                val start = parseReminderTimestamp(action?.start)
                if (!settings.enabled || action == null || !action.queued || action.completion != null ||
                    action.start != expectedStart || start == null || start.plusSeconds(300) <= Instant.now()
                ) {
                    WorkManager.getInstance(applicationContext).cancelUniqueWork(record.workName)
                    ledger.remove(actionId)
                    ReminderNotifications.cancel(applicationContext, record.notificationId)
                    ReminderNotifications.cancelLocalNotification(
                        applicationContext,
                        workspace,
                        actionId,
                        expectedStart
                    )
                } else {
                    ReminderNotifications.postActionReminder(
                        applicationContext, workspace, actionId, action.title,
                        expectedStart, start, settings.showActionTitle, record.notificationId
                    )
                }
            }
            Result.success()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: ReminderWorkspaceUnavailableException) {
            Result.success()
        } catch (_: Exception) {
            Result.retry()
        }
    }
}

class ReminderActionWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val id = inputData.getString("command-id") ?: return Result.success()
        val store = LocalReminderStore.get(applicationContext)
        val entry = store.localCommand(id) ?: return Result.success()
        val command = entry.command
        val ledger = ReminderLedger(applicationContext)
        if (entry.state.terminal || command.epoch != ledger.epoch() || !command.workspace.sameWorkspaceAs(ledger.activeWorkspace())) return Result.success()
        if (!store.localDatasetIsCurrent(command)) {
            store.localCommandState(id, CommandState.Superseded)
            return Result.success()
        }
        val next = if (Instant.now() >= command.retryUntil) CommandState.Expired else {
            val result = try {
                withTimeoutOrNull(25_000) {
                    AppServices.executeLocalReminder(
                        context = applicationContext, workspace = command.workspace, actionId = command.actionId,
                        expectedStart = command.expectedStart, operation = command.operation,
                        commandId = command.commandId, requestedAt = command.requestedAt,
                    )
                } ?: ReminderCommandResult.Retryable
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                ReminderCommandResult.Retryable
            }
            when (result) {
                ReminderCommandResult.Confirmed -> CommandState.Confirmed
                ReminderCommandResult.Queued -> CommandState.Queued
                ReminderCommandResult.Retryable -> CommandState.Retryable
                ReminderCommandResult.AuthenticationRequired -> CommandState.AuthenticationRequired
                ReminderCommandResult.Conflict -> CommandState.Conflict
                ReminderCommandResult.Superseded -> CommandState.Superseded
            }
        }
        synchronized(ReminderScheduler) {
            if (!store.localDatasetIsCurrent(command) || command.epoch != ledger.epoch() || !command.workspace.sameWorkspaceAs(
                    ledger.activeWorkspace()
                )
            ) return Result.success()
            store.localCommandState(id, next)
            store.localCommand(id)?.let { ReminderNotifications.postLocalCommand(applicationContext, it) }
        }
        ReminderScheduler.requestReconcile(applicationContext)
        return if (next in setOf(CommandState.Queued, CommandState.Retryable)) Result.retry() else Result.success()
    }
}

class ReminderReconcileWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val ledger = ReminderLedger(applicationContext)
        val workspace = ledger.activeWorkspace() ?: return Result.success()
        val epoch = ledger.epoch()
        return try {
            val actions = ReminderWorkspaceAccess.loadActions(applicationContext, workspace)
            synchronized(ReminderScheduler) {
                if (ledger.epoch() == epoch && workspace.sameWorkspaceAs(ledger.activeWorkspace())) {
                    ReminderScheduler.reconcile(applicationContext, workspace, actions, ledger.settings())
                }
            }
            Result.success()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: ReminderWorkspaceUnavailableException) {
            Result.success()
        } catch (_: Exception) {
            Result.retry()
        }
    }
}

private object ReminderWorkspaceAccess {
    suspend fun loadActions(context: Context, workspace: WorkspaceDescriptor): List<Action> {
        val authentication = AppServices.authentication(context)
        if (authentication.state.value == AuthenticationState.Initializing) authentication.initialize()
        val state = authentication.state.value
        val identity = when (state) {
            is AuthenticationState.SignedIn -> state.accountId
            is AuthenticationState.InteractionRequired -> state.accountId
            is AuthenticationState.Error -> state.accountId
            else -> null
        }
        val repository = AppServices.repository(context)
        val items = when (workspace) {
            is WorkspaceDescriptor.Local -> {
                if (state != AuthenticationState.SignedOut) throw ReminderWorkspaceUnavailableException()
                repository.loadLocal()
            }

            is WorkspaceDescriptor.Remote -> {
                if (identity == null || !canReconcileRemoteReminders(state, workspace.accountId)) {
                    throw ReminderWorkspaceUnavailableException()
                }
                repository.loadCached(identity) ?: throw ReminderWorkspaceUnavailableException()
            }
        }
        if (!workspace.sameWorkspaceAs(repository.activeWorkspaceDescriptor(identity))) throw ReminderWorkspaceUnavailableException()
        return items.actions
    }
}

private class ReminderWorkspaceUnavailableException : IllegalStateException()
