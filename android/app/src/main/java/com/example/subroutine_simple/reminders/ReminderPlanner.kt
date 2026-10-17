package com.example.subroutine_simple.reminders

import com.example.subroutine_simple.data.models.Action
import java.time.Instant
import java.time.OffsetDateTime

data class ActionReminderPlan(
    val actionId: String,
    val title: String,
    val start: Instant,
    val expectedStart: String,
    val triggerAt: Instant,
)

fun planActionReminders(
    actions: List<Action>,
    now: Instant,
    leadSeconds: Long,
): List<ActionReminderPlan> {
    val safeLeadSeconds = leadSeconds.coerceAtLeast(0L)
    return actions.asSequence()
        .filter { action -> action.queued && action.completion == null }
        .mapNotNull { action ->
            val expectedStart = action.start ?: return@mapNotNull null
            val start = parseTimestamp(expectedStart) ?: return@mapNotNull null
            if (runCatching { start.plusSeconds(300) <= now }.getOrDefault(true)) return@mapNotNull null
            val requestedTrigger = runCatching { start.minusSeconds(safeLeadSeconds) }
                .getOrDefault(Instant.MIN)
            ActionReminderPlan(
                actionId = action.id,
                title = action.title,
                start = start,
                expectedStart = expectedStart,
                triggerAt = requestedTrigger,
            )
        }
        .sortedWith(compareBy(ActionReminderPlan::triggerAt, ActionReminderPlan::start, ActionReminderPlan::actionId))
        .toList()
}

internal fun parseReminderTimestamp(value: String?): Instant? = value?.let(::parseTimestamp)

private fun parseTimestamp(value: String): Instant? = runCatching { Instant.parse(value) }
    .recoverCatching { OffsetDateTime.parse(value).toInstant() }
    .getOrNull()
