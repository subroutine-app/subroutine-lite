package com.example.subroutine_simple.reminders

import android.content.Context
import android.content.Intent
import com.example.subroutine_simple.BuildConfig
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor
import com.example.subroutine_simple.sync.AppServices
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import java.time.Instant

internal object LocalReminderRuntime {
    suspend fun onWorkspaceReady(
        context: Context,
        workspace: WorkspaceDescriptor.Remote,
        datasetId: String,
        authenticationIdentity: String,
        scheduleBackgroundWork: Boolean = true,
    ) = withContext(Dispatchers.IO) {
        if (BuildConfig.OFFLINE_ONLY) return@withContext
        val store = LocalReminderStore.get(context)
        val next = ReminderWorkspace(workspace.workspaceId, workspace.accountId, datasetId,
            authenticationIdentity, BuildConfig.API_BASE_URL)
        synchronized(ReminderScheduler) {
            val previous = store.workspace()
            if (previous != null && previous != next) ReminderScheduler.cancelAll(context)
            store.setWorkspace(next)
        }
        if (scheduleBackgroundWork) ReminderRecoveryWorker.request(context)
    }

    suspend fun invalidate(context: Context) = withContext(Dispatchers.IO) {
        ReminderScheduler.cancelAll(context)
    }

    fun onForeground(context: Context) = ReminderRecoveryWorker.request(context, resumeAuthentication = true)

    fun isCurrentOpenIntent(context: Context, intent: Intent): Boolean {
        val epoch = intent.getStringExtra(INPUT_REMINDER_EPOCH) ?: return false
        val workspace = intent.workspaceDescriptor() ?: return false
        val ledger = ReminderLedger(context)
        return ledger.epoch() == epoch && workspace.sameWorkspaceAs(ledger.activeWorkspace())
    }

    suspend fun refreshCommandStatuses(context: Context) = withContext(Dispatchers.IO) {
        val store = LocalReminderStore.get(context)
        val ledger = ReminderLedger(context)
        for (entry in store.localCommands()) {
            val command = entry.command
            fun isCurrent() = store.localDatasetIsCurrent(command) && command.epoch == ledger.epoch() &&
                command.workspace.sameWorkspaceAs(ledger.activeWorkspace())
            if (!isCurrent() || entry.state in setOf(CommandState.Confirmed, CommandState.Superseded)) continue
            val active = store.workspace()
            val identity = if (command.accountId == null) null else active?.authenticationIdentity ?: continue
            val status = try {
                AppServices.reminderCommandStatus(context, command.commandId, command.workspace, identity)
            } catch (_: CancellationException) {
                currentCoroutineContext().ensureActive()
                null
            }
            synchronized(ReminderScheduler) {
                if (store.workspace() == active && isCurrent()) {
                    val current = store.localCommand(command.commandId) ?: return@synchronized
                    store.localCommandState(command.commandId,
                        statusOnlyState(current.state, status, command.retryUntil, Instant.now()))
                    store.localCommand(command.commandId)?.let { ReminderNotifications.postLocalCommand(context, it) }
                }
            }
        }
        store.localCommands().forEach { ReminderNotifications.postLocalCommand(context, it) }
    }
}
