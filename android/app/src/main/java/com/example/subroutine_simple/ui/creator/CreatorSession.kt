package com.example.subroutine_simple.ui.creator

import com.example.subroutine_simple.data.models.UuidV7
import java.time.LocalDate
import java.time.ZoneId

data class InlineCreatorSource(val key: String, val creationId: String)

data class CreatorSession(
    val initial: ItemCreatorState,
    val source: InlineCreatorSource? = null,
    val id: String = UuidV7.newString(),
) {
    val creationId: String get() = source?.creationId ?: id
    val expandDetails: Boolean get() = source != null

    companion object {
        fun fromInline(source: InlineCreatorSource, title: String, date: LocalDate, zone: ZoneId) = CreatorSession(
            initial = ItemCreatorState.initial(zone).copy(
                title = title,
                schedule = CreatorSchedule(date = date, zoneId = zone),
                actionQueued = true,
            ),
            source = source,
        )
    }
}
