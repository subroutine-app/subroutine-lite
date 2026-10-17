package com.example.subroutine_simple.ui.settings

import com.example.subroutine_simple.settings.AppSettings

internal fun AppSettings.reminderSettingsKey() = Triple(
    notificationsEnabled, notificationLeadTime, notificationPreview,
)

enum class NotificationPermissionStatus(
    internal val description: String,
    internal val canPostNotifications: Boolean,
) {
    CHECKING("Checking notification permission", false),
    ALLOWED("Android allows notifications.", true),
    NOT_GRANTED("Enabling reminders requests notification permission.", false),
    BLOCKED("Notifications are blocked in Android settings.", false),
    UNAVAILABLE("Notifications are unavailable on this device.", false),
    UNKNOWN("Could not check notification permission.", false),
}

sealed interface SettingsAccountState {
    data object SignedOutLocalOnly : SettingsAccountState

    data class NeedsAttention(
        val username: String?,
    ) : SettingsAccountState

    data class SignedIn(
        val username: String?,
    ) : SettingsAccountState

    data class SignOutFailed(val message: String) : SettingsAccountState
}

data class SettingsDataState(
    val pendingCount: Int = 0,
    val blockedCount: Int = 0,
    val isRemoteAccountData: Boolean = false,
) {
    init {
        require(pendingCount >= 0) { "pendingCount must not be negative" }
        require(blockedCount >= 0) { "blockedCount must not be negative" }
        require(blockedCount <= pendingCount) { "blockedCount must be included in pendingCount" }
    }
}
