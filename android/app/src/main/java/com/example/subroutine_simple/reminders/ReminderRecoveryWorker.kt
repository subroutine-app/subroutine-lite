package com.example.subroutine_simple.reminders

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import java.time.Instant
import java.util.concurrent.TimeUnit

class ReminderRecoveryWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val store = LocalReminderStore.get(applicationContext)
        LocalReminderRuntime.refreshCommandStatuses(applicationContext)
        val resumeAuthentication = inputData.getBoolean("resume-authentication", false)
        val ledger = ReminderLedger(applicationContext)
        synchronized(ReminderScheduler) {
            store.localCommands().filter {
                !it.state.terminal && store.localDatasetIsCurrent(it.command) &&
                    it.command.epoch == ledger.epoch() && it.command.workspace.sameWorkspaceAs(ledger.activeWorkspace())
            }.forEach {
                when (commandRecovery(it.state, it.command.retryUntil, Instant.now(), resumeAuthentication)) {
                    CommandRecovery.Expire -> {
                        store.localCommandState(it.command.commandId, CommandState.Expired)
                        store.localCommand(it.command.commandId)?.let { entry -> ReminderNotifications.postLocalCommand(applicationContext, entry) }
                    }
                    CommandRecovery.Execute -> ReminderActionReceiver.enqueue(applicationContext, it.command)
                    CommandRecovery.None -> Unit
                }
            }
        }
        return Result.success()
    }

    companion object {
        fun request(context: Context, resumeAuthentication: Boolean = false) {
            val wm = WorkManager.getInstance(context)
            wm.enqueueUniqueWork("local-reminder-recovery", if (resumeAuthentication) ExistingWorkPolicy.APPEND_OR_REPLACE else ExistingWorkPolicy.KEEP,
                OneTimeWorkRequestBuilder<ReminderRecoveryWorker>().setInputData(workDataOf("resume-authentication" to resumeAuthentication)).build())
            wm.enqueueUniquePeriodicWork("local-reminder-recovery-periodic", ExistingPeriodicWorkPolicy.KEEP,
                PeriodicWorkRequestBuilder<ReminderRecoveryWorker>(15, TimeUnit.MINUTES).build())
        }
    }
}
