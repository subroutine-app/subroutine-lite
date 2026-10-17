package com.example.subroutine_simple.ui.timeline

import android.os.Bundle
import android.os.Parcel
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.example.subroutine_simple.ui.creator.CreateItemBuildResult
import com.example.subroutine_simple.ui.creator.CreateItemRequest
import com.example.subroutine_simple.ui.creator.CreatorItemType
import com.example.subroutine_simple.ui.creator.ItemParseResponse
import com.example.subroutine_simple.ui.creator.ParserHighlight
import com.example.subroutine_simple.ui.creator.ParserHighlightKind
import com.example.subroutine_simple.ui.creator.ParserOutcome
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.ZoneOffset
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [28])
class TimelineCreatorStateTest {
    private val zone = ZoneId.of("America/New_York")
    private val anchor = Instant.parse("2026-11-01T06:30:00Z")
    private val now = Instant.parse("2026-10-15T23:30:00Z")
    private val scope = "workspace-a:dataset-a"

    @Test
    fun parserContextKeepsClickedDateSeparateFromClockAndAnchor() {
        val state = TimelineCreatorViewModel(SavedStateHandle()).state(scope)
        assertTrue(state.begin(anchor, zoneId = zone, now = now))
        val initial = requireNotNull(state.draft)
        assertEquals(now.toString(), initial.request.now)
        assertEquals("2026-11-01", initial.request.defaultDate)
        assertEquals(zone.id, initial.request.timezone)
        assertEquals(ZoneOffset.ofHours(-5), initial.fields.schedule.preferredOffset)

        state.edit(TextFieldValue("Work tomorrow for 30m"))
        val pending = requireNotNull(state.draft)
        state.accept(
            requireNotNull(state.creationId), pending.request, pending.revision, ParserOutcome.Ready(
                ItemParseResponse(
                    version = 1, title = "Work", whenValue = "2026-10-16", durationSeconds = 1_800,
                    warnings = emptyList(),
                    highlights = listOf(
                        ParserHighlight(5, 13, ParserHighlightKind.When),
                        ParserHighlight(14, 21, ParserHighlightKind.Duration),
                    ),
                ),
            )
        )
        val parsed = requireNotNull(state.draft)
        assertEquals(anchor, state.anchor)
        assertEquals(initial.request.defaultDate, parsed.request.defaultDate)
        assertEquals(initial.now, parsed.now)
        val request = (parsed.buildRequest() as CreateItemBuildResult.Success).request as CreateItemRequest.Action
        assertEquals("2026-10-16T05:30:00Z", request.start?.toModelValue())
        assertEquals(Duration.ofMinutes(30), request.duration)
        assertTrue(request.queued && request.pinned)

        verifyTitle(state, "Work")
        val restored = requireNotNull(state.draft)
        assertEquals(initial.fields.schedule, restored.fields.schedule)
        assertNull(restored.fields.duration)
        assertEquals(anchor.toString(), restored.fields.schedule.toModelValue())
    }

    @Test
    fun externallySelectedTypesUseTheSnapAndNeverReplaceAnOpenDraft() {
        val state = TimelineCreatorViewModel(SavedStateHandle()).state(scope)
        listOf(
            CreatorItemType.Action,
            CreatorItemType.Event,
            CreatorItemType.Marker,
            CreatorItemType.Signal
        ).forEach { type ->
            assertTrue(state.begin(anchor, type, zone, now))
            verifyTitle(state, "Entry")
            val id = state.creationId
            assertFalse(state.begin(anchor.plusSeconds(3_600), CreatorItemType.Event, zone, now))
            assertEquals(id, state.creationId)
            assertEquals(anchor, state.anchor)
            when (val request = (requireNotNull(state.draft).buildRequest() as CreateItemBuildResult.Success).request) {
                is CreateItemRequest.Action -> assertEquals(anchor.toString(), request.start?.toModelValue())
                is CreateItemRequest.Event -> {
                    assertEquals(anchor, request.start.toInstant())
                    assertEquals(Duration.ofHours(1), request.duration)
                }

                is CreateItemRequest.Marker -> {
                    assertEquals(LocalDate.parse("2026-11-01"), request.startDate)
                    assertEquals(request.startDate, request.inclusiveEndDate)
                    assertNull(state.draft?.fields?.schedule?.time)
                }

                is CreateItemRequest.Signal -> assertEquals(anchor, request.at.toInstant())
                is CreateItemRequest.Routine -> fail("Routines are not timeline items")
            }
            state.cancel()
        }
    }

    @Test
    fun parcelRestorationPreservesScopeSelectionAndIdentityButRequiresParserVerification() {
        val saved = SavedStateHandle()
        val drafts = TimelineCreatorViewModel(saved)
        val first = drafts.state(scope)
        first.begin(anchor, zoneId = zone, now = now)
        verifyTitle(first, "First")
        first.submit { _, _, _ -> }
        first.edit(TextFieldValue("First", TextRange(1, 4), TextRange(0, 5)))
        val second = drafts.state("workspace-b:dataset-b")
        second.begin(anchor.plusSeconds(3_600), CreatorItemType.Signal, zone, now)
        verifyTitle(second, "Second")
        assertSame(first, drafts.state(scope))

        val restored = TimelineCreatorViewModel(roundTrip(saved))
        val restoredFirst = restored.state(scope)
        assertEquals(first.creationId, restoredFirst.creationId)
        assertEquals(anchor, restoredFirst.anchor)
        assertEquals(now, restoredFirst.draft?.now)
        assertEquals("First", restoredFirst.draft?.input?.text)
        assertEquals(TextRange(1, 4), restoredFirst.draft?.input?.selection)
        assertNull(restoredFirst.draft?.input?.composition)
        assertEquals(first.draft?.fields, restoredFirst.draft?.fields)
        assertTrue(requireNotNull(restoredFirst.draft).isPending)
        assertTrue(restoredFirst.attempted)
        restoredFirst.submit { _, _, _ -> fail("Restored parser results must be verified") }
        assertEquals("Second", restored.state("workspace-b:dataset-b").draft?.input?.text)
        assertNull(restored.state("workspace-c:dataset-c").draft)
        verifyTitle(restoredFirst, "First")
        restoredFirst.submit { _, _, complete -> complete() }
        assertNull(restoredFirst.draft)
        assertNull(restoredFirst.anchor)
        assertNull(restoredFirst.creationId)
    }

    @Test
    fun completionSurvivesOwnerRecreationAndRetriesKeepTheSameIdentity() {
        val saved = SavedStateHandle()
        val store = ViewModelStore()
        val factory = viewModelFactory { initializer { TimelineCreatorViewModel(saved) } }
        val drafts = ViewModelProvider(store, factory)[TimelineCreatorViewModel::class.java]
        val state = drafts.state(scope)
        state.begin(anchor, zoneId = zone, now = now)
        verifyTitle(state, "Retry")
        val id = state.creationId
        var complete: () -> Unit = {}
        repeat(2) {
            state.submit { _, submittedId, onSuccess ->
                assertEquals(id, submittedId)
                complete = onSuccess
            }
            assertEquals("Retry", state.draft?.input?.text)
        }
        val remounted = ViewModelProvider(store, factory)[TimelineCreatorViewModel::class.java].state(scope)
        assertSame(state, remounted)
        complete()
        assertNull(remounted.draft)
        assertTrue(saved.keys().isEmpty())
        store.clear()
    }

    @Test
    fun staleParsesAndCompletionsCannotEraseNewerEditsOrAReplacement() {
        val state = TimelineCreatorViewModel(SavedStateHandle()).state(scope)
        state.begin(anchor, zoneId = zone, now = now)
        verifyTitle(state, "Work")
        val submitted = requireNotNull(state.draft)
        val submittedId = requireNotNull(state.creationId)
        var complete: () -> Unit = {}
        state.submit { _, _, onSuccess -> complete = onSuccess }
        state.edit(TextFieldValue("Newer"))
        state.edit(TextFieldValue("Work"))
        state.accept(submittedId, submitted.request, submitted.revision, titleOutcome("Work"))
        assertTrue(requireNotNull(state.draft).isPending)
        complete()
        assertEquals("Work", state.draft?.input?.text)
        assertEquals(anchor, state.anchor)
        assertNotEquals(submittedId, state.creationId)
        verifyTitle(state, "Work")
        state.submit { _, _, onSuccess -> complete = onSuccess }
        state.cancel()
        state.begin(anchor, zoneId = zone, now = now)
        val replacement = state.creationId
        state.edit(TextFieldValue("Work"))
        state.accept(submittedId, submitted.request, submitted.revision, titleOutcome("Work"))
        complete()
        assertEquals(replacement, state.creationId)
        assertTrue(requireNotNull(state.draft).isPending)
        val pending = requireNotNull(state.draft)
        state.accept(
            requireNotNull(replacement),
            pending.request,
            pending.revision,
            ParserOutcome.Failed("Parser unavailable")
        )
        state.submit { _, _, _ -> fail("Parser failure must not become a plain title") }
        assertEquals("Work", state.draft?.input?.text)
        assertEquals("Parser unavailable", state.draft?.error)
    }

    private fun verifyTitle(state: TimelineCreatorState, title: String) {
        state.edit(TextFieldValue(title))
        val draft = requireNotNull(state.draft)
        state.accept(requireNotNull(state.creationId), draft.request, draft.revision, titleOutcome(title))
    }

    private fun titleOutcome(title: String) = ParserOutcome.Ready(
        ItemParseResponse(1, title, warnings = emptyList(), highlights = emptyList()),
    )

    private fun roundTrip(saved: SavedStateHandle): SavedStateHandle {
        val parcel = Parcel.obtain()
        return try {
            parcel.writeBundle(Bundle().apply {
                saved.keys().forEach { putBundle(it, saved[it]) }
            })
            parcel.setDataPosition(0)
            val restored = requireNotNull(parcel.readBundle(javaClass.classLoader))
            SavedStateHandle(restored.keySet().associateWith { restored.getBundle(it) })
        } finally {
            parcel.recycle()
        }
    }
}
