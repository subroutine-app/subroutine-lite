package com.example.subroutine_simple.data.models

import java.time.Instant
import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.ZoneId
import kotlin.math.absoluteValue


fun schedulePointHasTime(point: String): Boolean = point.contains('T')

fun parseSchedulePoint(point: String?): Instant? {
    if (point.isNullOrBlank()) return null
    return runCatching { Instant.parse(point) }
        .recoverCatching { OffsetDateTime.parse(point).toInstant() }
        .recoverCatching { LocalDate.parse(point).atStartOfDay(ZoneId.systemDefault()).toInstant() }
        .getOrNull()
}

private const val SECONDS_PER_MINUTE = 60L
private const val SECONDS_PER_HOUR = 60L * SECONDS_PER_MINUTE
private const val SECONDS_PER_DAY = 24L * SECONDS_PER_HOUR

private const val DAYS_PER_MONTH = 30L
private const val DAYS_PER_YEAR = 365L

private val ISO_DURATION = Regex(
    "^(-)?P(?:(-?\\d+)Y)?(?:(-?\\d+)M)?(?:(-?\\d+)W)?(?:(-?\\d+)D)?" +
        "(?:T(?:(-?\\d+)H)?(?:(-?\\d+)M)?(?:(-?\\d+(?:[.,]\\d+)?)S)?)?$",
)

fun parseIsoDurationSeconds(duration: String?): Long? {
    if (duration.isNullOrBlank()) return null
    val match = ISO_DURATION.matchEntire(duration.trim().uppercase()) ?: return null
    val groups = match.groupValues

    fun part(index: Int): Long = groups[index].takeIf { it.isNotEmpty() }?.toLongOrNull() ?: 0L
    val fractionalSeconds = groups[8]
        .takeIf { it.isNotEmpty() }
        ?.replace(',', '.')
        ?.toDoubleOrNull()
        ?: 0.0

    val total = part(2) * DAYS_PER_YEAR * SECONDS_PER_DAY +
        part(3) * DAYS_PER_MONTH * SECONDS_PER_DAY +
        part(4) * 7L * SECONDS_PER_DAY +
        part(5) * SECONDS_PER_DAY +
        part(6) * SECONDS_PER_HOUR +
        part(7) * SECONDS_PER_MINUTE +
        fractionalSeconds.toLong()

    return if (groups[1] == "-") -total else total
}

fun formatIsoDuration(totalSeconds: Long): String {
    if (totalSeconds == 0L) return "PT0S"
    val sign = if (totalSeconds < 0) "-" else ""
    var remaining = totalSeconds.absoluteValue

    val days = remaining / SECONDS_PER_DAY
    remaining %= SECONDS_PER_DAY
    val hours = remaining / SECONDS_PER_HOUR
    remaining %= SECONDS_PER_HOUR
    val minutes = remaining / SECONDS_PER_MINUTE
    val seconds = remaining % SECONDS_PER_MINUTE

    return buildString {
        append('P')
        if (days > 0) append("$sign${days}D")
        if (hours > 0 || minutes > 0 || seconds > 0) {
            append('T')
            if (hours > 0) append("$sign${hours}H")
            if (minutes > 0) append("$sign${minutes}M")
            if (seconds > 0) append("$sign${seconds}S")
        }
    }
}
