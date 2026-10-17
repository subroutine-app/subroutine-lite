package com.example.subroutine_simple.ui.home

import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.blocksTime
import com.example.subroutine_simple.data.models.isQueued
import com.example.subroutine_simple.ui.agenda.AgendaTime
import com.example.subroutine_simple.ui.agenda.parseAgendaTime
import com.example.subroutine_simple.ui.focus.DefaultFocusHorizon
import com.example.subroutine_simple.ui.focus.FocusEventMoment
import com.example.subroutine_simple.ui.focus.projectFocus
import java.time.Duration
import java.time.Instant
import java.time.ZoneId

val DefaultHomeBlockingWindow: Duration = Duration.ofMinutes(5)

data class HomeProjection(
    val nextAction: Action?,
    val event: FocusEventMoment?,
) {
    val isEmpty: Boolean get() = nextAction == null && event == null
}

// Queue eligibility is independent of Focus's action lookahead. Only the event moments are shared.
// Android currently supplies stored events only, without recurrence expansion.
fun projectHome(
    actions: List<Action>,
    events: List<Event>,
    now: Instant,
    zoneId: ZoneId,
    blockingWindow: Duration = DefaultHomeBlockingWindow,
    eventHorizon: Duration = DefaultFocusHorizon,
): HomeProjection {
    require(!blockingWindow.isNegative && !eventHorizon.isNegative)
    val moments = projectFocus(
        actions = emptyList(),
        events = events,
        signals = emptyList(),
        now = now,
        zoneId = zoneId,
        horizon = maxOf(eventHorizon, blockingWindow),
    ).events
    // A free active event can hide an upcoming busy event from the preview group.
    val blocked = projectFocus(
        actions = emptyList(),
        events = events.filter { it.blocksTime },
        signals = emptyList(),
        now = now,
        zoneId = zoneId,
        horizon = blockingWindow,
        noticeThreshold = blockingWindow,
    ).events.isNotEmpty()
    val next = if (blocked) null else actions.asSequence()
        .mapNotNull { eligibleAction(it, now, zoneId) }
        .minWithOrNull(compareBy<EligibleAction>({ it.priority }, { it.scheduled }, { it.action.id }))
        ?.action
    return HomeProjection(next, moments.firstOrNull())
}

private enum class ActionPriority { Scheduled, Today, Unscheduled }

private data class EligibleAction(
    val action: Action,
    val priority: ActionPriority,
    val scheduled: Instant?,
)

private fun eligibleAction(action: Action, now: Instant, zoneId: ZoneId): EligibleAction? {
    if (!action.isQueued) return null
    val start = action.start ?: return EligibleAction(action, ActionPriority.Unscheduled, null)
    return when (val schedule = parseAgendaTime(start) ?: return null) {
        is AgendaTime.Timed -> if (schedule.instant <= now) {
            EligibleAction(action, ActionPriority.Scheduled, schedule.instant)
        } else null

        is AgendaTime.Day -> {
            val today = now.atZone(zoneId).toLocalDate()
            if (schedule.date > today) return null
            EligibleAction(
                action,
                if (schedule.date == today) ActionPriority.Today else ActionPriority.Scheduled,
                schedule.date.atStartOfDay(zoneId).toInstant(),
            )
        }
    }
}

internal fun homeDuration(duration: Duration): String {
    val minutes = duration.toMinutes().coerceAtLeast(0)
    val hours = minutes / 60
    return when {
        minutes == 0L -> "<1 min"
        hours == 0L -> "$minutes min"
        minutes % 60 == 0L -> "$hours hr"
        else -> "$hours hr ${minutes % 60} min"
    }
}
