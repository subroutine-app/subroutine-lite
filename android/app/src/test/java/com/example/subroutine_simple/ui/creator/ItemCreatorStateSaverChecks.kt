package com.example.subroutine_simple.ui.creator

import androidx.compose.runtime.saveable.SaverScope
import java.time.Duration
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId

object ItemCreatorStateSaverChecks {
    @JvmStatic
    fun main(args: Array<String>) {
        fullDraftRoundTripsForEveryType()
        blankAndPartialDraftsRoundTrip()
        malformedPayloadIsNotRestored()
    }

    private val scope = object : SaverScope {
        override fun canBeSaved(value: Any): Boolean = value is String || value is Boolean
    }

    private fun fullDraftRoundTripsForEveryType() {
        val draft = ItemCreatorState(
            title = "  Café 🌱\n ",
            notes = "\nFirst paragraph\n\nSecond paragraph\t ",
            schedule = CreatorSchedule(
                date = LocalDate.of(2026, 3, 8),
                time = LocalTime.of(2, 30, 45, 123_456_789),
                zoneId = ZoneId.of("America/Los_Angeles"),
            ),
            duration = Duration.ofSeconds(9_000_000_000_000L, 987_654_321),
            actionQueued = true,
            actionPinned = true,
            markerEndDate = LocalDate.of(2026, 3, 10),
            routineSteps = listOf(
                RoutineStepDraft("step-b", "  Warm up\n", Duration.ofSeconds(1, 1)),
                RoutineStepDraft("step-a", "", null),
                RoutineStepDraft("step-zero", "\t", Duration.ZERO),
                RoutineStepDraft("step-negative", "Cool down", Duration.ofNanos(-1)),
            ),
        )
        CreatorItemType.entries.forEach { type ->
            checkRoundTrip(draft.copy(selectedType = type))
        }
    }

    private fun blankAndPartialDraftsRoundTrip() {
        val blank = ItemCreatorState.initial(ZoneId.of("UTC"))
        checkRoundTrip(blank)
        checkRoundTrip(
            blank.copy(
                title = " \t",
                notes = "\n ",
                schedule = CreatorSchedule(date = LocalDate.MIN, zoneId = ZoneId.of("+05:45")),
                duration = Duration.ZERO,
                markerEndDate = LocalDate.MAX,
                routineSteps = listOf(RoutineStepDraft("unfinished", "", null)),
            )
        )
        checkRoundTrip(
            blank.copy(
                schedule = CreatorSchedule(time = LocalTime.MAX, zoneId = ZoneId.of("UTC+05:45")),
                actionPinned = true,
            )
        )
        checkRoundTrip(
            blank.copy(
                schedule = blank.schedule.copy(time = LocalTime.MIDNIGHT),
                actionQueued = true,
            )
        )
    }

    private fun malformedPayloadIsNotRestored() {
        check(ItemCreatorStateSaver.restore(emptyList<Any?>()) == null)
        val saved = save(ItemCreatorState.initial(ZoneId.of("UTC")))
        check(ItemCreatorStateSaver.restore(saved.dropLast(1)) == null)
        check(ItemCreatorStateSaver.restore(saved + listOf("incomplete-step")) == null)
        check(ItemCreatorStateSaver.restore(saved.map { if (it == "UTC") "invalid/zone" else it }) == null)
    }

    private fun checkRoundTrip(state: ItemCreatorState) {
        val restored = ItemCreatorStateSaver.restore(ArrayList(save(state)))
        check(restored == state) { "Creator draft did not round-trip" }
    }

    private fun save(state: ItemCreatorState): List<*> {
        val saved = requireNotNull(with(ItemCreatorStateSaver) { scope.save(state) })
        check(saved is List<*>)
        check(saved.all { it == null || scope.canBeSaved(it) }) {
            "Creator saver must emit only primitive values and nulls"
        }
        return saved
    }
}
