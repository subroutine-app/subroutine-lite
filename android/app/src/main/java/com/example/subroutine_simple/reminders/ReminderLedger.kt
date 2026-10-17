package com.example.subroutine_simple.reminders

import android.content.Context
import android.content.SharedPreferences
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor
import java.nio.ByteBuffer
import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.util.Base64

internal data class ReminderSettingsSnapshot(
    val enabled: Boolean,
    val leadSeconds: Long,
    val showActionTitle: Boolean,
)

internal data class ReminderLedgerRecord(
    val actionId: String,
    val expectedStart: String,
    val triggerAtEpochMillis: Long,
    val workName: String,
    val notificationId: Int,
)

internal class ReminderLedger(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(
        PREFERENCES_NAME,
        Context.MODE_PRIVATE,
    )

    fun takeLegacyRecords(): Collection<ReminderLedgerRecord> = synchronized(LOCK) {
        if (preferences.getInt("format", 0) >= 2) return@synchronized emptyList()
        val previous = records().values.toList()
        removeAll()
        deactivate()
        check(preferences.edit().putInt("format", 2).commit())
        previous
    }

    fun activeWorkspace(): WorkspaceDescriptor? = synchronized(LOCK) {
        val workspaceId = preferences.getString(KEY_WORKSPACE_ID, null)
            ?: return@synchronized null
        when (preferences.getString(KEY_WORKSPACE_KIND, null)) {
            WORKSPACE_LOCAL -> WorkspaceDescriptor.Local(workspaceId)
            WORKSPACE_REMOTE -> preferences.getString(KEY_ACCOUNT_ID, null)?.let { accountId ->
                WorkspaceDescriptor.Remote(workspaceId, accountId)
            }
            else -> null
        }
    }

    fun epoch(): String = synchronized(LOCK) {
        preferences.getString("epoch", null) ?: java.util.UUID.randomUUID().toString().also {
            check(preferences.edit().putString("epoch", it).commit())
        }
    }

    fun deactivate() = synchronized(LOCK) {
        check(preferences.edit().remove(KEY_WORKSPACE_ID).remove(KEY_WORKSPACE_KIND)
            .remove(KEY_ACCOUNT_ID).putString("epoch", java.util.UUID.randomUUID().toString()).commit())
    }

    fun settings(): ReminderSettingsSnapshot = synchronized(LOCK) { ReminderSettingsSnapshot(
        enabled = preferences.getBoolean(KEY_ENABLED, false),
        leadSeconds = preferences.getLong(KEY_LEAD_SECONDS, 0L).coerceAtLeast(0L),
        showActionTitle = preferences.getBoolean(KEY_SHOW_ACTION_TITLE, false),
    ) }

    fun replaceWorkspaceAndSettings(
        workspace: WorkspaceDescriptor,
        settings: ReminderSettingsSnapshot,
    ) = synchronized(LOCK) {
        if (!workspace.sameWorkspaceAs(activeWorkspace())) deactivate()
        val editor = preferences.edit()
            .putString(KEY_WORKSPACE_ID, workspace.reminderWorkspaceId)
            .putBoolean(KEY_ENABLED, settings.enabled)
            .putLong(KEY_LEAD_SECONDS, settings.leadSeconds.coerceAtLeast(0L))
            .putBoolean(KEY_SHOW_ACTION_TITLE, settings.showActionTitle)
        when (workspace) {
            is WorkspaceDescriptor.Local -> editor
                .putString(KEY_WORKSPACE_KIND, WORKSPACE_LOCAL)
                .remove(KEY_ACCOUNT_ID)
            is WorkspaceDescriptor.Remote -> editor
                .putString(KEY_WORKSPACE_KIND, WORKSPACE_REMOTE)
                .putString(KEY_ACCOUNT_ID, workspace.accountId)
        }
        check(editor.commit()) { "Could not persist reminder workspace settings" }
    }

    fun records(): Map<String, ReminderLedgerRecord> = synchronized(LOCK) {
        recordIds().mapNotNull { actionId ->
            readRecord(actionId)?.let { actionId to it }
        }.toMap()
    }

    fun put(record: ReminderLedgerRecord) = synchronized(LOCK) {
        val ids = recordIds() + record.actionId
        check(
            preferences.edit()
                .putStringSet(KEY_RECORD_IDS, ids)
                .putString(recordKey(record.actionId), encode(record))
                .commit(),
        ) { "Could not persist reminder work" }
    }

    fun remove(actionId: String) = synchronized(LOCK) {
        val ids = recordIds() - actionId
        check(
            preferences.edit()
                .putStringSet(KEY_RECORD_IDS, ids)
                .remove(recordKey(actionId))
                .commit(),
        ) { "Could not remove reminder work" }
    }

    fun removeAll() = synchronized(LOCK) {
        val editor = preferences.edit().remove(KEY_RECORD_IDS)
        recordIds().forEach { actionId -> editor.remove(recordKey(actionId)) }
        check(editor.commit()) { "Could not clear reminder work" }
    }

    private fun recordIds(): Set<String> = preferences.getStringSet(KEY_RECORD_IDS, emptySet())
        ?.toSet()
        .orEmpty()

    private fun readRecord(actionId: String): ReminderLedgerRecord? = preferences
        .getString(recordKey(actionId), null)
        ?.let { encoded -> decode(actionId, encoded) }

    private fun recordKey(actionId: String): String = KEY_RECORD_PREFIX + encodeText(actionId)

    private fun encode(record: ReminderLedgerRecord): String = listOf(
        encodeText(record.expectedStart),
        record.triggerAtEpochMillis.toString(),
        encodeText(record.workName),
        record.notificationId.toString(),
    ).joinToString(FIELD_SEPARATOR)

    private fun decode(actionId: String, value: String): ReminderLedgerRecord? {
        val fields = value.split(FIELD_SEPARATOR)
        if (fields.size != 4) return null
        return runCatching {
            ReminderLedgerRecord(
                actionId = actionId,
                expectedStart = decodeText(fields[0]),
                triggerAtEpochMillis = fields[1].toLong(),
                workName = decodeText(fields[2]),
                notificationId = fields[3].toInt(),
            )
        }.getOrNull()
    }

    companion object {
        private val LOCK = Any()
        private const val PREFERENCES_NAME = "action-reminders"
        private const val KEY_WORKSPACE_KIND = "active-workspace-kind"
        private const val KEY_WORKSPACE_ID = "active-workspace-id"
        private const val KEY_ACCOUNT_ID = "active-account-id"
        private const val KEY_ENABLED = "notifications-enabled"
        private const val KEY_LEAD_SECONDS = "notification-lead-seconds"
        private const val KEY_SHOW_ACTION_TITLE = "notification-show-action-title"
        private const val KEY_RECORD_IDS = "scheduled-action-ids"
        private const val KEY_RECORD_PREFIX = "scheduled-action-"
        private const val WORKSPACE_LOCAL = "local"
        private const val WORKSPACE_REMOTE = "remote"
        private const val FIELD_SEPARATOR = "."

        fun workName(workspaceId: String, actionId: String): String =
            "subroutine-action-reminder-${digest("$workspaceId\u0000$actionId")}"

        fun actionWorkName(workspaceId: String, actionId: String): String =
            "subroutine-reminder-action-${digest("$workspaceId\u0000$actionId")}"

        fun notificationId(workspaceId: String, actionId: String): Int {
            val bytes = MessageDigest.getInstance("SHA-256")
                .digest("$workspaceId\u0000$actionId".toByteArray(StandardCharsets.UTF_8))
            return ByteBuffer.wrap(bytes).int and Int.MAX_VALUE
        }

        private fun digest(value: String): String = MessageDigest.getInstance("SHA-256")
            .digest(value.toByteArray(StandardCharsets.UTF_8))
            .joinToString("") { byte -> "%02x".format(byte.toInt() and 0xff) }

        private fun encodeText(value: String): String = Base64.getUrlEncoder()
            .withoutPadding()
            .encodeToString(value.toByteArray(StandardCharsets.UTF_8))

        private fun decodeText(value: String): String = String(
            Base64.getUrlDecoder().decode(value),
            StandardCharsets.UTF_8,
        )
    }
}

internal fun WorkspaceDescriptor.sameWorkspaceAs(other: WorkspaceDescriptor?): Boolean = when {
    other == null -> false
    this is WorkspaceDescriptor.Local && other is WorkspaceDescriptor.Local -> workspaceId == other.workspaceId
    this is WorkspaceDescriptor.Remote && other is WorkspaceDescriptor.Remote ->
        workspaceId == other.workspaceId && accountId == other.accountId
    else -> false
}
