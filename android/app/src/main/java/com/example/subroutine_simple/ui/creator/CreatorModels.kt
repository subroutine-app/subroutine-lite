package com.example.subroutine_simple.ui.creator

import com.example.subroutine_simple.data.models.Action as ActionModel
import com.example.subroutine_simple.data.models.Event as EventModel
import com.example.subroutine_simple.data.models.Marker as MarkerModel
import com.example.subroutine_simple.data.models.Routine as RoutineModel
import com.example.subroutine_simple.data.models.RoutineStep
import com.example.subroutine_simple.data.models.Signal as SignalModel
import java.time.Clock
import java.time.Duration
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset
import kotlinx.serialization.json.JsonElement
import java.time.ZonedDateTime
import java.util.UUID

private const val WHOLE_ITEM_DURATION_STEP_MINUTES = 15L
private const val ROUTINE_STEP_DURATION_MINUTES = 5L
private const val DEFAULT_EVENT_DURATION_MINUTES = 60L

enum class CreatorItemType(val label: String) {
    Action("Action"),
    Event("Event"),
    Marker("Marker"),
    Signal("Signal"),
    Routine("Routine"),
}

data class CreatorSchedule(
    val date: LocalDate? = null,
    val time: LocalTime? = null,
    val zoneId: ZoneId = ZoneId.systemDefault(),
    val preferredOffset: ZoneOffset? = null,
) {
    val hasDate: Boolean get() = date != null
    val hasTime: Boolean get() = date != null && time != null

    fun toModelValue(): String? = when {
        date == null -> null
        time == null -> date.toString()
        else -> requireNotNull(toZonedDateTime()).toInstant().toString()
    }

    fun toZonedDateTime(): ZonedDateTime? {
        val selectedDate = date ?: return null
        val selectedTime = time ?: return null
        return ZonedDateTime.ofLocal(selectedDate.atTime(selectedTime), zoneId, preferredOffset)
    }
}

data class RoutineStepDraft(
    val id: String = UUID.randomUUID().toString(),
    val title: String,
    val duration: Duration? = null,
) {
    fun toModel(): RoutineStep = RoutineStep(
        title = title.trim(),
        duration = duration?.toString(),
    )
}

data class CreatorValidationIssue(
    val field: CreatorField,
    val message: String,
)

enum class CreatorField {
    Title,
    Date,
    Time,
    Duration,
    Queue,
    MarkerEndDate,
    RoutineSteps,
}

data class CreatorValidation(val issues: List<CreatorValidationIssue>) {
    val isValid: Boolean get() = issues.isEmpty()
    val firstMessage: String? get() = issues.firstOrNull()?.message

    fun messageFor(field: CreatorField): String? = issues.firstOrNull { it.field == field }?.message
}

data class ItemCreatorState(
    val selectedType: CreatorItemType = CreatorItemType.Action,
    val title: String = "",
    val notes: String = "",
    val schedule: CreatorSchedule = CreatorSchedule(),
    val duration: Duration? = null,
    val actionQueued: Boolean = false,
    val actionPinned: Boolean = false,
    val markerEndDate: LocalDate? = null,
    val routineSteps: List<RoutineStepDraft> = emptyList(),
    val recurrence: JsonElement? = null,
) {
    companion object {
        fun initial(zoneId: ZoneId = ZoneId.systemDefault()): ItemCreatorState =
            ItemCreatorState(schedule = CreatorSchedule(zoneId = zoneId))
    }

    val canPinAction: Boolean
        get() = actionQueued && schedule.hasTime

    fun selectType(
        type: CreatorItemType,
        clock: Clock = Clock.systemDefaultZone(),
    ): ItemCreatorState {
        val today = LocalDate.now(clock.withZone(schedule.zoneId))
        val nextQuarter = nextQuarterHour(clock, schedule.zoneId)
        return when (type) {
            CreatorItemType.Action,
            CreatorItemType.Routine,
                -> copy(selectedType = type)

            CreatorItemType.Event -> copy(
                selectedType = type,
                schedule = schedule.copy(
                    date = schedule.date ?: today,
                    time = schedule.time ?: nextQuarter,
                ),
                duration = duration ?: Duration.ofMinutes(DEFAULT_EVENT_DURATION_MINUTES),
            )

            CreatorItemType.Signal -> copy(
                selectedType = type,
                schedule = schedule.copy(
                    date = schedule.date ?: today,
                    time = schedule.time ?: nextQuarter,
                ),
            )

            CreatorItemType.Marker -> {
                val start = schedule.date ?: today
                copy(
                    selectedType = type,
                    schedule = schedule.copy(date = start, time = null),
                    actionPinned = false,
                    markerEndDate = markerEndDate?.coerceAtLeast(start) ?: start,
                )
            }
        }
    }

    fun withScheduleDate(date: LocalDate?): ItemCreatorState {
        val nextSchedule = schedule.copy(
            date = date,
            time = schedule.time.takeIf { date != null },
            preferredOffset = null,
        )
        val nextEndDate = if (selectedType == CreatorItemType.Marker && date != null) {
            markerEndDate?.coerceAtLeast(date) ?: date
        } else {
            markerEndDate
        }
        return copy(
            schedule = nextSchedule,
            markerEndDate = nextEndDate,
            actionPinned = actionPinned && actionQueued && nextSchedule.hasTime,
        )
    }

    fun withScheduleTime(time: LocalTime?): ItemCreatorState = copy(
        schedule = schedule.copy(time = time.takeIf { schedule.date != null }, preferredOffset = null),
        actionPinned = actionPinned && actionQueued && schedule.date != null && time != null,
    )

    fun withActionQueued(queued: Boolean): ItemCreatorState = copy(
        actionQueued = queued,
        actionPinned = actionPinned && queued && schedule.hasTime,
    )

    fun withActionPinned(pinned: Boolean): ItemCreatorState = copy(
        actionPinned = pinned && canPinAction,
    )

    fun stepDuration(steps: Int): ItemCreatorState {
        val nextMinutes = (duration?.toMinutes() ?: 0L) + steps * WHOLE_ITEM_DURATION_STEP_MINUTES
        return copy(duration = nextMinutes.takeIf { it > 0 }?.let(Duration::ofMinutes))
    }

    fun addRoutineStep(title: String): ItemCreatorState {
        if (title.isBlank()) return this
        return copy(routineSteps = routineSteps + RoutineStepDraft(title = title.trim()))
    }

    fun updateRoutineStepTitle(id: String, title: String): ItemCreatorState = copy(
        routineSteps = routineSteps.map { step ->
            if (step.id == id) step.copy(title = title) else step
        },
    )

    fun removeRoutineStep(id: String): ItemCreatorState =
        copy(routineSteps = routineSteps.filterNot { it.id == id })

    fun moveRoutineStep(id: String, offset: Int): ItemCreatorState {
        val from = routineSteps.indexOfFirst { it.id == id }
        if (from < 0) return this
        val to = (from + offset).coerceIn(routineSteps.indices)
        if (from == to) return this
        val reordered = routineSteps.toMutableList()
        val step = reordered.removeAt(from)
        reordered.add(to, step)
        return copy(routineSteps = reordered)
    }

    fun stepRoutineDuration(id: String, steps: Int): ItemCreatorState = copy(
        routineSteps = routineSteps.map { step ->
            if (step.id != id) return@map step
            val nextMinutes = (step.duration?.toMinutes() ?: 0L) + steps * ROUTINE_STEP_DURATION_MINUTES
            step.copy(duration = nextMinutes.takeIf { it > 0 }?.let(Duration::ofMinutes))
        },
    )

    fun validate(): CreatorValidation {
        val issues = buildList {
            if (title.isBlank()) {
                add(CreatorValidationIssue(CreatorField.Title, "Enter a title."))
            }
            if (schedule.date == null && schedule.time != null) {
                add(CreatorValidationIssue(CreatorField.Date, "Choose a date for this time."))
            }
            if (schedule.date != null && schedule.time != null) {
                val offsets = schedule.zoneId.rules.getValidOffsets(schedule.date.atTime(schedule.time))
                if (offsets.isEmpty()) {
                    add(
                        CreatorValidationIssue(
                            CreatorField.Time,
                            "This time does not exist due to daylight saving. Choose another time."
                        )
                    )
                } else if (offsets.size > 1 && (schedule.preferredOffset == null || schedule.preferredOffset !in offsets)) {
                    add(
                        CreatorValidationIssue(
                            CreatorField.Time,
                            "This time occurs twice. Specify a UTC offset in the title."
                        )
                    )
                } else if (schedule.preferredOffset != null && schedule.preferredOffset !in offsets) {
                    add(
                        CreatorValidationIssue(
                            CreatorField.Time,
                            "The time’s UTC offset no longer matches this timezone."
                        )
                    )
                }
            }
            if (duration != null && (duration.isZero || duration.isNegative)) {
                add(CreatorValidationIssue(CreatorField.Duration, "Duration must be greater than zero."))
            }

            when (selectedType) {
                CreatorItemType.Action -> {
                    if (actionPinned && !actionQueued) {
                        add(CreatorValidationIssue(CreatorField.Queue, "A pinned action must be queued."))
                    }
                    if (actionPinned && !schedule.hasTime) {
                        add(CreatorValidationIssue(CreatorField.Time, "Choose a date and time before pinning."))
                    }
                }

                CreatorItemType.Event -> {
                    if (!schedule.hasDate) {
                        add(CreatorValidationIssue(CreatorField.Date, "Choose an event date."))
                    }
                    if (!schedule.hasTime) {
                        add(CreatorValidationIssue(CreatorField.Time, "Choose an event time."))
                    }
                    if (duration == null) {
                        add(CreatorValidationIssue(CreatorField.Duration, "Choose an event duration."))
                    }
                }

                CreatorItemType.Marker -> {
                    val start = schedule.date
                    val end = markerEndDate
                    if (start == null) {
                        add(CreatorValidationIssue(CreatorField.Date, "Choose a marker start date."))
                    }
                    if (end == null) {
                        add(CreatorValidationIssue(CreatorField.MarkerEndDate, "Choose an inclusive end date."))
                    } else if (start != null && end.isBefore(start)) {
                        add(CreatorValidationIssue(CreatorField.MarkerEndDate, "End date cannot be before start date."))
                    }
                }

                CreatorItemType.Signal -> {
                    if (!schedule.hasDate) {
                        add(CreatorValidationIssue(CreatorField.Date, "Choose a signal date."))
                    }
                    if (!schedule.hasTime) {
                        add(CreatorValidationIssue(CreatorField.Time, "Choose when the signal should fire."))
                    }
                }

                CreatorItemType.Routine -> {
                    if (routineSteps.isEmpty()) {
                        add(CreatorValidationIssue(CreatorField.RoutineSteps, "Add at least one routine step."))
                    }
                    if (routineSteps.any { it.title.isBlank() }) {
                        add(CreatorValidationIssue(CreatorField.RoutineSteps, "Every routine step needs a title."))
                    }
                    if (routineSteps.any { it.duration?.let { value -> value.isZero || value.isNegative } == true }) {
                        add(
                            CreatorValidationIssue(
                                CreatorField.RoutineSteps,
                                "Step durations must be greater than zero."
                            )
                        )
                    }
                }
            }
        }
        return CreatorValidation(issues)
    }

    fun buildRequest(): CreateItemBuildResult {
        val validation = validate()
        if (!validation.isValid) return CreateItemBuildResult.Invalid(validation)

        val cleanTitle = title.trim()
        val cleanNotes = notes.trim().ifEmpty { null }
        val request = when (selectedType) {
            CreatorItemType.Action -> CreateItemRequest.Action(
                title = cleanTitle,
                notes = cleanNotes,
                start = schedule.takeIf { it.hasDate },
                duration = duration,
                recurrence = recurrence,
                queued = actionQueued,
                pinned = actionPinned,
            )

            CreatorItemType.Event -> CreateItemRequest.Event(
                title = cleanTitle,
                notes = cleanNotes,
                start = requireNotNull(schedule.toZonedDateTime()),
                duration = requireNotNull(duration),
                recurrence = recurrence,
            )

            CreatorItemType.Marker -> CreateItemRequest.Marker(
                title = cleanTitle,
                notes = cleanNotes,
                startDate = requireNotNull(schedule.date),
                inclusiveEndDate = requireNotNull(markerEndDate),
                recurrence = recurrence,
            )

            CreatorItemType.Signal -> CreateItemRequest.Signal(
                title = cleanTitle,
                notes = cleanNotes,
                at = requireNotNull(schedule.toZonedDateTime()),
                recurrence = recurrence,
            )

            CreatorItemType.Routine -> CreateItemRequest.Routine(
                title = cleanTitle,
                notes = cleanNotes,
                target = schedule.takeIf { it.hasDate },
                steps = routineSteps.map(RoutineStepDraft::toModel),
                recurrence = recurrence,
            )
        }
        return CreateItemBuildResult.Success(request)
    }
}

sealed interface CreateItemRequest {
    val title: String
    val notes: String?
    val recurrence: JsonElement?

    data class Action(
        override val title: String,
        override val notes: String?,
        val start: CreatorSchedule?,
        val duration: Duration?,
        val queued: Boolean,
        val pinned: Boolean,
        override val recurrence: JsonElement? = null,
    ) : CreateItemRequest {
        init {
            require(!pinned || queued) { "A pinned action must be queued." }
            require(!pinned || start?.hasTime == true) { "A pinned action needs an explicit time." }
        }
    }

    data class Event(
        override val title: String,
        override val notes: String?,
        val start: ZonedDateTime,
        val duration: Duration,
        override val recurrence: JsonElement? = null,
    ) : CreateItemRequest

    data class Marker(
        override val title: String,
        override val notes: String?,
        val startDate: LocalDate,
        val inclusiveEndDate: LocalDate,
        override val recurrence: JsonElement? = null,
    ) : CreateItemRequest {
        init {
            require(!inclusiveEndDate.isBefore(startDate)) { "Marker end date cannot precede its start date." }
        }
    }

    data class Signal(
        override val title: String,
        override val notes: String?,
        val at: ZonedDateTime,
        override val recurrence: JsonElement? = null,
    ) : CreateItemRequest

    data class Routine(
        override val title: String,
        override val notes: String?,
        val target: CreatorSchedule?,
        val steps: List<RoutineStep>,
        override val recurrence: JsonElement? = null,
    ) : CreateItemRequest
}

sealed interface CreateItemBuildResult {
    data class Success(val request: CreateItemRequest) : CreateItemBuildResult
    data class Invalid(val validation: CreatorValidation) : CreateItemBuildResult
}

data class CreatorModelIdentity(
    val id: String,
    val lineageId: String = id,
)

sealed interface CreatorModel {
    data class Action(val value: ActionModel) : CreatorModel
    data class Event(val value: EventModel) : CreatorModel
    data class Marker(val value: MarkerModel) : CreatorModel
    data class Signal(val value: SignalModel) : CreatorModel
    data class Routine(val value: RoutineModel) : CreatorModel
}

fun CreateItemRequest.toModel(identity: CreatorModelIdentity): CreatorModel = when (this) {
    is CreateItemRequest.Action -> CreatorModel.Action(
        ActionModel(
            id = identity.id,
            recurrenceId = identity.lineageId,
            title = title,
            content = notes,
            queued = queued,
            pinned = pinned,
            start = start?.toModelValue(),
            duration = duration?.toString(),
            recurrence = recurrence,
        ),
    )

    is CreateItemRequest.Event -> CreatorModel.Event(
        EventModel(
            id = identity.id,
            lineageId = identity.lineageId,
            title = title,
            content = notes,
            start = start.toInstant().toString(),
            duration = duration.toString(),
            recurrence = recurrence,
        ),
    )

    is CreateItemRequest.Marker -> CreatorModel.Marker(
        MarkerModel(
            id = identity.id,
            lineageId = identity.lineageId,
            title = title,
            content = notes,
            date = startDate.toString(),
            endDate = inclusiveEndDate.takeIf { it != startDate }?.toString(),
            recurrence = recurrence,
        ),
    )

    is CreateItemRequest.Signal -> CreatorModel.Signal(
        SignalModel(
            id = identity.id,
            lineageId = identity.lineageId,
            title = title,
            content = notes,
            datetime = at.toInstant().toString(),
            recurrence = recurrence,
        ),
    )

    is CreateItemRequest.Routine -> CreatorModel.Routine(
        RoutineModel(
            id = identity.id,
            recurrenceId = identity.lineageId,
            title = title,
            content = notes,
            target = target?.toModelValue(),
            steps = steps,
            recurrence = recurrence,
        ),
    )
}

internal fun nextQuarterHour(clock: Clock, zoneId: ZoneId): LocalTime {
    val now = ZonedDateTime.now(clock.withZone(zoneId))
    val nextMinutes = ((now.minute / 15) + 1) * 15
    val nextHour = (now.hour + nextMinutes / 60) % 24
    return LocalTime.of(nextHour, nextMinutes % 60)
}
