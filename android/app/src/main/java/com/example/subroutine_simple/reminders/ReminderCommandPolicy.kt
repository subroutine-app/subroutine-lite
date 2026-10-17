package com.example.subroutine_simple.reminders

import android.os.Build
import androidx.work.OutOfQuotaPolicy
import com.example.subroutine_simple.data.local.ReminderCommandStatus
import java.time.Instant

internal fun commandExpeditedPolicy(sdkInt: Int, userInitiated: Boolean): OutOfQuotaPolicy? =
    if (userInitiated && sdkInt >= Build.VERSION_CODES.S) OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST else null

internal fun canTransitionCommand(current: CommandState, next: CommandState): Boolean =
    !current.terminal || (current == CommandState.Expired && next in setOf(CommandState.Confirmed, CommandState.Superseded))

internal enum class CommandRecovery { None, Execute, Expire }

internal fun commandRecovery(
    state: CommandState,
    retryUntil: Instant,
    now: Instant,
    resumeAuthentication: Boolean,
): CommandRecovery = when {
    state.terminal -> CommandRecovery.None
    now >= retryUntil -> CommandRecovery.Expire
    state == CommandState.Conflict -> CommandRecovery.None
    state == CommandState.AuthenticationRequired && !resumeAuthentication -> CommandRecovery.None
    else -> CommandRecovery.Execute
}

internal fun statusOnlyState(current: CommandState, status: ReminderCommandStatus?, retryUntil: Instant, now: Instant): CommandState = when (status) {
    ReminderCommandStatus.CONFIRMED -> CommandState.Confirmed
    ReminderCommandStatus.SUPERSEDED -> CommandState.Superseded
    ReminderCommandStatus.CONFLICT -> if (current.terminal) current else CommandState.Conflict
    ReminderCommandStatus.QUEUED -> if (current == CommandState.Conflict && now < retryUntil) CommandState.Queued else current
    null -> current
}
