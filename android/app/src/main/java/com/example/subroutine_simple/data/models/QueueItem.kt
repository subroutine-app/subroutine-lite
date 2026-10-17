package com.example.subroutine_simple.data.models

import java.time.Instant

sealed class QueueItem {
    abstract val id: String
    abstract val title: String
    abstract val content: String?

    abstract val sortInstant: Instant

    data class ActionItem(val action: Action) : QueueItem() {
        override val id = action.id
        override val title = action.title
        override val content = action.content
        override val sortInstant = parseSchedulePoint(action.scheduledStart) ?: Instant.MAX
    }

    data class EventItem(val event: Event) : QueueItem() {
        override val id = event.id
        override val title = event.title
        override val content = event.content
        override val sortInstant = event.startInstant ?: Instant.MAX
    }
}
