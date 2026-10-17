package com.example.subroutine_simple.ui.timeline

import android.os.Looper
import androidx.lifecycle.viewModelScope
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.data.network.SubroutineApi
import com.example.subroutine_simple.data.repository.SubroutineRepository
import com.example.subroutine_simple.data.repository.WorkspaceEditConflictException
import com.example.subroutine_simple.sync.AppServices
import com.example.subroutine_simple.ui.MainViewModel
import com.example.subroutine_simple.ui.timelineEditedAction
import com.example.subroutine_simple.ui.timelineEditedEvent
import com.example.subroutine_simple.ui.timelineEditedMarker
import com.example.subroutine_simple.ui.timelineEditedSignal
import java.lang.reflect.Proxy
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.ZoneOffset
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.LooperMode
import org.robolectric.annotation.Config
import org.robolectric.annotation.SQLiteMode

class TimelineEditTest {
    private val utc = ZoneOffset.UTC
    private val berlin = ZoneId.of("Europe/Berlin")
    private val origin = Instant.parse("2026-10-15T09:00:00Z")
    private val recurrence = Json.parseToJsonElement("{\"frequency\":\"weekly\"}")

    @Test
    fun moveSubtractsGrabOffsetBeforeSnappingAndKeepsPhysicalDuration() {
        val original = interval()
        val viewport = TimelineViewport(origin.plusSeconds(3600), 1.0)
        val moved = editTimelineInterval(
            original, TimelineEditMode.Move, viewport.timeAt(780f, 600f), 120.0, TimelineDivision.FifteenMinutes, utc,
        )
        assertEquals(origin.plusSeconds(3600), moved.start)
        assertEquals(original.endExclusive.plusSeconds(3600), moved.endExclusive)
        assertEquals(300f, viewport.yAt(moved.start, 600f), 0f)
        assertEquals(
            Duration.between(original.start, original.endExclusive),
            Duration.between(moved.start, moved.endExclusive)
        )
        assertEquals(original, moved.original)
        assertEquals(original.key, moved.key)
        assertTrue(moved.changed)
        assertFalse(
            editTimelineInterval(
                original, TimelineEditMode.Move, origin.plusSeconds(120), 120.0, TimelineDivision.Minute, utc,
            ).changed
        )
    }

    @Test
    fun resizingKeepsTheOppositeEndpointAndClampsCrossingToOneSecond() {
        val original = interval()
        val start = editTimelineInterval(
            original, TimelineEditMode.Start, origin.plusSeconds(720), 120.0, TimelineDivision.FiveMinutes, utc,
        )
        val end = editTimelineInterval(
            original, TimelineEditMode.End, origin.plusSeconds(2820), 120.0, TimelineDivision.FifteenMinutes, utc,
        )
        assertEquals(origin.plusSeconds(600), start.start)
        assertEquals(original.endExclusive, start.endExclusive)
        assertEquals(original.start, end.start)
        assertEquals(origin.plusSeconds(2700), end.endExclusive)
        val clampedStart = editTimelineInterval(
            original, TimelineEditMode.Start, origin.plusSeconds(7200), 0.0, TimelineDivision.Hour, utc,
        )
        val clampedEnd = editTimelineInterval(
            original, TimelineEditMode.End, origin.minusSeconds(7200), 0.0, TimelineDivision.Hour, utc,
        )
        assertEquals(original.endExclusive.minusSeconds(1), clampedStart.start)
        assertEquals(original.endExclusive, clampedStart.endExclusive)
        assertEquals(original.start, clampedEnd.start)
        assertEquals(original.start.plusSeconds(1), clampedEnd.endExclusive)
    }

    @Test
    fun signalsSubtractFractionalGrabOffsetsAndSnapWithoutAnInterval() {
        val original = TimelineIndexedSignal("signal:s", "s", "Signal", null, origin)
        val moved = editTimelineSignal(
            original,
            origin.plusSeconds(90).plusNanos(250_000_000),
            30.25,
            TimelineDivision.Minute,
            utc
        )
        assertEquals(origin.plusSeconds(60), moved.instant)
        assertEquals(original.key, moved.key)
        assertTrue(moved.changed)
        assertFalse(editTimelineSignal(original, origin.plusSeconds(30), 30.0, TimelineDivision.Second, utc).changed)
    }

    @Test
    fun timedMovesKeepElapsedDurationAcrossTheSpringGapAndAutumnOverlap() {
        for (date in listOf(LocalDate.parse("2026-03-29"), LocalDate.parse("2026-10-25"))) {
            val original = interval(start = date.minusDays(1).atStartOfDay(berlin).toInstant())
            val target = date.atStartOfDay(berlin).plusHours(2).toInstant()
            val moved =
                editTimelineInterval(original, TimelineEditMode.Move, target, 0.0, TimelineDivision.Hour, berlin)
            assertEquals(target, moved.start)
            assertEquals(Duration.ofMinutes(30), Duration.between(moved.start, moved.endExclusive))
        }
    }

    @Test
    fun markerMovesPreserveLocalDayCountRatherThanElapsedHoursAcrossDst() {
        for ((date, hours) in listOf("2026-03-29" to 71L, "2026-10-25" to 73L)) {
            val transition = LocalDate.parse(date)
            val original = dateInterval(transition.minusDays(1), transition.plusDays(2))
            assertEquals(hours, Duration.between(original.start, original.endExclusive).toHours())
            val targetDate = transition.plusDays(7)
            val moved = editTimelineInterval(
                original, TimelineEditMode.Move, targetDate.atTime(5, 0).atZone(berlin).toInstant(),
                3600.0, TimelineDivision.Minute, berlin,
            )
            assertEquals(targetDate.atStartOfDay(berlin).toInstant(), moved.start)
            assertEquals(targetDate.plusDays(3).atStartOfDay(berlin).toInstant(), moved.endExclusive)
            assertEquals(72L, Duration.between(moved.start, moved.endExclusive).toHours())
            val returned = editTimelineInterval(
                original.copy(start = moved.start, endExclusive = moved.endExclusive), TimelineEditMode.Move,
                original.start, 0.0, TimelineDivision.Day, berlin,
            )
            assertEquals(original.start, returned.start)
            assertEquals(original.endExclusive, returned.endExclusive)
        }
    }

    @Test
    fun markerResizesSnapToDayBoundariesAndClampToOneLocalDay() {
        val date = LocalDate.parse("2026-03-29")
        val original = dateInterval(date.minusDays(1), date.plusDays(1))
        val start = editTimelineInterval(
            original, TimelineEditMode.Start, date.plusDays(4).atStartOfDay(berlin).toInstant(),
            0.0, TimelineDivision.Minute, berlin,
        )
        assertEquals(date.atStartOfDay(berlin).toInstant(), start.start)
        assertEquals(original.endExclusive, start.endExclusive)
        assertEquals(23L, Duration.between(start.start, start.endExclusive).toHours())
        val end = editTimelineInterval(
            original, TimelineEditMode.End, original.start.minusSeconds(86400), 0.0, TimelineDivision.Hour, berlin,
        )
        assertEquals(original.start, end.start)
        assertEquals(date.atStartOfDay(berlin).toInstant(), end.endExclusive)
        val extended = editTimelineInterval(
            original, TimelineEditMode.End, date.plusDays(2).atTime(18, 0).atZone(berlin).toInstant(),
            0.0, TimelineDivision.Hour, berlin,
        )
        assertEquals(date.plusDays(3).atStartOfDay(berlin).toInstant(), extended.endExclusive)
    }

    @Test
    fun allDayActionsOnlyMoveTheirDateAndKeepTheirStoredDuration() {
        val action = Action(
            "a",
            "r",
            title = "All day",
            queued = true,
            start = "2026-03-28",
            duration = "PT45M",
            recurrence = recurrence
        )
        val original = index(berlin, actions = listOf(action)).intervals.single()
        val date = LocalDate.parse("2026-03-29")
        val moved = editTimelineInterval(
            original,
            TimelineEditMode.Move,
            date.atStartOfDay(berlin).toInstant(),
            0.0,
            TimelineDivision.Minute,
            berlin,
        )
        assertEquals(23L, Duration.between(moved.start, moved.endExclusive).toHours())
        assertEquals(action.copy(start = date.toString()), timelineEditedAction(action, moved, berlin))
        for (mode in listOf(TimelineEditMode.Start, TimelineEditMode.End)) {
            assertThrows(IllegalArgumentException::class.java) {
                editTimelineInterval(original, mode, original.start, 0.0, TimelineDivision.Day, berlin)
            }
        }
    }

    @Test
    fun constructorsRejectInvalidGeometryAndHelpersRejectNonfiniteOffsets() {
        val original = interval()
        assertThrows(IllegalArgumentException::class.java) {
            TimelineEdit.Interval(original, original.start, original.start, TimelineEditMode.End)
        }
        assertThrows(IllegalArgumentException::class.java) {
            TimelineEdit.Interval(original, original.start, original.start.plusNanos(999_999_999), TimelineEditMode.End)
        }
        assertThrows(IllegalArgumentException::class.java) {
            TimelineEdit.Interval(original, original.start.plusSeconds(1), original.endExclusive, TimelineEditMode.Move)
        }
        assertThrows(IllegalArgumentException::class.java) {
            TimelineEdit.Interval(original, original.start.plusSeconds(1), original.endExclusive, TimelineEditMode.End)
        }
        assertThrows(IllegalArgumentException::class.java) {
            editTimelineInterval(original, TimelineEditMode.Move, origin, Double.NaN, TimelineDivision.Second, utc)
        }
        assertThrows(IllegalArgumentException::class.java) {
            editTimelineSignal(
                TimelineIndexedSignal("signal:s", "s", "", null, origin),
                origin,
                Double.POSITIVE_INFINITY,
                TimelineDivision.Second,
                utc
            )
        }
    }

    @Test
    fun movingAnUnspecifiedDurationActionMergesMetadataWithoutMaterializingItsFallback() {
        val action = Action(
            "a",
            "r",
            title = "Old",
            queued = true,
            pinned = true,
            start = origin.toString(),
            recurrence = recurrence
        )
        val original = index(actions = listOf(action)).intervals.single()
        val edit = editTimelineInterval(
            original,
            TimelineEditMode.Move,
            origin.plusSeconds(3600),
            0.0,
            TimelineDivision.Minute,
            utc
        )
        val latest = action.copy(title = "New", content = "Notes", routineId = "routine", sourceProvider = "provider")
        assertEquals(latest.copy(start = edit.start.toString()), timelineEditedAction(latest, edit, utc))
        for (stale in listOf(
            latest.copy(start = origin.plusSeconds(1).toString()),
            latest.copy(duration = "PT5M"),
            latest.copy(queued = false),
            latest.copy(completion = origin.toString())
        )) {
            assertThrows(WorkspaceEditConflictException::class.java) { timelineEditedAction(stale, edit, utc) }
        }
        val resized = editTimelineInterval(
            original,
            TimelineEditMode.End,
            origin.plusSeconds(600),
            0.0,
            TimelineDivision.Minute,
            utc
        )
        assertEquals(latest.copy(duration = "PT10M"), timelineEditedAction(latest, resized, utc))
    }

    @Test
    fun eventsRetainMetadataAndMoveDurationEncodingWhileResizeChangesOnlyTiming() {
        val event = Event(
            "e",
            "l",
            title = "Old",
            start = "2026-10-15T11:00:00+02:00",
            duration = "PT1800S",
            recurrence = recurrence
        )
        val original = index(events = listOf(event)).intervals.single()
        val latest =
            event.copy(title = "New", content = "Notes", templateId = "t", sourceBusy = false, busyOverride = true)
        val move = editTimelineInterval(
            original,
            TimelineEditMode.Move,
            origin.plusSeconds(3600),
            0.0,
            TimelineDivision.Minute,
            utc
        )
        assertEquals(latest.copy(start = move.start.toString()), timelineEditedEvent(latest, move, utc))
        val end = editTimelineInterval(
            original,
            TimelineEditMode.End,
            origin.plusSeconds(3600),
            0.0,
            TimelineDivision.Minute,
            utc
        )
        assertEquals(latest.copy(duration = "PT1H"), timelineEditedEvent(latest, end, utc))
        val start = editTimelineInterval(
            original,
            TimelineEditMode.Start,
            origin.plusSeconds(600),
            0.0,
            TimelineDivision.Minute,
            utc
        )
        assertEquals(
            latest.copy(start = start.start.toString(), duration = "PT20M"),
            timelineEditedEvent(latest, start, utc)
        )
        assertThrows(WorkspaceEditConflictException::class.java) {
            timelineEditedEvent(
                latest.copy(duration = "PT31M"),
                move,
                utc
            )
        }
    }

    @Test
    fun markersSerializeInclusiveEndsAndKeepRecurrenceWhenMovingOrResizing() {
        val marker =
            Marker("m", "l", title = "Old", date = "2026-03-28", endDate = "2026-03-30", recurrence = recurrence)
        val original = index(berlin, markers = listOf(marker)).intervals.single()
        val latest = marker.copy(title = "New", content = "Notes", templateId = "t", sourceProvider = "provider")
        val move = editTimelineInterval(
            original,
            TimelineEditMode.Move,
            LocalDate.parse("2026-04-02").atStartOfDay(berlin).toInstant(),
            0.0,
            TimelineDivision.Day,
            berlin
        )
        assertEquals(
            latest.copy(date = "2026-04-02", endDate = "2026-04-04"),
            timelineEditedMarker(latest, move, berlin)
        )
        val end =
            editTimelineInterval(original, TimelineEditMode.End, original.start, 0.0, TimelineDivision.Day, berlin)
        assertEquals(latest.copy(endDate = "2026-03-28"), timelineEditedMarker(latest, end, berlin))
        assertThrows(WorkspaceEditConflictException::class.java) {
            timelineEditedMarker(
                latest.copy(endDate = "2026-03-31"),
                move,
                berlin
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            timelineEditedMarker(
                latest,
                move.copy(start = move.start.plusSeconds(1)),
                berlin
            )
        }
        val single = marker.copy(endDate = null)
        val singleOriginal = index(berlin, markers = listOf(single)).intervals.single()
        val extendedStart = editTimelineInterval(
            singleOriginal,
            TimelineEditMode.Start,
            singleOriginal.start.minusSeconds(86400),
            0.0,
            TimelineDivision.Day,
            berlin
        )
        assertEquals(
            single.copy(date = "2026-03-27", endDate = "2026-03-28"),
            timelineEditedMarker(single, extendedStart, berlin)
        )
    }

    @Test
    fun signalPersistenceAcceptsMetadataRefreshButRefusesStaleTiming() {
        val signal = Signal("s", "l", title = "Old", datetime = origin.toString(), recurrence = recurrence)
        val original = index(signals = listOf(signal)).matching(setOf("signal:s")).signals.single()
        val edit = editTimelineSignal(original, origin.plusSeconds(60), 0.0, TimelineDivision.Minute, utc)
        val latest = signal.copy(title = "New", content = "Notes", templateId = "t")
        assertEquals(latest.copy(datetime = edit.instant.toString()), timelineEditedSignal(latest, edit))
        assertThrows(WorkspaceEditConflictException::class.java) {
            timelineEditedSignal(
                latest.copy(datetime = edit.instant.toString()),
                edit
            )
        }
    }

    private fun interval(start: Instant = origin) = TimelineIndexedInterval(
        "event:e", TimelineItemKind.Event, "e", "Event", null, start, start.plusSeconds(1800), false,
    )

    private fun dateInterval(start: LocalDate, end: LocalDate) = TimelineIndexedInterval(
        "marker:m", TimelineItemKind.Marker, "m", "Marker", null,
        start.atStartOfDay(berlin).toInstant(), end.atStartOfDay(berlin).toInstant(), true,
    )

    private fun index(
        zoneId: ZoneId = utc,
        actions: List<Action> = emptyList(),
        events: List<Event> = emptyList(),
        markers: List<Marker> = emptyList(),
        signals: List<Signal> = emptyList(),
    ) = TimelineDataIndex.build(zoneId, actions, events, markers, signals)
}

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [28])
@SQLiteMode(SQLiteMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
class TimelineEditPersistenceTest {
    private val event = Event("e", "l", title = "Event", start = "2026-10-15T09:00:00Z", duration = "PT30M")
    private val marker = Marker("m", "l", title = "Marker", date = "2026-10-15")
    private val signal = Signal("s", "l", title = "Signal", datetime = "2026-10-15T09:00:00Z")

    @Test
    fun guardedTimingWritesSurviveReopeningAndRejectAConcurrentMetadataWrite() = runBlocking {
        withTimeout(10_000) {
            val latestEvent = event.copy(title = "Refreshed event")
            val latestMarker = marker.copy(title = "Refreshed marker")
            val latestSignal = signal.copy(title = "Refreshed signal")
            val editedEvent = latestEvent.copy(start = "2026-10-16T09:00:00Z")
            val editedMarker = latestMarker.copy(date = "2026-10-16")
            val editedSignal = latestSignal.copy(datetime = "2026-10-16T09:00:00Z")
            repository().use { repo ->
                repo.updateEvent(null, latestEvent)
                repo.updateMarker(null, latestMarker)
                repo.updateSignal(null, latestSignal)
                assertTrue(runCatching {
                    repo.updateEvent(
                        null,
                        editedEvent,
                        expectedEvent = event
                    )
                }.exceptionOrNull() is WorkspaceEditConflictException)
                assertTrue(runCatching {
                    repo.updateMarker(
                        null,
                        editedMarker,
                        expectedMarker = marker
                    )
                }.exceptionOrNull() is WorkspaceEditConflictException)
                assertTrue(runCatching {
                    repo.updateSignal(
                        null,
                        editedSignal,
                        expectedSignal = signal
                    )
                }.exceptionOrNull() is WorkspaceEditConflictException)
                val unchanged = repo.loadLocal()
                assertEquals(listOf(latestEvent), unchanged.events)
                assertEquals(listOf(latestMarker), unchanged.markers)
                assertEquals(listOf(latestSignal), unchanged.signals)
                repo.updateEvent(null, editedEvent, attemptSync = false, expectedEvent = latestEvent)
                repo.updateMarker(null, editedMarker, attemptSync = false, expectedMarker = latestMarker)
                repo.updateSignal(null, editedSignal, attemptSync = false, expectedSignal = latestSignal)
            }
            repository().use { repo ->
                val restored = repo.loadLocal()
                assertEquals(listOf(editedEvent), restored.events)
                assertEquals(listOf(editedMarker), restored.markers)
                assertEquals(listOf(editedSignal), restored.signals)
            }
        }
    }

    @Test
    fun expectedObjectsCannotResurrectDeletedItemsOrAuthorizeADifferentId() = runBlocking {
        withTimeout(10_000) {
            repository().use { repo ->
                repo.updateEvent(null, event)
                repo.updateMarker(null, marker)
                repo.updateSignal(null, signal)
                assertTrue(runCatching {
                    repo.updateEvent(
                        null,
                        event.copy(id = "other"),
                        expectedEvent = event
                    )
                }.exceptionOrNull() is WorkspaceEditConflictException)
                assertTrue(runCatching {
                    repo.updateMarker(
                        null,
                        marker.copy(id = "other"),
                        expectedMarker = marker
                    )
                }.exceptionOrNull() is WorkspaceEditConflictException)
                assertTrue(runCatching {
                    repo.updateSignal(
                        null,
                        signal.copy(id = "other"),
                        expectedSignal = signal
                    )
                }.exceptionOrNull() is WorkspaceEditConflictException)
                repo.deleteEvent(null, event.id)
                repo.deleteMarker(null, marker.id)
                repo.deleteSignal(null, signal.id)
                assertTrue(runCatching {
                    repo.updateEvent(
                        null,
                        event,
                        expectedEvent = event
                    )
                }.exceptionOrNull() is WorkspaceEditConflictException)
                assertTrue(runCatching {
                    repo.updateMarker(
                        null,
                        marker,
                        expectedMarker = marker
                    )
                }.exceptionOrNull() is WorkspaceEditConflictException)
                assertTrue(runCatching {
                    repo.updateSignal(
                        null,
                        signal,
                        expectedSignal = signal
                    )
                }.exceptionOrNull() is WorkspaceEditConflictException)
                val empty = repo.loadLocal()
                assertTrue(empty.events.isEmpty() && empty.markers.isEmpty() && empty.signals.isEmpty())
            }
        }
    }

    @Test
    fun viewModelCompletesCallbacksAfterInstallationAndOnStaleOrCancelledSaves() = runBlocking {
        val application = RuntimeEnvironment.getApplication()
        val viewModel = MainViewModel(application)
        val repo = AppServices.repository(application)
        try {
            await { viewModel.uiState.value.content != null }
            repo.updateEvent(null, event)
            viewModel.loadActions()
            await { viewModel.event(event.id) == event }
            val original = TimelineDataIndex.build(
                ZoneOffset.UTC,
                emptyList(),
                listOf(event),
                emptyList(),
                emptyList()
            ).intervals.single()
            val edit = editTimelineInterval(
                original,
                TimelineEditMode.Move,
                original.start.plusSeconds(3600),
                0.0,
                TimelineDivision.Minute,
                ZoneOffset.UTC
            )
            val latest = event.copy(title = "Refreshed while dragging")
            repo.updateEvent(null, latest)
            val finished = mutableListOf<Boolean>()
            viewModel.applyTimelineEdit(edit) { finished += it }
            assertTrue(viewModel.saving.value)
            assertTrue(finished.isEmpty())
            await { finished.isNotEmpty() }
            assertEquals(listOf(true), finished)
            assertFalse(viewModel.saving.value)
            assertEquals(latest.copy(start = edit.start.toString()), viewModel.event(event.id))
            assertEquals(viewModel.event(event.id), repo.loadLocal().events.single())

            viewModel.applyTimelineEdit(edit) { finished += it }
            await { finished.size == 2 }
            assertEquals(listOf(true, false), finished)
            assertFalse(viewModel.saving.value)
            assertNotNull(viewModel.uiState.value.operationError)

            viewModel.viewModelScope.cancel()
            viewModel.applyTimelineEdit(edit) { finished += it }
            assertEquals(listOf(true, false, false), finished)
            assertFalse(viewModel.saving.value)
        } finally {
            viewModel.viewModelScope.cancel()
            repo.close()
        }
    }

    private suspend fun await(predicate: () -> Boolean) = withTimeout(10_000) {
        while (!predicate()) {
            shadowOf(Looper.getMainLooper()).idle()
            delay(10)
        }
    }

    private fun repository(): SubroutineRepository {
        val api = Proxy.newProxyInstance(
            SubroutineApi::class.java.classLoader,
            arrayOf(SubroutineApi::class.java)
        ) { _, method, _ ->
            error("Unexpected network call: ${method.name}")
        } as SubroutineApi
        return SubroutineRepository(api, RuntimeEnvironment.getApplication(), "https://timeline.example.test")
    }
}
