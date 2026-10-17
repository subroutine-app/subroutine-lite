package com.example.subroutine_simple.ui.creator

import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.listSaver
import java.time.Duration
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset

val ItemCreatorStateSaver: Saver<ItemCreatorState, Any> = listSaver<ItemCreatorState, Any?>(
    save = { state ->
        buildList {
            addAll(
                listOf(
                    "creator-v2",
                    state.recurrence?.toString(),
                    state.schedule.preferredOffset?.id,
                    state.selectedType.name,
                    state.title,
                    state.notes,
                    state.schedule.date?.toString(),
                    state.schedule.time?.toString(),
                    state.schedule.zoneId.id,
                    state.duration?.toString(),
                    state.actionQueued,
                    state.actionPinned,
                    state.markerEndDate?.toString(),
                )
            )
            state.routineSteps.forEach { step ->
                add(step.id)
                add(step.title)
                add(step.duration?.toString())
            }
        }
    },
    restore = { saved ->
        runCatching {
            val versioned = saved.firstOrNull() == "creator-v2"
            val values = if (versioned) saved.drop(3) else saved
            val recurrence = if (versioned) (saved[1] as String?)?.let(parserJson::parseToJsonElement) else null
            val offset = if (versioned) (saved[2] as String?)?.let(ZoneOffset::of) else null
            require(values.size >= 10 && (values.size - 10) % 3 == 0)
            ItemCreatorState(
                selectedType = CreatorItemType.valueOf(values[0] as String),
                title = values[1] as String,
                notes = values[2] as String,
                schedule = CreatorSchedule(
                    date = (values[3] as String?)?.let(LocalDate::parse),
                    time = (values[4] as String?)?.let(LocalTime::parse),
                    zoneId = ZoneId.of(values[5] as String),
                    preferredOffset = offset,
                ),
                duration = (values[6] as String?)?.let(Duration::parse),
                recurrence = recurrence,
                actionQueued = values[7] as Boolean,
                actionPinned = values[8] as Boolean,
                markerEndDate = (values[9] as String?)?.let(LocalDate::parse),
                routineSteps = values.drop(10).chunked(3).map { step ->
                    RoutineStepDraft(
                        id = step[0] as String,
                        title = step[1] as String,
                        duration = (step[2] as String?)?.let(Duration::parse),
                    )
                },
            )
        }.getOrNull()
    },
)
