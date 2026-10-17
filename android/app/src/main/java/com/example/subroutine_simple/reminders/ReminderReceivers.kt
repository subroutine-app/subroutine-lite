package com.example.subroutine_simple.reminders

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.workDataOf
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

class ReminderActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val workspace = intent.workspaceDescriptor() ?: return
        val actionId = intent.getStringExtra(EXTRA_ACTION_ID)?.takeIf { it.length in 1..128 } ?: return
        val expectedStart = intent.getStringExtra(INPUT_EXPECTED_START) ?: return
        val epoch = intent.getStringExtra(INPUT_REMINDER_EPOCH) ?: return
        val operation = when (intent.action) {
            ACTION_COMPLETE_REMINDER -> ReminderOperation.Complete
            ACTION_UNQUEUE_REMINDER -> ReminderOperation.Unqueue
            else -> return
        }
        val pending = goAsync()
        scope.launch {
            try {
                synchronized(ReminderScheduler) {
                    val ledger = ReminderLedger(context)
                    val record = ledger.records()[actionId]
                    val now = Instant.now()
                    if (!workspace.sameWorkspaceAs(ledger.activeWorkspace()) || ledger.epoch() != epoch ||
                        record?.expectedStart != expectedStart || parseReminderTimestamp(expectedStart)?.plusSeconds(300)?.let { it <= now } != false) return@synchronized
                    val store = LocalReminderStore.get(context)
                    val datasetId = if (workspace is WorkspaceDescriptor.Remote) {
                        store.datasetFor(workspace) ?: return@synchronized
                    } else null
                    val command = store.captureLocal(LocalReminderCommand(
                        UUID.randomUUID().toString(), workspace.workspaceId, (workspace as? WorkspaceDescriptor.Remote)?.accountId,
                        epoch, ReminderNotifications.localOccurrence(context, workspace, actionId), actionId, expectedStart, operation, now, datasetId,
                    ))
                    try {
                        store.localCommand(command.commandId)?.let { ReminderNotifications.postLocalCommand(context, it) }
                    } finally {
                        enqueue(context, command, userInitiated = true)
                        ReminderRecoveryWorker.request(context)
                    }
                }
            } catch (_: Exception) { Log.w("ReminderAction", "Could not finish notification command processing") }
            finally { pending.finish() }
        }
    }

    companion object {
        private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        internal fun enqueue(context: Context, command: LocalReminderCommand, userInitiated: Boolean = false) {
            WorkManager.getInstance(context).enqueueUniqueWork("local-command-${command.commandId}", ExistingWorkPolicy.KEEP,
                OneTimeWorkRequestBuilder<ReminderActionWorker>().setInputData(workDataOf("command-id" to command.commandId))
                    .addTag("local-reminder-commands")
                    .apply { commandExpeditedPolicy(Build.VERSION.SDK_INT, userInitiated)?.let { setExpedited(it) } }
                    .build())
        }
    }
}

class ReminderDismissReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val occurrence = intent.getStringExtra("occurrence")?.takeIf { it.length <= 256 } ?: return
        val version = intent.getStringExtra("version")?.takeIf { it.length <= 256 } ?: return
        val pending = goAsync()
        scope.launch {
            try {
                LocalReminderStore.get(context).dismiss(occurrence, version)
            } catch (_: Exception) { Log.w("ReminderDismiss", "Could not persist notification dismissal") }
            finally { pending.finish() }
        }
    }

    companion object { private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO) }
}

class ReminderSystemReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action in setOf(Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_TIME_CHANGED,
                Intent.ACTION_TIMEZONE_CHANGED, Intent.ACTION_MY_PACKAGE_REPLACED)) {
            ReminderScheduler.requestReconcile(context.applicationContext)
            ReminderRecoveryWorker.request(context.applicationContext)
        }
    }
}
