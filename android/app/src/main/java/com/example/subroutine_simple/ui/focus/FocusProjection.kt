package com.example.subroutine_simple.ui.focus

import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.data.models.parseIsoDurationSeconds
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.util.Locale

val DefaultFocusHorizon: Duration = Duration.ofHours(24)
val DefaultFocusNoticeThreshold: Duration = Duration.ofMinutes(15)
val FocusSignalRecentWindow: Duration = Duration.ofMinutes(2)

@kotlinx.serialization.Serializable
enum class FocusMode {
    Actions,
    Events,
}

sealed interface FocusActionSchedule {
    val original: String
    val sortInstant: Instant

    data class Timed(
        override val original: String,
        val instant: Instant,
        val local: ZonedDateTime,
    ) : FocusActionSchedule {
        override val sortInstant: Instant = instant
    }

    data class DateOnly(
        override val original: String,
        val date: LocalDate,
        val zoneId: ZoneId,
    ) : FocusActionSchedule {
        override val sortInstant: Instant = date.atStartOfDay(zoneId).toInstant()
    }
}

data class FocusAction(
    val key: String,
    val action: Action,
    val schedule: FocusActionSchedule,
)

sealed interface FocusEventMoment {
    val key: String
    val event: Event
    val startsAt: ZonedDateTime
    val endsAt: ZonedDateTime
    val countdown: Duration
    val countdownSeconds: Long
        get() = focusCountdownSeconds(countdown)

    data class InProgress(
        override val key: String,
        override val event: Event,
        override val startsAt: ZonedDateTime,
        override val endsAt: ZonedDateTime,
        val progress: Float,
        val remaining: Duration,
    ) : FocusEventMoment {
        override val countdown: Duration = remaining
    }

    data class Upcoming(
        override val key: String,
        override val event: Event,
        override val startsAt: ZonedDateTime,
        override val endsAt: ZonedDateTime,
        val until: Duration,
        val withinNoticeThreshold: Boolean,
    ) : FocusEventMoment {
        override val countdown: Duration = until
    }
}

sealed interface FocusSignalNotice {
    val key: String
    val signal: Signal
    val occursAt: ZonedDateTime
    val timing: Duration
    val timingSeconds: Long
        get() = focusCountdownSeconds(timing)

    data class Recent(
        override val key: String,
        override val signal: Signal,
        override val occursAt: ZonedDateTime,
        val elapsed: Duration,
    ) : FocusSignalNotice {
        override val timing: Duration = elapsed
    }

    data class Upcoming(
        override val key: String,
        override val signal: Signal,
        override val occursAt: ZonedDateTime,
        val until: Duration,
    ) : FocusSignalNotice {
        override val timing: Duration = until
    }
}

data class FocusProjection(
    val actions: List<FocusAction>,
    val events: List<FocusEventMoment>,
    val signalNotice: FocusSignalNotice?,
    val horizon: Duration,
    val noticeThreshold: Duration,
)

fun projectFocus(
    actions: List<Action>,
    events: List<Event>,
    signals: List<Signal>,
    now: Instant = Instant.now(),
    zoneId: ZoneId = ZoneId.systemDefault(),
    horizon: Duration = DefaultFocusHorizon,
    noticeThreshold: Duration = DefaultFocusNoticeThreshold,
    dismissedSignalKeys: Set<String> = emptySet(),
): FocusProjection {
    require(!horizon.isNegative) { "Focus horizon must not be negative" }
    require(!noticeThreshold.isNegative) { "Focus notice threshold must not be negative" }

    val horizonEnd = now.plus(horizon)
    val horizonDate = horizonEnd.atZone(zoneId).toLocalDate()
    val projectedActions = actions.mapNotNull { action ->
        if (!action.queued || action.completion != null) return@mapNotNull null
        val start = action.start ?: return@mapNotNull null
        val schedule = parseActionSchedule(start, zoneId) ?: return@mapNotNull null
        val isVisible = when (schedule) {
            is FocusActionSchedule.DateOnly -> schedule.date <= horizonDate
            is FocusActionSchedule.Timed -> schedule.instant <= horizonEnd
        }
        if (!isVisible) return@mapNotNull null
        FocusAction(
            key = stableFocusKey("action", action.id),
            action = action,
            schedule = schedule,
        )
    }.sortedWith(
        compareBy<FocusAction> { it.schedule.sortInstant }
            .thenBy { it.key },
    )

    val eventCandidates = events.mapNotNull { event ->
        val start = parseTimestamp(event.start) ?: return@mapNotNull null
        val durationSeconds = parseIsoDurationSeconds(event.duration)
            ?.takeIf { it > 0 }
            ?: return@mapNotNull null
        val end = runCatching { start.plusSeconds(durationSeconds) }.getOrNull()
            ?: return@mapNotNull null
        EventCandidate(event, start, end)
    }.sortedWith(
        compareBy<EventCandidate>({ it.start }, { it.end }, { stableFocusKey("event", it.event.id) }),
    )

    val activeEvents = eventCandidates.filter { it.start <= now && now < it.end }
    val projectedEvents = if (activeEvents.isNotEmpty()) {
        activeEvents.map { it.toInProgress(now, zoneId) }
    } else {
        val anchor = eventCandidates
            .filter { it.start > now && it.start <= horizonEnd }
            .minWithOrNull(compareBy<EventCandidate>({ it.start }, { stableFocusKey("event", it.event.id) }))
        if (anchor == null) {
            emptyList()
        } else {
            eventCandidates
                .asSequence()
                .filter { candidate ->
                    candidate.start > now &&
                            candidate.start <= horizonEnd &&
                            (candidate.start == anchor.start ||
                                    (candidate.start < anchor.end && candidate.end > anchor.start))
                }
                .map { it.toUpcoming(now, zoneId, noticeThreshold) }
                .toList()
        }
    }

    val signalNotice = selectSignalNotice(signals, now, zoneId, noticeThreshold)
        ?.takeUnless { it.key in dismissedSignalKeys }

    return FocusProjection(
        actions = projectedActions,
        events = projectedEvents,
        signalNotice = signalNotice,
        horizon = horizon,
        noticeThreshold = noticeThreshold,
    )
}

fun focusCountdownSeconds(duration: Duration): Long {
    if (duration.isNegative || duration.isZero) return 0
    val seconds = duration.seconds
    return if (duration.nano == 0) seconds else seconds + 1
}

fun formatFocusCountdown(duration: Duration): String {
    val totalSeconds = focusCountdownSeconds(duration)
    val hours = totalSeconds / 3_600
    val minutes = (totalSeconds % 3_600) / 60
    val seconds = totalSeconds % 60
    return if (hours > 0) {
        String.format(Locale.ROOT, "%02d:%02d:%02d", hours, minutes, seconds)
    } else {
        String.format(Locale.ROOT, "%02d:%02d", minutes, seconds)
    }
}

private data class EventCandidate(
    val event: Event,
    val start: Instant,
    val end: Instant,
) {
    fun toInProgress(now: Instant, zoneId: ZoneId): FocusEventMoment.InProgress {
        val totalMillis = Duration.between(start, end).toMillis()
        val elapsedMillis = Duration.between(start, now).toMillis()
        val progress = if (totalMillis <= 0) {
            1f
        } else {
            (elapsedMillis.toDouble() / totalMillis.toDouble()).coerceIn(0.0, 1.0).toFloat()
        }
        return FocusEventMoment.InProgress(
            key = stableFocusKey("event", event.id),
            event = event,
            startsAt = start.atZone(zoneId),
            endsAt = end.atZone(zoneId),
            progress = progress,
            remaining = Duration.between(now, end),
        )
    }

    fun toUpcoming(
        now: Instant,
        zoneId: ZoneId,
        noticeThreshold: Duration,
    ): FocusEventMoment.Upcoming {
        val until = Duration.between(now, start)
        return FocusEventMoment.Upcoming(
            key = stableFocusKey("event", event.id),
            event = event,
            startsAt = start.atZone(zoneId),
            endsAt = end.atZone(zoneId),
            until = until,
            withinNoticeThreshold = until <= noticeThreshold,
        )
    }
}

private data class SignalCandidate(
    val signal: Signal,
    val instant: Instant,
)

private fun selectSignalNotice(
    signals: List<Signal>,
    now: Instant,
    zoneId: ZoneId,
    noticeThreshold: Duration,
): FocusSignalNotice? {
    val recentStart = now.minus(FocusSignalRecentWindow)
    val noticeEnd = now.plus(noticeThreshold)
    val candidates = signals.mapNotNull { signal ->
        val instant = parseTimestamp(signal.datetime) ?: return@mapNotNull null
        if (instant < recentStart || instant > noticeEnd) return@mapNotNull null
        SignalCandidate(signal, instant)
    }

    val recent = candidates
        .filter { it.instant <= now }
        .maxWithOrNull(compareBy<SignalCandidate>({ it.instant }, { stableFocusKey("signal", it.signal.id) }))
    if (recent != null) {
        return FocusSignalNotice.Recent(
            key = stableFocusKey("signal", recent.signal.id),
            signal = recent.signal,
            occursAt = recent.instant.atZone(zoneId),
            elapsed = Duration.between(recent.instant, now),
        )
    }

    val upcoming = candidates
        .filter { it.instant > now }
        .minWithOrNull(compareBy<SignalCandidate>({ it.instant }, { stableFocusKey("signal", it.signal.id) }))
        ?: return null
    return FocusSignalNotice.Upcoming(
        key = stableFocusKey("signal", upcoming.signal.id),
        signal = upcoming.signal,
        occursAt = upcoming.instant.atZone(zoneId),
        until = Duration.between(now, upcoming.instant),
    )
}

private fun parseActionSchedule(value: String, zoneId: ZoneId): FocusActionSchedule? {
    if ('T' !in value) {
        val date = runCatching { LocalDate.parse(value) }.getOrNull() ?: return null
        return FocusActionSchedule.DateOnly(value, date, zoneId)
    }
    val instant = parseTimestamp(value) ?: return null
    return FocusActionSchedule.Timed(value, instant, instant.atZone(zoneId))
}

private fun parseTimestamp(value: String): Instant? {
    if ('T' !in value) return null
    return runCatching { Instant.parse(value) }
        .recoverCatching { OffsetDateTime.parse(value).toInstant() }
        .getOrNull()
}

private fun stableFocusKey(kind: String, id: String): String = "$kind:$id"
