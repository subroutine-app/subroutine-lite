package com.example.subroutine_simple.ui.creator

import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZoneOffset
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.longOrNull

internal val parserJson = Json {
    ignoreUnknownKeys = true
    encodeDefaults = true
}

@Serializable
enum class ParserItemKind { Action, Event, Marker, Signal, RoutineStep, ActionTemplate, EventTemplate }

@Serializable
data class ItemParseRequest(
    val text: String,
    val kind: ParserItemKind,
    val now: String,
    val timezone: String,
    val defaultDate: String? = null,
    val version: Int = 1,
) {
    fun validate() {
        require(version == 1) { "Unsupported parser request version: $version" }
        require(OffsetDateTime.parse(now).offset == ZoneOffset.UTC) { "Parser clock must be UTC." }
        require(timezone in ZoneId.getAvailableZoneIds()) { "Choose an IANA timezone for parsing." }
        defaultDate?.let(LocalDate::parse)
    }
}

@Serializable
enum class ParserHighlightKind { Title, When, Duration, Recurrence, Tag, Location, People, Priority, Sigil }

@Serializable
data class ParserHighlight(val start: Int, val end: Int, val kind: ParserHighlightKind) {
    fun isValidFor(text: String): Boolean = start >= 0 && start < end && end <= text.length &&
            text.isUtf16Boundary(start) && text.isUtf16Boundary(end)
}

internal fun String.isUtf16Boundary(offset: Int): Boolean = offset in 0..length &&
        (offset == 0 || offset == length || !(this[offset - 1].isHighSurrogate() && this[offset].isLowSurrogate()))

@Serializable
data class ItemParseResponse(
    val version: Int,
    val title: String,
    val whenValue: String? = null,
    val naiveTime: String? = null,
    val durationSeconds: Long? = null,
    val recurrence: JsonElement? = null,
    val warnings: List<String>,
    val highlights: List<ParserHighlight>,
    val error: String? = null,
) {
    fun validate(request: ItemParseRequest): ItemParseResponse {
        require(version == 1) { "Unsupported parser response version: $version. Update the app and native parser together." }
        require(highlights.all { it.isValidFor(request.text) }) { "Parser returned invalid UTF-16 highlight offsets." }
        whenValue?.let { value ->
            if ('T' in value) {
                require(OffsetDateTime.parse(value).offset == ZoneOffset.UTC) { "Parser timestamp must be UTC." }
                Instant.parse(value)
            } else {
                LocalDate.parse(value)
            }
        }
        naiveTime?.let(LocalTime::parse)
        require(durationSeconds == null || durationSeconds > 0) { "Parser duration must be positive." }
        validateRecurrence(recurrence)

        if (request.text.isBlank() && error == null) {
            require(
                title.isBlank() && whenValue == null && naiveTime == null && durationSeconds == null &&
                        (recurrence == null || recurrence == JsonNull) && highlights.isEmpty()
            ) {
                "Parser returned content for an empty input."
            }
        }
        return copy(recurrence = recurrence.takeUnless { it == JsonNull })
    }
}

internal fun validateRecurrence(value: JsonElement?) {
    if (value == null || value == JsonNull) return
    require(value is JsonObject) { "Parser recurrence must be a core Recurrence object." }
    val rule = value["rule"]
    require(
        rule is JsonObject && rule["type"] is JsonPrimitive &&
                (rule["type"] as JsonPrimitive).isString && (rule["type"] as JsonPrimitive).content.isNotBlank()
    ) {
        "Parser recurrence is missing its core rule."
    }
    fun string(name: String): String? = value[name]?.takeUnless { it == JsonNull }?.let {
        require(it is JsonPrimitive && it.isString) { "Invalid recurrence $name." }
        it.content
    }
    string("end_date")?.let(LocalDate::parse)
    string("timezone")?.let {
        require(it in ZoneId.getAvailableZoneIds()) { "Invalid recurrence timezone." }
    }
    value["remaining"]?.takeUnless { it == JsonNull }?.let {
        val count = (it as? JsonPrimitive)?.takeUnless { primitive -> primitive.isString }?.longOrNull
        require(count != null && count in 1L..4_294_967_295L) { "Invalid recurrence count." }
        require(value["end_date"] == null || value["end_date"] == JsonNull) {
            "Recurrence cannot have both an end date and count."
        }
    }
}

fun interface ItemParser {
    fun parse(request: ItemParseRequest): ItemParseResponse
}

object NativeItemParser : ItemParser {
    // Defer linking until a worker actually parses; opening the app must not fail on a missing ABI.
    private val library by lazy { System.loadLibrary("subroutine_parser_jni") }

    external fun nativeParse(requestJson: String): String

    override fun parse(request: ItemParseRequest): ItemParseResponse {
        request.validate()
        try {
            library
            val response = nativeParse(parserJson.encodeToString(ItemParseRequest.serializer(), request))
            return parserJson.decodeFromString(ItemParseResponse.serializer(), response).validate(request)
        } catch (failure: LinkageError) {
            throw IllegalStateException(
                "Item parser unavailable. Install a build containing libsubroutine_parser_jni.so for this device’s ABI.",
                failure,
            )
        }
    }
}

sealed interface ParserOutcome {
    data class Ready(val response: ItemParseResponse) : ParserOutcome
    data class Failed(val message: String) : ParserOutcome
}

suspend fun ItemParser.parseInBackground(request: ItemParseRequest): ParserOutcome = withContext(Dispatchers.Default) {
    ensureActive()
    val outcome = try {
        request.validate()
        ParserOutcome.Ready(parse(request).validate(request))
    } catch (cancelled: CancellationException) {
        throw cancelled
    } catch (failure: Exception) {
        ParserOutcome.Failed(failure.message ?: "Item parser failed. Update the app and try again.")
    }
    // JNI is synchronous and may finish after its coroutine was cancelled.
    ensureActive()
    outcome
}

internal val CreatorItemType.parserKind: ParserItemKind
    get() = when (this) {
        CreatorItemType.Action, CreatorItemType.Routine -> ParserItemKind.Action
        CreatorItemType.Event -> ParserItemKind.Event
        CreatorItemType.Marker -> ParserItemKind.Marker
        CreatorItemType.Signal -> ParserItemKind.Signal
    }
