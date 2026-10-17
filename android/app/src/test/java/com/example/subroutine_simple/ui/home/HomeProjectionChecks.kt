package com.example.subroutine_simple.ui.home

import com.example.subroutine_simple.auth.AuthenticationRecovery
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.EventTemplate
import com.example.subroutine_simple.data.models.ResourceValue
import com.example.subroutine_simple.data.models.asTemplate
import com.example.subroutine_simple.data.models.blocksTime
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor
import com.example.subroutine_simple.ui.focus.FocusEventMoment
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.encodeToJsonElement
import kotlinx.serialization.json.jsonObject
import java.time.Duration
import java.time.Instant
import java.time.ZoneId
import java.time.ZoneOffset

// Run with the debug unit-test runtime classpath, like CompoundAgendaChecks.
object HomeProjectionChecks {
    @JvmStatic
    fun main(args: Array<String>) {
        onlyEligibleActionsAreRanked()
        eligibilityUsesLocalDatesAndExactInstants()
        previewLookaheadDoesNotBlockEarly()
        blockingIncludesThresholdAndStartButExcludesEnd()
        previewPrefersEarliestActiveStart()
        onlyValidStoredEventsWithinTheHorizonAppear()
        cachedWorkspacesRemainWritableOnlyForTheirOwner()
        optionalAvailabilitySurvivesWireCopiesAndTemplates()
        freePreviewsDoNotHideBusyBlockers()
        availabilityOverridesControlReadiness()
        println("Home projection checks passed")
    }

    private val now = Instant.parse("2026-10-15T12:00:00Z")
    private val utc = ZoneOffset.UTC

    private fun action(id: String, start: String? = null) = Action(
        id = id, recurrenceId = id, title = id, queued = true, start = start,
    )

    private fun event(id: String, start: Instant, duration: Duration) = Event(
        id = id, lineageId = id, title = id, start = start.toString(), duration = duration.toString(),
    )

    private fun onlyEligibleActionsAreRanked() {
        val eligible = listOf(
            action("missed-date", "2026-10-14"),
            action("missed-time", "2026-10-14T15:00:00Z"),
            action("due-a", now.toString()),
            action("due-b", now.toString()),
            action("today-a", "2026-10-15"),
            action("today-b", "2026-10-15"),
            action("any-a"),
            action("any-b"),
        )
        val ineligible = listOf(
            action("future-time", now.plusNanos(1).toString()),
            action("future-day", "2026-10-16"),
            action("backlog").copy(queued = false),
            action("backlog-date", "2026-10-14").copy(queued = false),
            action("completed", "2026-10-14").copy(completion = now.toString()),
            action("invalid", "not-a-date"),
        )
        var remaining = (eligible + ineligible).reversed()
        eligible.forEach { expected ->
            check(projectHome(remaining, emptyList(), now, utc).nextAction == expected) {
                "Due/missed work must precede today's date-only actions and queued unscheduled actions"
            }
            remaining = remaining.filterNot { it.id == expected.id }
        }
        check(projectHome(remaining, emptyList(), now, utc).isEmpty)
    }

    private fun eligibilityUsesLocalDatesAndExactInstants() {
        val zone = ZoneId.of("America/Los_Angeles")
        val localEvening = Instant.parse("2026-10-16T01:00:00Z")
        val tomorrow = action("tomorrow", "2026-10-16")
        val today = action("today", "2026-10-15")
        val timed = action("timed", "2026-10-15T18:00:00-07:00")
        check(projectHome(listOf(tomorrow), emptyList(), localEvening, zone).isEmpty)
        check(projectHome(listOf(tomorrow, today), emptyList(), localEvening, zone).nextAction == today)
        check(projectHome(listOf(today, timed), emptyList(), localEvening, zone).nextAction == timed)
        check(projectHome(listOf(today, timed), emptyList(), localEvening.minusNanos(1), zone).nextAction == today)
    }

    private fun previewLookaheadDoesNotBlockEarly() {
        val action = action("now")
        val event = event("later", now.plusSeconds(600), Duration.ofMinutes(30))
        val projection = projectHome(listOf(action), listOf(event), now, utc)
        check(projection.nextAction == action)
        val preview = projection.event as FocusEventMoment.Upcoming
        check(preview.withinNoticeThreshold) {
            "Focus's 15-minute notice is not Home's five-minute queue blocker"
        }
        check(preview.until == Duration.ofMinutes(10))
    }

    private fun blockingIncludesThresholdAndStartButExcludesEnd() {
        val action = action("next")
        val event = event("meeting", now, Duration.ofMinutes(30))
        fun at(instant: Instant) = projectHome(listOf(action), listOf(event), instant, utc)
        val threshold = now.minus(DefaultHomeBlockingWindow)
        check(at(threshold.minusNanos(1)).nextAction == action)
        check(at(threshold).nextAction == null)
        val started = at(now)
        check(started.nextAction == null)
        check((started.event as FocusEventMoment.InProgress).progress == 0f)
        val end = now.plusSeconds(1800)
        check(at(end.minusNanos(1)).nextAction == null)
        check(at(end).nextAction == action && at(end).event == null)
        val narrowPreview = projectHome(listOf(action), listOf(event), threshold, utc, eventHorizon = Duration.ZERO)
        check(narrowPreview.event != null) { "A queue blocker must not produce a false empty creation slot" }
    }

    private fun previewPrefersEarliestActiveStart() {
        val earliest = event("earliest", now.minusSeconds(1200), Duration.ofMinutes(40))
        val endingFirst = event("ending-first", now.minusSeconds(300), Duration.ofMinutes(10))
        val upcoming = event("upcoming", now.plusSeconds(60), Duration.ofMinutes(20))
        val projection = projectHome(listOf(action("next")), listOf(upcoming, endingFirst, earliest), now, utc)
        check(projection.nextAction == null)
        val preview = projection.event as FocusEventMoment.InProgress
        check(preview.event == earliest) { "Home previews the first active event, not the shortest blocker" }
        check(preview.progress == 0.5f && preview.remaining == Duration.ofMinutes(20))
    }

    private fun onlyValidStoredEventsWithinTheHorizonAppear() {
        val horizon = now.plus(Duration.ofHours(24))
        val boundary = event("boundary", horizon, Duration.ofMinutes(30))
        check(projectHome(emptyList(), listOf(boundary), now, utc).event?.event == boundary)
        check(
            projectHome(
                emptyList(),
                listOf(boundary.copy(start = horizon.plusNanos(1).toString())),
                now,
                utc
            ).isEmpty
        )
        val ended = event("ended", now.minusSeconds(3600), Duration.ofMinutes(30))
            .copy(recurrence = JsonObject(emptyMap()))
        val malformed = listOf(
            ended,
            boundary.copy(start = "2026-10-15"),
            boundary.copy(start = now.toString(), duration = "PT0S"),
            boundary.copy(start = now.toString(), duration = "invalid"),
        )
        check(projectHome(emptyList(), malformed, now, utc).isEmpty) {
            "Invalid schedules and unexpanded recurrences must not invent active events"
        }
    }

    private fun optionalAvailabilitySurvivesWireCopiesAndTemplates() {
        val json = Json { encodeDefaults = true; ignoreUnknownKeys = true }
        val legacy =
            """{"id":"event","lineage_id":"lineage","title":"Meeting","start":"2026-10-15T12:00:00Z","duration":"PT30M"}"""
        val missing = json.decodeFromString<Event>(legacy)
        check(missing.sourceBusy == null && missing.busyOverride == null && missing.blocksTime)
        val absentTemplate = json.decodeFromString<EventTemplate>(
            """{"id":"template","lineage_id":"lineage","title":"Meeting","duration":"PT30M"}""",
        )
        check(absentTemplate.sourceBusy == null && absentTemplate.busyOverride == null)
        val explicitNull = json.decodeFromString<Event>(
            legacy.dropLast(1) + """, "source_busy":null, "busy_override":null}""",
        )
        check(explicitNull == missing)

        val sourceFree = json.decodeFromString<Event>(legacy.dropLast(1) + """, "source_busy":false}""")
        val forcedBusy = json.decodeFromString<Event>(
            legacy.dropLast(1) + """, "source_busy":false, "busy_override":true}""",
        )
        val forcedFree = json.decodeFromString<Event>(
            legacy.dropLast(1) + """, "source_busy":true, "busy_override":false}""",
        )
        check(!sourceFree.blocksTime && forcedBusy.blocksTime && !forcedFree.blocksTime)
        for (original in listOf(missing, sourceFree, forcedBusy, forcedFree)) {
            val edited = original.copy(title = "Edited", duration = "PT45M")
            check(json.decodeFromString<Event>(json.encodeToString(edited)) == edited)
            val wire = json.encodeToJsonElement(edited).jsonObject
            check(wire["source_busy"] == (original.sourceBusy?.let { JsonPrimitive(it) } ?: JsonNull))
            check(wire["busy_override"] == (original.busyOverride?.let { JsonPrimitive(it) } ?: JsonNull))
            val copied = original.copy(id = "copy", lineageId = "copy", sourceProvider = null, sourceExternalId = null)
            check(copied.sourceBusy == original.sourceBusy && copied.busyOverride == original.busyOverride)

            val template = edited.asTemplate("template").copy(title = "Reusable")
            check(template.sourceBusy == original.sourceBusy && template.busyOverride == original.busyOverride)
            check(json.decodeFromString<EventTemplate>(json.encodeToString(template)) == template)
            val resources = listOf<ResourceValue>(ResourceValue.Event(edited), ResourceValue.EventTemplate(template))
            resources.forEach { resource ->
                check(json.decodeFromString<ResourceValue>(json.encodeToString(resource)) == resource) {
                    "Outbox and snapshot resource codecs must retain both nullable availability fields"
                }
            }
        }
    }

    private fun freePreviewsDoNotHideBusyBlockers() {
        val action = action("next")
        val free = event("free", now.minusSeconds(600), Duration.ofHours(1)).copy(sourceBusy = false)
        fun with(vararg events: Event) = projectHome(listOf(action), events.toList(), now, utc)
        check(with(free).let { it.nextAction == action && it.event?.event == free })

        val activeBusy = event("active-busy", now.minusSeconds(60), Duration.ofMinutes(2))
        check(with(free, activeBusy).let { it.nextAction == null && it.event?.event == free })
        val upcomingBusy = event("upcoming-busy", now.plus(DefaultHomeBlockingWindow), Duration.ofMinutes(30))
        check(with(free, upcomingBusy).let { it.nextAction == null && it.event?.event == free }) {
            "An active free preview must not hide a busy event inside the blocking window"
        }
        val upcomingFree = free.copy(start = now.plusSeconds(60).toString(), duration = "PT1M")
        check(with(upcomingFree, upcomingBusy).let { it.nextAction == null && it.event?.event == upcomingFree }) {
            "A later non-overlapping busy group must still block, even when only the first free group is previewed"
        }
        check(
            with(
                free,
                upcomingBusy.copy(start = now.plus(DefaultHomeBlockingWindow).plusNanos(1).toString())
            ).nextAction == action
        )
        val finishedBusy = activeBusy.copy(duration = "PT1M")
        check(with(free, finishedBusy).nextAction == action)
    }

    private fun availabilityOverridesControlReadiness() {
        val action = action("next")
        val event = event("event", now, Duration.ofMinutes(30))
        fun next(event: Event) = projectHome(listOf(action), listOf(event), now, utc).nextAction
        check(next(event) == null)
        check(next(event.copy(sourceBusy = false)) == action)
        check(next(event.copy(sourceBusy = false, busyOverride = true)) == null)
        check(next(event.copy(sourceBusy = true, busyOverride = false)) == action)
        check(next(event.copy(busyOverride = false)) == action)
    }

    private fun cachedWorkspacesRemainWritableOnlyForTheirOwner() {
        val local = WorkspaceDescriptor.Local("local")
        val remote = WorkspaceDescriptor.Remote("remote", "account-a")
        check(homeWorkspaceWritable(AuthenticationState.SignedOut, local, localOnly = true))
        check(!homeWorkspaceWritable(AuthenticationState.Initializing, local, localOnly = true))
        check(!homeWorkspaceWritable(AuthenticationState.SignedOut, remote, localOnly = false))
        check(
            homeWorkspaceWritable(
                AuthenticationState.InteractionRequired(null, "account-a", 2),
                remote,
                localOnly = false
            )
        )
        check(
            homeWorkspaceWritable(
                AuthenticationState.Error("Offline", "account-a", 2, retryable = true),
                remote,
                localOnly = false
            )
        )
        check(!homeWorkspaceWritable(AuthenticationState.SignedIn(null, "account-b", 2), remote, localOnly = false))
        check(!homeWorkspaceWritable(AuthenticationState.SignedIn(null, "account-a", 2), remote, localOnly = true))
        check(
            !homeWorkspaceWritable(
                AuthenticationState.Error("Sign out failed", "account-a", 2, recovery = AuthenticationRecovery.SignOut),
                remote, localOnly = false,
            )
        )
        val recurring = action("recurring").copy(recurrence = JsonObject(emptyMap()))
        check(!homeCompletionAvailable(recurring, localOnly = true))
        check(homeCompletionAvailable(recurring, localOnly = false))
        check(homeCompletionAvailable(action("one-off"), localOnly = true))
    }
}
