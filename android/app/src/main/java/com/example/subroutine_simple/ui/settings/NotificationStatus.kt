package com.example.subroutine_simple.ui.settings

import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.produceState
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle

import com.example.subroutine_simple.reminders.ReminderNotificationStatus
import com.example.subroutine_simple.reminders.ReminderNotifications
import com.example.subroutine_simple.settings.AppSettings
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext

internal data class NotificationStatus(
    val permission: NotificationPermissionStatus = NotificationPermissionStatus.CHECKING,

)

@Composable
internal fun rememberNotificationStatus(
    context: Context,
    settings: AppSettings,
    refreshRevision: Int,
    active: Boolean,

): State<NotificationStatus> {
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val applicationContext = context.applicationContext
    return produceState(
        initialValue = NotificationStatus(),
        lifecycle, settings.reminderSettingsKey(), refreshRevision, active,
    ) {
        if (!active) return@produceState
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            while (isActive) {
                value = try {
                    withContext(Dispatchers.IO) {
                        val permission = when (ReminderNotifications.permissionStatus(applicationContext)) {
                            ReminderNotificationStatus.Enabled,
                            ReminderNotificationStatus.DisabledInApp -> NotificationPermissionStatus.ALLOWED
                            ReminderNotificationStatus.PermissionRequired -> NotificationPermissionStatus.NOT_GRANTED
                            ReminderNotificationStatus.BlockedBySystem,
                            ReminderNotificationStatus.ChannelDisabled -> NotificationPermissionStatus.BLOCKED
                        }
                        NotificationStatus(permission)
                    }
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (_: Exception) {
                    NotificationStatus(
                        permission = NotificationPermissionStatus.UNKNOWN,

                    )
                }
                delay(2_000)
            }
        }
    }
}
