package com.example.subroutine_simple.ui.components

import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.Composable
import androidx.compose.runtime.saveable.SaverScope
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.ui.agenda.AgendaEntry
import java.time.LocalDate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CompoundCardStateTest {
    @Test
    fun noteExpansionSurvivesRemovalAndRestorationWithoutDependingOnTextLengthOrPosition() {
        val short = entry("short", "One line")
        val long = entry("long", "A paragraph\n".repeat(30))
        val notes = ItemCardNotesState()
        assertFalse(notes.isExpanded(short.key))
        assertFalse(notes.isExpanded(long.key))
        notes.toggle(short.key)
        notes.toggle(long.key)
        notes.toggle(long.key)

        val scope = object : SaverScope {
            override fun canBeSaved(value: Any): Boolean = value is String || value is List<*>
        }
        val saved = with(ItemCardNotesState.Saver) { scope.save(notes) }
        val restored = requireNotNull(ItemCardNotesState.Saver.restore(requireNotNull(saved)))
        val reordered = listOf(long, short.copy(action = short.action.copy(content = "Edited notes")))
        assertEquals(listOf(false, true), reordered.map { restored.isExpanded(it.key) })
        assertTrue(restored.isExpanded(short.key))
    }

    @Test
    fun largeGroupsRegisterIndividualRowsWithoutComposingThemAndKeepKeysAcrossMoves() {
        val entries = List(20_000) { entry(it.toString()) }
        val first = RecordedRows()
        first.compoundItemCards(entries, "day:one", itemKey = { "agenda:${it.key}" })
        assertEquals(entries.size, first.count)
        assertEquals(entries.size, (0 until first.count).map(first.key).toSet().size)

        val moved = RecordedRows()
        moved.compoundItemCards(
            entries.take(3).reversed(), "day:two", dateContext = LocalDate.of(2026, 10, 16),
            itemKey = { "agenda:${it.key}" },
        )
        assertEquals((0..2).map(first.key).reversed(), (0..2).map(moved.key))

        val empty = RecordedRows()
        empty.compoundItemCards(emptyList(), "empty")
        assertEquals(0, empty.count)
    }

    private fun entry(id: String, notes: String? = null) = AgendaEntry.ActionEntry(
        Action(id = id, recurrenceId = id, title = id, content = notes),
    )

    private class RecordedRows : LazyListScope {
        var count = 0
        var key: (Int) -> Any = { error("No rows registered") }

        override fun items(
            count: Int,
            key: ((Int) -> Any)?,
            contentType: (Int) -> Any?,
            itemContent: @Composable LazyItemScope.(Int) -> Unit,
        ) {
            this.count = count
            this.key = requireNotNull(key)
        }
    }
}
