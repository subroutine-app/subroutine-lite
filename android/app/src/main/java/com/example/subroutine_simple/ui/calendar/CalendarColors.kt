package com.example.subroutine_simple.ui.calendar

import androidx.compose.material3.ColorScheme
import androidx.compose.ui.graphics.Color

internal data class CalendarDayColors(
    val container: Color,
    val content: Color,
    val numberContainer: Color,
    val numberContent: Color,
    val numberBorder: Color,
)

internal fun calendarDayColors(
    day: CalendarDay,
    scheme: ColorScheme,
    currentTime: Color,
    onCurrentTime: Color,
): CalendarDayColors {
    val content = when {
        day.isSelected -> scheme.onPrimaryContainer
        day.isInVisibleMonth -> scheme.onSurface
        else -> scheme.onSurfaceVariant
    }
    return CalendarDayColors(
        container = if (day.isSelected) scheme.primaryContainer else Color.Transparent,
        content = content,
        numberContainer = if (day.isToday) currentTime else Color.Transparent,
        numberContent = if (day.isToday) onCurrentTime else content,
        numberBorder = if (day.isToday && day.isSelected) scheme.onSurface else Color.Transparent,
    )
}

internal fun CalendarDay.summaryDescription(): String = buildList {
    if (summary.workCount > 0) add("${summary.workCount} work")
    if (summary.markerCount > 0) add("${summary.markerCount} markers")
    if (summary.signalCount > 0) add("${summary.signalCount} signals")
}.joinToString()
