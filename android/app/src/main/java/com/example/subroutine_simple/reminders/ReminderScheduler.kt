package com.example.subroutine_simple.reminders

import android.content.Context
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor
import com.example.subroutine_simple.settings.AppSettings

import com.example.subroutine_simple.settings.NotificationPreview
import java.time.Duration
import java.time.Instant
import java.util.concurrent.TimeUnit

object ReminderScheduler {
    fun reconcile(
        context: Context,
        workspace: WorkspaceDescriptor,
        actions: List<Action>,
        settings: AppSettings,
        now: Instant = Instant.now(),
    ): List<ActionReminderPlan> {
        val snapshot = ReminderSettingsSnapshot(
            enabled = settings.notificationsEnabled,
            leadSeconds = settings.notificationLeadTime.seconds.toLong().coerceAtLeast(0L),
            showActionTitle = settings.notificationPreview == NotificationPreview.ACTION_TITLE,
        )
        return reconcile(context.applicationContext, workspace, actions, snapshot, now)
    }

    @Synchronized
    internal fun reconcile(
        context: Context,
        workspace: WorkspaceDescriptor,
        actions: List<Action>,
        settings: ReminderSettingsSnapshot,
        now: Instant = Instant.now(),
    ): List<ActionReminderPlan> {
        val appContext = context.applicationContext
        val workManager = WorkManager.getInstance(appContext)
        val ledger = ReminderLedger(appContext)
        cancelRecords(appContext, workManager, ledger.takeLegacyRecords())
        val snapshot = settings.copy(
            enabled = settings.enabled &&
                ReminderNotifications.permissionStatus(appContext) == ReminderNotificationStatus.Enabled,
        )

        if (workspace is WorkspaceDescriptor.Remote) {
            if (LocalReminderStore.get(appContext).datasetFor(workspace) == null) return emptyList()
        }


        if (reminderWorkspaceChange(ledger.activeWorkspace(), workspace, ledger.settings().showActionTitle,
                snapshot.showActionTitle) == ReminderWorkspaceChange.Workspace) {
            ledger.activeWorkspace()?.let { ReminderNotifications.cancelWorkspace(appContext, it.workspaceId) }
            cancelRecords(appContext, workManager, ledger.records().values)
            ledger.removeAll()
        }
        if (!snapshot.enabled || ledger.settings().showActionTitle != snapshot.showActionTitle) {
            ReminderNotifications.cancelWorkspace(appContext, workspace.workspaceId)
        }
        ledger.replaceWorkspaceAndSettings(workspace, snapshot)
        ReminderNotifications.createChannel(appContext)


        val plans = if (snapshot.enabled) {
            planActionReminders(actions, now, snapshot.leadSeconds)
        } else {
            emptyList()
        }
        val plansById = plans.associateBy(ActionReminderPlan::actionId)
        val existing = ledger.records()

        existing.values.forEach { record ->
            val plan = plansById[record.actionId]
            if (plan == null ||
                record.expectedStart != plan.expectedStart ||
                record.triggerAtEpochMillis != plan.triggerAt.toEpochMilli()
            ) {
                workManager.cancelUniqueWork(record.workName)
                ReminderNotifications.cancel(appContext, record.notificationId)
                ReminderNotifications.cancelLocalNotification(appContext, workspace, record.actionId, record.expectedStart)
                ledger.remove(record.actionId)
            }
        }

        plans.forEach { plan ->
            val workName = ReminderLedger.workName(workspace.reminderWorkspaceId, plan.actionId)
            val notificationId = ReminderLedger.notificationId(
                workspace.reminderWorkspaceId,
                plan.actionId,
            )
            val record = ReminderLedgerRecord(
                actionId = plan.actionId,
                expectedStart = plan.expectedStart,
                triggerAtEpochMillis = plan.triggerAt.toEpochMilli(),
                workName = workName,
                notificationId = notificationId,
            )
            if (plan.triggerAt <= now) {
                ReminderNotifications.postActionReminder(appContext, workspace, plan.actionId, plan.title,
                    plan.expectedStart, plan.start, snapshot.showActionTitle, notificationId)
            }
            if (existing[plan.actionId] == record) return@forEach
            val delayMillis = runCatching { Duration.between(now, plan.triggerAt).toMillis() }
                .getOrDefault(Long.MAX_VALUE)
                .coerceAtLeast(0L)
            val request = OneTimeWorkRequestBuilder<ActionReminderWorker>()
                .setInitialDelay(delayMillis, TimeUnit.MILLISECONDS)
                .setInputData(
                    reminderInputData(
                        workspace = workspace,
                        actionId = plan.actionId,
                        expectedStart = plan.expectedStart,
                        notificationId = notificationId,
                        epoch = ledger.epoch(),
                    ),
                )
                .addTag(REMINDER_WORK_TAG)
                .build()
            workManager.enqueueUniqueWork(workName, ExistingWorkPolicy.REPLACE, request)
            ledger.put(record)
        }
        return plans
    }

    @Synchronized
    fun cancelAll(context: Context) {
        val appContext = context.applicationContext
        val workManager = WorkManager.getInstance(appContext)
        val ledger = ReminderLedger(appContext)
        ledger.activeWorkspace()?.let { ReminderNotifications.cancelWorkspace(appContext, it.workspaceId) }
        ledger.activeWorkspace()?.let { LocalReminderStore.get(appContext).retireLocalWorkspace(it.workspaceId) }
        LocalReminderStore.get(appContext).setWorkspace(null)
        ledger.deactivate()
        cancelRecords(appContext, workManager, ledger.records().values)
        ledger.removeAll()
        workManager.cancelAllWorkByTag(REMINDER_WORK_TAG)
        workManager.cancelAllWorkByTag(REMINDER_RECONCILE_TAG)
        workManager.cancelAllWorkByTag("local-reminder-commands")
    }

    fun isCurrentCommand(context: Context, commandId: String): Boolean {
        val entry = LocalReminderStore.get(context).localCommand(commandId) ?: return false
        val ledger = ReminderLedger(context)
        return !entry.state.terminal && LocalReminderStore.get(context).localDatasetIsCurrent(entry.command) &&
            entry.command.epoch == ledger.epoch() &&
            entry.command.workspace.sameWorkspaceAs(ledger.activeWorkspace())
    }

    fun requestReconcile(context: Context) {
        val request = OneTimeWorkRequestBuilder<ReminderReconcileWorker>()
            .addTag(REMINDER_RECONCILE_TAG)
            .build()
        WorkManager.getInstance(context.applicationContext).enqueueUniqueWork(
            RECONCILE_WORK_NAME,
            ExistingWorkPolicy.APPEND_OR_REPLACE,
            request,
        )
    }

    private fun cancelRecords(
        context: Context,
        workManager: WorkManager,
        records: Collection<ReminderLedgerRecord>,
    ) {
        records.forEach { record ->
            workManager.cancelUniqueWork(record.workName)
            ReminderNotifications.cancel(context, record.notificationId)
        }
    }

    internal const val REMINDER_WORK_TAG = "subroutine-action-reminders"
    internal const val REMINDER_RECONCILE_TAG = "subroutine-reminder-reconcile"
    private const val RECONCILE_WORK_NAME = "subroutine-reminder-reconcile"
}
