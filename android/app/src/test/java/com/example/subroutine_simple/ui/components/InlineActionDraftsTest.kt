package com.example.subroutine_simple.ui.components

import android.os.Bundle
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [28])
class InlineActionDraftsTest {
    private val home = "home:workspace-a:dataset-a"
    private val today = "calendar:workspace-a:dataset-a:2026-10-15"
    private val tomorrow = "calendar:workspace-a:dataset-a:2026-10-16"
    private val otherWorkspace = "calendar:workspace-b:dataset-b:2026-10-15"

    @Test
    fun completionUpdatesTheLiveDraftAcrossScreenRemovalAndOwnerRecreation() {
        val savedState = SavedStateHandle()
        val retainedStore = ViewModelStore()
        val factory = viewModelFactory { initializer { InlineActionDraftsViewModel(savedState) } }
        val drafts = ViewModelProvider(retainedStore, factory)[InlineActionDraftsViewModel::class.java]
        val draft = drafts.draft(home)
        draft.expand()
        draft.edit("Send invoice")
        val submittedId = draft.creationId
        var complete: () -> Unit = {}
        draft.submit { _, onSuccess -> complete = onSuccess }

        drafts.draft(today).expand()
        complete()

        val recreated = ViewModelProvider(retainedStore, factory)[InlineActionDraftsViewModel::class.java]
        val remounted = recreated.draft(home)
        assertSame(drafts, recreated)
        assertSame(draft, remounted)
        assertEquals("", remounted.title)
        assertFalse(remounted.expanded)
        assertFalse(remounted.attempted)
        assertNotEquals(submittedId, remounted.creationId)
        val restored = InlineActionDraftsViewModel(restore(savedState)).draft(home)
        assertEquals("", restored.title)
        assertNotEquals(submittedId, restored.creationId)
        assertFalse("inline-draft:$home" in savedState.keys())
        assertFalse(restored.expanded)
        retainedStore.clear()
    }

    @Test
    fun savedStateRetainsDraftsByWorkspaceAndDate() {
        val savedState = SavedStateHandle()
        val drafts = InlineActionDraftsViewModel(savedState)
        val first = drafts.draft(today)
        first.expand()
        first.edit("First day")
        val creationId = first.creationId
        drafts.draft(tomorrow).apply { expand(); edit("Second day") }
        drafts.draft(otherWorkspace).apply { expand(); edit("Other account") }
        first.submit { _, _ -> }

        assertSame(first, drafts.draft(today))
        val restored = InlineActionDraftsViewModel(restore(savedState))
        val restoredFirst = restored.draft(today)
        assertEquals("First day", restoredFirst.title)
        assertEquals(creationId, restoredFirst.creationId)
        assertTrue(restoredFirst.expanded)
        assertTrue(restoredFirst.attempted)
        assertFalse(restoredFirst.focusRequested)
        assertEquals("Second day", restored.draft(tomorrow).title)
        assertEquals("Other account", restored.draft(otherWorkspace).title)
    }

    @Test
    fun retriesReuseTheCreationIdUntilSuccessOrCancellation() {
        val draft = InlineActionDraftsViewModel(SavedStateHandle()).draft(home)
        draft.expand()
        draft.edit("Retry me")
        val creationId = draft.creationId
        val submittedIds = mutableListOf<String>()
        var complete: () -> Unit = {}
        repeat(2) {
            draft.submit { title, onSuccess ->
                assertEquals("Retry me", title)
                submittedIds += draft.creationId
                complete = onSuccess
            }
        }
        assertEquals(listOf(creationId, creationId), submittedIds)
        complete()
        val nextId = draft.creationId
        assertNotEquals(creationId, nextId)
        draft.expand()
        draft.edit("Another item")
        complete()
        assertEquals("Another item", draft.title)
        assertEquals(nextId, draft.creationId)
        draft.cancel()
        assertNotEquals(nextId, draft.creationId)
        assertEquals("", draft.title)
    }

    @Test
    fun lateSuccessCannotClearEditsOrAReplacementDraft() {
        val savedState = SavedStateHandle()
        val draft = InlineActionDraftsViewModel(savedState).draft(today)
        draft.expand()
        draft.edit("Submitted")
        val submittedId = draft.creationId
        var complete: () -> Unit = {}
        draft.submit { _, onSuccess -> complete = onSuccess }
        draft.edit("Newer edits")
        complete()
        assertEquals("Newer edits", draft.title)
        assertTrue(draft.expanded)
        assertNotEquals(submittedId, draft.creationId)
        assertEquals("Newer edits", InlineActionDraftsViewModel(restore(savedState)).draft(today).title)

        draft.submit { _, onSuccess -> complete = onSuccess }
        draft.cancel()
        draft.expand()
        draft.edit("Replacement")
        val replacementId = draft.creationId
        complete()
        assertEquals("Replacement", draft.title)
        assertEquals(replacementId, draft.creationId)
    }

    @Test
    fun fullCreatorCompletionClearsOnlyTheHandedOffDraft() {
        val drafts = InlineActionDraftsViewModel(SavedStateHandle())
        val draft = drafts.draft(today)
        draft.expand()
        draft.edit("Continue editing")
        val handedOff = draft.creationId
        draft.complete("unrelated")
        assertEquals("Continue editing", draft.title)
        draft.complete(handedOff)
        assertFalse(draft.expanded)
        assertEquals("", draft.title)
        draft.expand()
        draft.edit("Next item")
        draft.complete(handedOff)
        assertEquals("Next item", draft.title)
    }

    private fun restore(state: SavedStateHandle) = SavedStateHandle(
        state.keys().associateWith { key -> Bundle(requireNotNull(state.get<Bundle>(key))) },
    )
}
