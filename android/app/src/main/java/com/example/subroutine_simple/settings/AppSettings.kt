package com.example.subroutine_simple.settings

enum class AppearanceMode(
    val label: String,
    internal val storedValue: String,
) {
    SYSTEM("System", "system"),
    LIGHT("Light", "light"),
    DARK("Dark", "dark");

    internal companion object {
        fun fromStored(value: String?): AppearanceMode =
            entries.firstOrNull { it.storedValue == value } ?: SYSTEM
    }
}

enum class NotificationLeadTime(
    val label: String,
    val seconds: Int,
) {
    THIRTY_SECONDS("30 sec", 30),
    ONE_MINUTE("1 min", 60),
    FIVE_MINUTES("5 min", 5 * 60),
    TEN_MINUTES("10 min", 10 * 60),
    FIFTEEN_MINUTES("15 min", 15 * 60),
    THIRTY_MINUTES("30 min", 30 * 60);

    internal companion object {
        fun fromStored(seconds: Int): NotificationLeadTime =
            entries.firstOrNull { it.seconds == seconds } ?: THIRTY_SECONDS
    }
}

enum class NotificationPreview(
    val label: String,
    internal val storedValue: String,
) {
    GENERIC("Generic", "generic"),
    ACTION_TITLE("Action title", "action_title");

    internal companion object {
        fun fromStored(value: String?): NotificationPreview =
            entries.firstOrNull { it.storedValue == value } ?: GENERIC
    }
}

data class AppSettings(
    val appearance: AppearanceMode = AppearanceMode.SYSTEM,
    val notificationsEnabled: Boolean = true,
    val notificationLeadTime: NotificationLeadTime = NotificationLeadTime.THIRTY_SECONDS,
    val notificationPreview: NotificationPreview = NotificationPreview.GENERIC,
)
