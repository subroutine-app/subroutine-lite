package com.example.subroutine_simple.reminders

import android.Manifest
import android.annotation.SuppressLint
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.example.subroutine_simple.MainActivity
import com.example.subroutine_simple.R
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor
import com.example.subroutine_simple.settings.AppSettings
import com.example.subroutine_simple.settings.SettingsStore

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

const val POST_NOTIFICATIONS_PERMISSION: String = Manifest.permission.POST_NOTIFICATIONS

enum class ReminderNotificationStatus {
    DisabledInApp,
    PermissionRequired,
    BlockedBySystem,
    ChannelDisabled,
    Enabled,
}

object ReminderNotifications {
    const val CHANNEL_ID = "action-reminders"
    private const val CHANNEL_NAME = "Subroutine Lite action reminders"
    private const val CHANNEL_DESCRIPTION = "Subroutine Lite reminders for upcoming queued actions"
    private const val TEST_NOTIFICATION_ID = 0x535542

    fun createChannel(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = context.getSystemService(NotificationManager::class.java)
        val channel = manager.getNotificationChannel(CHANNEL_ID) ?: NotificationChannel(
            CHANNEL_ID,
            CHANNEL_NAME,
            NotificationManager.IMPORTANCE_DEFAULT,
        )
        channel.name = CHANNEL_NAME
        channel.description = CHANNEL_DESCRIPTION
        manager.createNotificationChannel(channel)
    }

    fun hasPostNotificationsPermission(context: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, POST_NOTIFICATIONS_PERMISSION) ==
            PackageManager.PERMISSION_GRANTED

    fun status(context: Context, settings: AppSettings): ReminderNotificationStatus =
        if (settings.notificationsEnabled) permissionStatus(context) else ReminderNotificationStatus.DisabledInApp

    fun permissionStatus(context: Context): ReminderNotificationStatus {
        if (!hasPostNotificationsPermission(context)) return ReminderNotificationStatus.PermissionRequired
        if (!NotificationManagerCompat.from(context).areNotificationsEnabled()) {
            return ReminderNotificationStatus.BlockedBySystem
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = context.getSystemService(NotificationManager::class.java)
                .getNotificationChannel(CHANNEL_ID)
            if (channel != null && channel.importance == NotificationManager.IMPORTANCE_NONE) {
                return ReminderNotificationStatus.ChannelDisabled
            }
        }
        return ReminderNotificationStatus.Enabled
    }

    fun systemSettingsIntent(context: Context): Intent =
        Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)

    fun openSystemSettings(context: Context) {
        val intent = systemSettingsIntent(context).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        runCatching { context.startActivity(intent) }
            .recoverCatching {
                context.startActivity(
                    Intent(
                        Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                        Uri.parse("package:${context.packageName}"),
                    ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                )
            }
    }

    @SuppressLint("MissingPermission")
    fun postTestNotification(context: Context, settings: AppSettings): Boolean {
        createChannel(context)
        if (status(context, settings) != ReminderNotificationStatus.Enabled) return false
        val notification = NotificationCompat.Builder(context, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle("Subroutine Lite test notification")
            .setContentText("Notifications can be delivered while Subroutine Lite is running.")
            .setCategory(NotificationCompat.CATEGORY_REMINDER)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setAutoCancel(true)
            .build()
        return runCatching {
            NotificationManagerCompat.from(context).notify(TEST_NOTIFICATION_ID, notification)
            true
        }.getOrDefault(false)
    }

    internal fun postActionReminder(
        context: Context, workspace: WorkspaceDescriptor, actionId: String, actionTitle: String,
        expectedStart: String, start: Instant, showActionTitle: Boolean, notificationId: Int,
    ): Boolean {
        val store = LocalReminderStore.get(context)
        synchronized(store) {
            val occurrence = localOccurrence(context, workspace, actionId)
            val pending = store.localCommandFor(occurrence)
            if (pending != null && store.localDatasetIsCurrent(pending.command) &&
                pending.command.epoch == ReminderLedger(context).epoch() && pending.command.expectedStart == expectedStart) {
                if (postLocalCommand(context, pending)) return true
            }
            if (!canNotify(context)) return false
            val version = "local:${ReminderLedger(context).epoch()}:$expectedStart:$showActionTitle:${actionTitle.hashCode()}"
            val claim = store.claimAlert(occurrence, version, workspace.workspaceId) ?: return true
            val posted = postPreparedReminder(context, workspace, actionId, actionTitle, expectedStart, start,
                showActionTitle, notificationId, occurrence, version, claim.silent, null)
            if (posted) store.rendered(occurrence, version)
            return posted
        }
    }

    internal fun postLocalCommand(context: Context, stored: StoredLocalReminderCommand): Boolean {
        val command = stored.command
        val store = LocalReminderStore.get(context)
        synchronized(store) {
            val ledger = ReminderLedger(context)
            if (!store.localDatasetIsCurrent(command) || !command.workspace.sameWorkspaceAs(ledger.activeWorkspace()) ||
                command.epoch != ledger.epoch() || !canNotify(context)) return false
            if (!store.canRenderLocalFeedback(stored, ledger.records()[command.actionId]?.expectedStart)) return false
            val version = "local-command:${command.commandId}:${stored.state}"
            store.claimAlert(command.occurrence, version, command.workspaceId) ?: return true
            if (postPreparedReminder(context, command.workspace, command.actionId, "", command.expectedStart,
                Instant.parse(command.expectedStart), false, ReminderLedger.notificationId(command.workspaceId, command.actionId),
                command.occurrence, version, true, feedback(command.operation, stored.state))) {
                store.rendered(command.occurrence, version)
            }
            return true
        }
    }

    @SuppressLint("MissingPermission")
    private fun postPreparedReminder(
        context: Context,
        workspace: WorkspaceDescriptor,
        actionId: String,
        actionTitle: String,
        expectedStart: String,
        start: Instant,
        showActionTitle: Boolean,
        notificationId: Int,
        occurrence: String,
        version: String,
        silent: Boolean,
        feedback: String?,
    ): Boolean {
        createChannel(context)
        if (!hasPostNotificationsPermission(context) ||
            !NotificationManagerCompat.from(context).areNotificationsEnabled()
        ) {
            return false
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            context.getSystemService(NotificationManager::class.java)
                .getNotificationChannel(CHANNEL_ID)?.importance == NotificationManager.IMPORTANCE_NONE
        ) {
            return false
        }

        val formattedStart = DateTimeFormatter
            .ofLocalizedTime(FormatStyle.SHORT)
            .format(start.atZone(ZoneId.systemDefault()))
        val contentIntent = PendingIntent.getActivity(
            context,
            notificationId,
            notificationIntent(context, workspace, actionId).setData(reminderUri(workspace, actionId, "open/${ReminderLedger(context).epoch()}/$expectedStart")),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val completeIntent = actionPendingIntent(
            context = context,
            workspace = workspace,
            actionId = actionId,
            expectedStart = expectedStart,
            notificationId = notificationId,
            operation = ReminderOperation.Complete,
        )
        val unqueueIntent = actionPendingIntent(
            context = context,
            workspace = workspace,
            actionId = actionId,
            expectedStart = expectedStart,
            notificationId = notificationId,
            operation = ReminderOperation.Unqueue,
        )
        val notification = NotificationCompat.Builder(context, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(
                if (showActionTitle && actionTitle.isNotBlank()) actionTitle else "Upcoming action",
            )
            .setContentText(feedback ?: "Scheduled for $formattedStart")
            .setContentIntent(contentIntent)
            .setCategory(NotificationCompat.CATEGORY_REMINDER)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .setSilent(silent)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setDeleteIntent(deleteIntent(context, occurrence, version))
            .apply {
                if (feedback == null) {
                    setTimeoutAfter(java.time.Duration.between(Instant.now(), start.plusSeconds(300)).toMillis().coerceAtLeast(1))
                    addAction(0, "Complete", completeIntent)
                    addAction(0, "Unqueue", unqueueIntent)
                }
            }
            .build()
        return runCatching {
            NotificationManagerCompat.from(context).notify(occurrence, 0, notification)
            true
        }.getOrDefault(false)
    }

    internal fun cancel(context: Context, notificationId: Int) {
        NotificationManagerCompat.from(context).cancel(notificationId)
    }

    internal fun canNotify(context: Context): Boolean =
        status(context, SettingsStore(context).settings.value) == ReminderNotificationStatus.Enabled

    internal fun localOccurrence(context: Context, workspace: WorkspaceDescriptor, actionId: String): String {
        val dataset = (workspace as? WorkspaceDescriptor.Remote)?.let { LocalReminderStore.get(context).datasetFor(it) }
        return if (workspace is WorkspaceDescriptor.Remote && dataset != null) {
            "${workspace.accountId}/$dataset/$actionId"
        } else "local/${workspace.workspaceId}/$actionId"
    }

    internal fun cancelWorkspace(context: Context, workspaceId: String) {
        val store = LocalReminderStore.get(context)
        synchronized(store) {
            store.alertTags(workspaceId).forEach {
                NotificationManagerCompat.from(context).cancel(it, 0)
                store.forgetRendering(it)
            }
        }
    }

    internal fun cancelLocalNotification(context: Context, workspace: WorkspaceDescriptor, actionId: String, expectedStart: String) {
        val store = LocalReminderStore.get(context)
        synchronized(store) {
            val occurrence = localOccurrence(context, workspace, actionId)
            val version = store.renderedVersion(occurrence)
            if (version?.startsWith("local:") == true && version.contains(":$expectedStart:")) {
                NotificationManagerCompat.from(context).cancel(occurrence, 0)
                store.forgetRendering(occurrence)
            }
        }
    }

    private fun feedback(operation: ReminderOperation, state: CommandState): String {
        val verb = if (operation == ReminderOperation.Complete) "Complete" else "Unqueue"
        return when (state) {
            CommandState.Pending -> "$verb pending — saved on this device."
            CommandState.Queued -> "$verb pending — waiting for server confirmation."
            CommandState.Retryable -> "$verb pending — waiting for connection."
            CommandState.AuthenticationRequired -> "$verb pending — sign in to finish."
            CommandState.Conflict -> "$verb needs attention — open Subroutine Lite to review the conflict."
            CommandState.Confirmed -> if (operation == ReminderOperation.Complete) "Action completed." else "Action unqueued."
            CommandState.Superseded -> "Reminder changed or is no longer current. This command was not applied."
            CommandState.Expired -> "Could not confirm this command within 24 hours. Open Subroutine Lite to check its status."
        }
    }

    private fun deleteIntent(context: Context, occurrence: String, version: String): PendingIntent = PendingIntent.getBroadcast(
        context, 0, Intent(context, ReminderDismissReceiver::class.java)
            .setData(Uri.Builder().scheme("subroutine-lite").authority("reminder-dismiss")
                .appendPath(occurrence).appendPath(version).build())
            .putExtra("occurrence", occurrence).putExtra("version", version),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    private fun notificationIntent(
        context: Context,
        workspace: WorkspaceDescriptor,
        actionId: String,
    ): Intent = Intent(context, MainActivity::class.java)
        .setAction(ACTION_OPEN_REMINDER)
        .setData(reminderUri(workspace, actionId, "open"))
        .putWorkspace(workspace)
        .putExtra(EXTRA_ACTION_ID, actionId)
        .putExtra(INPUT_REMINDER_EPOCH, ReminderLedger(context).epoch())
        .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)

    private fun actionPendingIntent(
        context: Context,
        workspace: WorkspaceDescriptor,
        actionId: String,
        expectedStart: String,
        notificationId: Int,
        operation: ReminderOperation,
    ): PendingIntent {
        val requestCode = notificationId xor operation.requestCodeSalt
        val intent = Intent(context, ReminderActionReceiver::class.java)
            .setAction(operation.intentAction)
            .setData(reminderUri(workspace, actionId, "${operation.path}/${ReminderLedger(context).epoch()}/$expectedStart"))
            .putWorkspace(workspace)
            .putExtra(EXTRA_ACTION_ID, actionId)
            .putExtra(INPUT_EXPECTED_START, expectedStart)
            .putExtra(INPUT_REMINDER_EPOCH, ReminderLedger(context).epoch())
            .putExtra(EXTRA_NOTIFICATION_ID, notificationId)
        return PendingIntent.getBroadcast(
            context,
            requestCode,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private fun reminderUri(
        workspace: WorkspaceDescriptor,
        actionId: String,
        operation: String,
    ): Uri = Uri.Builder()
        .scheme("subroutine-lite")
        .authority("reminder")
        .appendPath(operation)
        .appendPath(workspace.reminderWorkspaceId)
        .appendPath(actionId)
        .build()
}
