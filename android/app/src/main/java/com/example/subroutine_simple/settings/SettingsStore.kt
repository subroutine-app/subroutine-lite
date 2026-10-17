package com.example.subroutine_simple.settings

import android.content.Context
import android.content.SharedPreferences
import com.example.subroutine_simple.reminders.ReminderNotificationStatus
import com.example.subroutine_simple.reminders.ReminderNotifications
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class SettingsStore(
    private val preferences: SharedPreferences,
    private val notificationsAllowed: () -> Boolean,
) {
    constructor(
        context: Context,
        preferencesName: String = DEFAULT_PREFERENCES_NAME,
    ) : this(
        context.applicationContext.getSharedPreferences(preferencesName, Context.MODE_PRIVATE),
        { ReminderNotifications.permissionStatus(context.applicationContext) == ReminderNotificationStatus.Enabled },
    )

    private val mutableSettings = MutableStateFlow(preferences.readSettings())
    val settings: StateFlow<AppSettings> = mutableSettings.asStateFlow()

    init {
        refreshNotificationPermission()
    }

    val notificationPermissionChecked: Boolean
        get() = preferences.getBoolean(KEY_NOTIFICATION_PERMISSION_CHECKED, false)

    fun markNotificationPermissionChecked() {
        check(preferences.edit().putBoolean(KEY_NOTIFICATION_PERMISSION_CHECKED, true).commit()) {
            "Could not persist notification permission request"
        }
    }

    fun refreshNotificationPermission() {
        update { it }
    }

    fun setAppearance(appearance: AppearanceMode) {
        update { it.copy(appearance = appearance) }
    }

    fun setNotificationsEnabled(enabled: Boolean) {
        update { it.copy(notificationsEnabled = enabled) }
    }

    fun setNotificationLeadTime(leadTime: NotificationLeadTime) {
        update { it.copy(notificationLeadTime = leadTime) }
    }

    fun setNotificationPreview(preview: NotificationPreview) {
        update { it.copy(notificationPreview = preview) }
    }

    fun reset() {
        update { AppSettings() }
    }

    fun update(transform: (AppSettings) -> AppSettings) {
        synchronized(preferences) {
            replace(transform(preferences.readSettings()))
        }
    }

    private fun replace(requested: AppSettings) {
        val updated = if (requested.notificationsEnabled && !notificationsAllowed()) {
            requested.copy(notificationsEnabled = false)
        } else requested
        if (updated != preferences.readSettings()) {
            val persisted = preferences.edit()
                .putString(KEY_APPEARANCE, updated.appearance.storedValue)
                .putBoolean(KEY_NOTIFICATIONS_ENABLED, updated.notificationsEnabled)
                .putInt(KEY_NOTIFICATION_LEAD_SECONDS, updated.notificationLeadTime.seconds)
                .putString(KEY_NOTIFICATION_PREVIEW, updated.notificationPreview.storedValue)
                .commit()

            check(persisted) { "Could not persist settings" }
        }
        mutableSettings.value = updated
    }

    private fun SharedPreferences.readSettings(): AppSettings = AppSettings(
        appearance = AppearanceMode.fromStored(getString(KEY_APPEARANCE, null)),
        notificationsEnabled = getBoolean(KEY_NOTIFICATIONS_ENABLED, true),
        notificationLeadTime = NotificationLeadTime.fromStored(
            getInt(KEY_NOTIFICATION_LEAD_SECONDS, NotificationLeadTime.THIRTY_SECONDS.seconds),
        ),
        notificationPreview = NotificationPreview.fromStored(
            getString(KEY_NOTIFICATION_PREVIEW, null),
        ),
    )

    companion object {
        const val DEFAULT_PREFERENCES_NAME = "subroutine_settings"

        private const val KEY_APPEARANCE = "appearance"
        private const val KEY_NOTIFICATIONS_ENABLED = "notifications_enabled"
        private const val KEY_NOTIFICATION_PERMISSION_CHECKED = "notification_permission_checked"
        private const val KEY_NOTIFICATION_LEAD_SECONDS = "notification_lead_seconds"
        private const val KEY_NOTIFICATION_PREVIEW = "notification_preview"
    }
}
