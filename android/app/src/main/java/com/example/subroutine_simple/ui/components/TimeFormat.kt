package com.example.subroutine_simple.ui.components

import com.example.subroutine_simple.data.models.parseSchedulePoint
import com.example.subroutine_simple.data.models.schedulePointHasTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter

private val dateTimeFormatter = DateTimeFormatter.ofPattern("MMM d, h:mm a")
private val dateFormatter = DateTimeFormatter.ofPattern("MMM d")

internal fun formatSchedulePoint(point: String): String {
    val instant = parseSchedulePoint(point) ?: return point
    val local = instant.atZone(ZoneId.systemDefault())
    val formatter = if (schedulePointHasTime(point)) dateTimeFormatter else dateFormatter
    return local.format(formatter)
}

internal fun formatDuration(totalMins: Long): String = when {
    totalMins >= 60 -> {
        val hours = totalMins / 60
        val mins = totalMins % 60
        if (mins == 0L) "${hours}h" else "${hours}h ${mins}m"
    }
    else -> "${totalMins}m"
}
