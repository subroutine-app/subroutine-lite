package com.example.subroutine_simple.ui.components

import android.text.format.DateFormat
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CalendarMonth
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerColors
import androidx.compose.material3.DatePickerDefaults
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.TimePickerColors
import androidx.compose.material3.TimePickerDefaults
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.data.models.parseIsoDurationSeconds
import com.example.subroutine_simple.ui.theme.SubroutineTheme
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

private val EditorDateFormatter = DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)
private val EditorTimeFormatter = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)

data class EditorSchedule(
    val date: LocalDate? = null,
    val time: LocalTime? = null,
    val zoneId: ZoneId = ZoneId.systemDefault(),
) {
    fun toSchedulePoint(): String? = when {
        date == null -> null
        time == null -> date.toString()
        else -> ZonedDateTime.of(date, time, zoneId).toInstant().toString()
    }

    companion object {
        fun fromSchedulePoint(value: String?, zoneId: ZoneId = ZoneId.systemDefault()): EditorSchedule {
            if (value.isNullOrBlank()) return EditorSchedule(zoneId = zoneId)
            if ('T' !in value) {
                return EditorSchedule(
                    date = runCatching { LocalDate.parse(value) }.getOrNull(),
                    zoneId = zoneId,
                )
            }
            val instant = runCatching { Instant.parse(value) }
                .recoverCatching { OffsetDateTime.parse(value).toInstant() }
                .getOrNull()
                ?: return EditorSchedule(zoneId = zoneId)
            val local = instant.atZone(zoneId)
            return EditorSchedule(local.toLocalDate(), local.toLocalTime().withSecond(0).withNano(0), zoneId)
        }
    }
}

fun durationMinutes(value: String?): Long? =
    parseIsoDurationSeconds(value)?.takeIf { it > 0 }?.div(60)

fun durationIso(minutes: Long?): String? = minutes?.takeIf { it > 0 }?.let { "PT${it}M" }

@Composable
fun ScheduleEditor(
    schedule: EditorSchedule,
    durationMinutes: Long?,
    onScheduleChange: (EditorSchedule) -> Unit,
    onDurationChange: (Long?) -> Unit,
    requireDateAndTime: Boolean,
    requireDuration: Boolean,
    showDuration: Boolean = true,
    showHeading: Boolean = true,
    showDateTime: Boolean = true,
    enabled: Boolean,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (showHeading) {
            Text(
                text = "Schedule",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
        if (showDateTime) {
            DateEditorField(
                date = schedule.date,
                required = requireDateAndTime,
                enabled = enabled,
                onDateChange = { date ->
                    onScheduleChange(
                        schedule.copy(
                            date = date,
                            time = schedule.time.takeIf { date != null },
                        ),
                    )
                },
            )
            TimeControl(
                time = schedule.time,
                required = requireDateAndTime,
                enabled = enabled && schedule.date != null,
                onTimeChange = { onScheduleChange(schedule.copy(time = it)) },
            )
        }
        if (showDuration) {
            DurationControl(
                minutes = durationMinutes,
                required = requireDuration,
                enabled = enabled,
                onChange = onDurationChange,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DateEditorField(
    label: String = "Date",
    date: LocalDate?,
    required: Boolean,
    enabled: Boolean,
    onDateChange: (LocalDate?) -> Unit,
) {
    var showPicker by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedButton(
                onClick = { showPicker = true },
                enabled = enabled,
                modifier = Modifier.weight(1f),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.onSurface),
            ) {
                Icon(
                    Icons.Default.CalendarMonth,
                    contentDescription = null,
                    tint = if (enabled) MaterialTheme.colorScheme.onSurfaceVariant else LocalContentColor.current,
                )
                Text(
                    text = date?.format(EditorDateFormatter) ?: if (required) "Choose date" else "No date",
                    modifier = Modifier.padding(start = 8.dp),
                )
            }
            if (!required && date != null) {
                IconButton(onClick = { onDateChange(null) }, enabled = enabled) {
                    Icon(Icons.Default.Close, contentDescription = "Clear $label")
                }
            }
        }
    }

    if (showPicker) {
        val selectedMillis = date?.atStartOfDay(ZoneOffset.UTC)?.toInstant()?.toEpochMilli()
        val pickerState = rememberDatePickerState(initialSelectedDateMillis = selectedMillis)
        DatePickerDialog(
            onDismissRequest = { showPicker = false },
            confirmButton = {
                TextButton(
                    onClick = {
                        pickerState.selectedDateMillis?.let { millis ->
                            onDateChange(Instant.ofEpochMilli(millis).atZone(ZoneOffset.UTC).toLocalDate())
                        }
                        showPicker = false
                    },
                    enabled = pickerState.selectedDateMillis != null,
                ) { Text("Use date") }
            },
            dismissButton = {
                TextButton(onClick = { showPicker = false }) { Text("Cancel") }
            },
        ) { DatePicker(state = pickerState, colors = editorDatePickerColors()) }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun editorDatePickerColors(): DatePickerColors {
    val colors = SubroutineTheme.colors
    return DatePickerDefaults.colors(
        todayContentColor = colors.currentTimeText,
        todayDateBorderColor = colors.currentTime,
        selectedDayContainerColor = MaterialTheme.colorScheme.primary,
        selectedDayContentColor = MaterialTheme.colorScheme.onPrimary,
        selectedYearContainerColor = MaterialTheme.colorScheme.primary,
        selectedYearContentColor = MaterialTheme.colorScheme.onPrimary,
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun editorTimePickerColors(): TimePickerColors {
    val scheme = MaterialTheme.colorScheme
    return TimePickerDefaults.colors(
        periodSelectorSelectedContainerColor = scheme.primaryContainer,
        periodSelectorSelectedContentColor = scheme.onPrimaryContainer,
        timeSelectorSelectedContainerColor = scheme.primaryContainer,
        timeSelectorSelectedContentColor = scheme.onPrimaryContainer,
        selectorColor = scheme.primary,
        clockDialSelectedContentColor = scheme.onPrimary,
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TimeControl(
    time: LocalTime?,
    required: Boolean,
    enabled: Boolean,
    onTimeChange: (LocalTime?) -> Unit,
) {
    var showPicker by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OutlinedButton(
            onClick = { showPicker = true },
            enabled = enabled,
            modifier = Modifier.weight(1f),
            colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.onSurface),
        ) {
            Icon(
                Icons.Default.Schedule,
                contentDescription = null,
                tint = if (enabled) MaterialTheme.colorScheme.onSurfaceVariant else LocalContentColor.current,
            )
            Text(
                text = time?.format(EditorTimeFormatter) ?: if (required) "Choose time" else "No time",
                modifier = Modifier.padding(start = 8.dp),
            )
        }
        if (!required && time != null) {
            IconButton(onClick = { onTimeChange(null) }, enabled = enabled) {
                Icon(Icons.Default.Close, contentDescription = "Clear time")
            }
        }
    }

    if (showPicker) {
        val context = LocalContext.current
        val initial = time ?: LocalTime.NOON
        val pickerState = rememberTimePickerState(
            initialHour = initial.hour,
            initialMinute = initial.minute,
            is24Hour = DateFormat.is24HourFormat(context),
        )
        AlertDialog(
            onDismissRequest = { showPicker = false },
            title = { Text("Choose time") },
            text = { TimePicker(state = pickerState, colors = editorTimePickerColors()) },
            confirmButton = {
                TextButton(onClick = {
                    onTimeChange(LocalTime.of(pickerState.hour, pickerState.minute))
                    showPicker = false
                }) { Text("Use time") }
            },
            dismissButton = {
                TextButton(onClick = { showPicker = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun DurationControl(
    minutes: Long?,
    required: Boolean,
    enabled: Boolean,
    onChange: (Long?) -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text("Duration", style = MaterialTheme.typography.labelLarge)
            Text(
                text = minutes?.let(::formatDuration) ?: if (required) "Choose a duration" else "No fixed duration",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        IconButton(
            onClick = {
                val next = (minutes ?: 0L) - 15L
                onChange(next.takeIf { it > 0 })
            },
            enabled = enabled && minutes != null && (!required || minutes > 15),
        ) {
            Icon(Icons.Default.Remove, contentDescription = "Decrease duration by 15 minutes")
        }
        IconButton(
            onClick = { onChange((minutes ?: 0L) + 15L) },
            enabled = enabled,
        ) {
            Icon(Icons.Default.Add, contentDescription = "Increase duration by 15 minutes")
        }
    }
}
