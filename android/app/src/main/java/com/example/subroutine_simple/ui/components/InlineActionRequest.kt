package com.example.subroutine_simple.ui.components

import com.example.subroutine_simple.ui.creator.CreateItemRequest
import com.example.subroutine_simple.ui.creator.CreatorSchedule
import java.time.LocalDate

fun inlineActionRequest(title: String, date: LocalDate): CreateItemRequest.Action? =
    title.trim().takeIf { it.isNotEmpty() }?.let { cleanTitle ->
        CreateItemRequest.Action(
            title = cleanTitle,
            notes = null,
            start = CreatorSchedule(date = date),
            duration = null,
            queued = true,
            pinned = false,
        )
    }
