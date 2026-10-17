package com.example.subroutine_simple.reminders

import android.content.Intent
import androidx.work.Data
import androidx.work.workDataOf
import com.example.subroutine_simple.BuildConfig
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor

const val ACTION_OPEN_REMINDER = "${BuildConfig.APPLICATION_ID}.reminders.OPEN"
internal const val ACTION_COMPLETE_REMINDER = "${BuildConfig.APPLICATION_ID}.reminders.COMPLETE"
internal const val ACTION_UNQUEUE_REMINDER = "${BuildConfig.APPLICATION_ID}.reminders.UNQUEUE"

const val EXTRA_ACTION_ID = "com.example.subroutine_simple.extra.ACTION_ID"
const val EXTRA_WORKSPACE_ID = "com.example.subroutine_simple.extra.WORKSPACE_ID"
const val EXTRA_WORKSPACE_KIND = "com.example.subroutine_simple.extra.WORKSPACE_KIND"
const val EXTRA_ACCOUNT_ID = "com.example.subroutine_simple.extra.ACCOUNT_ID"
internal const val EXTRA_NOTIFICATION_ID = "com.example.subroutine_simple.extra.NOTIFICATION_ID"

internal const val INPUT_EXPECTED_START = "expected-start"
internal const val INPUT_OPERATION = "operation"
internal const val INPUT_REMINDER_EPOCH = "reminder-epoch"

@kotlinx.serialization.Serializable
internal enum class ReminderOperation(
    val intentAction: String,
    val path: String,
    val requestCodeSalt: Int,
) {
    Complete(ACTION_COMPLETE_REMINDER, "complete", 0x434f4d50),
    Unqueue(ACTION_UNQUEUE_REMINDER, "unqueue", 0x554e5155),
}

internal val WorkspaceDescriptor.reminderWorkspaceId: String
    get() = when (this) {
        is WorkspaceDescriptor.Local -> workspaceId
        is WorkspaceDescriptor.Remote -> workspaceId
    }

internal fun Intent.putWorkspace(workspace: WorkspaceDescriptor): Intent {
    putExtra(EXTRA_WORKSPACE_ID, workspace.reminderWorkspaceId)
    when (workspace) {
        is WorkspaceDescriptor.Local -> putExtra(EXTRA_WORKSPACE_KIND, "local")
        is WorkspaceDescriptor.Remote -> {
            putExtra(EXTRA_WORKSPACE_KIND, "remote")
            putExtra(EXTRA_ACCOUNT_ID, workspace.accountId)
        }
    }
    return this
}

internal fun Intent.workspaceDescriptor(): WorkspaceDescriptor? = workspaceDescriptor(
    kind = getStringExtra(EXTRA_WORKSPACE_KIND),
    workspaceId = getStringExtra(EXTRA_WORKSPACE_ID),
    accountId = getStringExtra(EXTRA_ACCOUNT_ID),
)

internal fun reminderInputData(
    workspace: WorkspaceDescriptor,
    actionId: String,
    expectedStart: String? = null,
    operation: ReminderOperation? = null,
    notificationId: Int? = null,
    epoch: String? = null,
): Data {
    val values = mutableMapOf<String, Any>(
        EXTRA_WORKSPACE_KIND to if (workspace is WorkspaceDescriptor.Local) "local" else "remote",
        EXTRA_WORKSPACE_ID to workspace.reminderWorkspaceId,
        EXTRA_ACTION_ID to actionId,
    )
    if (workspace is WorkspaceDescriptor.Remote) values[EXTRA_ACCOUNT_ID] = workspace.accountId
    if (expectedStart != null) values[INPUT_EXPECTED_START] = expectedStart
    if (operation != null) values[INPUT_OPERATION] = operation.name
    if (notificationId != null) values[EXTRA_NOTIFICATION_ID] = notificationId
    if (epoch != null) values[INPUT_REMINDER_EPOCH] = epoch
    return workDataOf(*values.map { it.key to it.value }.toTypedArray())
}

internal fun Data.workspaceDescriptor(): WorkspaceDescriptor? = workspaceDescriptor(
    kind = getString(EXTRA_WORKSPACE_KIND),
    workspaceId = getString(EXTRA_WORKSPACE_ID),
    accountId = getString(EXTRA_ACCOUNT_ID),
)

private fun workspaceDescriptor(
    kind: String?,
    workspaceId: String?,
    accountId: String?,
): WorkspaceDescriptor? {
    if (workspaceId.isNullOrBlank()) return null
    return when (kind) {
        "local" -> WorkspaceDescriptor.Local(workspaceId)
        "remote" -> accountId?.takeIf(String::isNotBlank)?.let { WorkspaceDescriptor.Remote(workspaceId, it) }
        else -> null
    }
}
