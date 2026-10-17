package com.example.subroutine_simple.reminders


internal fun localFeedbackCanRender(
    command: LocalReminderCommand,
    state: CommandState,
    ownerCommandId: String?,
    renderedVersion: String?,
    currentExpectedStart: String?,
): Boolean {
    if (currentExpectedStart != null && parseReminderTimestamp(currentExpectedStart) != parseReminderTimestamp(command.expectedStart)) return false
    if (ownerCommandId != null) return ownerCommandId == command.commandId
    if (renderedVersion?.startsWith("local-command:${command.commandId}:") == true) return true
    if (state.terminal) return false
    if (renderedVersion?.startsWith("local:${command.epoch}:${command.expectedStart}:") == true) return true
    return state == CommandState.Pending && renderedVersion == null && currentExpectedStart == command.expectedStart
}
