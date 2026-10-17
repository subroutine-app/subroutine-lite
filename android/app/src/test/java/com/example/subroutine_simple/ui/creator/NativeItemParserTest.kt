package com.example.subroutine_simple.ui.creator

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.*
import org.junit.Test

class NativeItemParserTest {
    private fun request(text: String = "Café 🧪 tomorrow") = ItemParseRequest(
        text, ParserItemKind.Action, "2026-10-15T12:00:00Z", "Europe/Berlin", "2026-10-20",
    )

    private fun response() = ItemParseResponse(
        version = 1,
        title = "Café 🧪",
        whenValue = "2026-10-16",
        warnings = emptyList(),
        highlights = listOf(ParserHighlight(8, 16, ParserHighlightKind.When)),
    )

    @Test
    fun requestIncludesVersionAndExplicitContext() {
        val json = parserJson.encodeToString(ItemParseRequest.serializer(), request())
        val decoded = parserJson.parseToJsonElement(json).toString()
        assertTrue(decoded.contains("\"version\":1"))
        assertTrue(decoded.contains("\"timezone\":\"Europe/Berlin\""))
        assertEquals(request(), parserJson.decodeFromString(ItemParseRequest.serializer(), json))
        assertTrue(runCatching { request().copy(now = "2026-10-15T14:00:00+02:00").validate() }.isFailure)
        assertTrue(runCatching { request().copy(timezone = "+02:00").validate() }.isFailure)
    }

    @Test
    fun validatesUtf16OffsetsWithoutLosingNonAsciiText() {
        val value = response().validate(request())
        assertEquals(
            "tomorrow",
            request().text.substring(value.highlights.single().start, value.highlights.single().end)
        )
        listOf(
            ParserHighlight(6, 7, ParserHighlightKind.Title),
            ParserHighlight(-1, 4, ParserHighlightKind.When),
            ParserHighlight(8, 17, ParserHighlightKind.When),
        ).forEach { invalid ->
            assertTrue(runCatching { response().copy(highlights = listOf(invalid)).validate(request()) }.isFailure)
        }
    }

    @Test
    fun rejectsIncompatibleOrUnsafeResponses() {
        val invalid = listOf(
            response().copy(version = 2),
            response().copy(whenValue = "2026-10-16T10:00:00+02:00"),
            response().copy(whenValue = "2026-02-30"),
            response().copy(naiveTime = "25:00:00"),
            response().copy(durationSeconds = -1),
            response().copy(recurrence = parserJson.parseToJsonElement("[]")),
        )
        invalid.forEach { assertTrue(runCatching { it.validate(request()) }.isFailure) }
        assertTrue(runCatching {
            parserJson.decodeFromString(
                ItemParseResponse.serializer(),
                "{\"title\":\"unsafe\",\"warnings\":[],\"highlights\":[]}"
            )
        }.isFailure)
    }

    @Test
    fun preservesCoreRecurrenceTimezoneAndLimits() {
        val recurrence = parserJson.parseToJsonElement(
            """{"rule":{"type":"relative","value":{"unit":"weeks","interval":2}},"end_date":"2027-02-10","remaining":null,"timezone":"America/New_York"}""",
        )
        assertEquals(recurrence, response().copy(recurrence = recurrence).validate(request()).recurrence)
        val count = parserJson.parseToJsonElement(
            """{"rule":{"type":"relative","value":{"unit":"days","interval":1}},"remaining":12,"timezone":"Europe/Berlin"}""",
        )
        assertEquals(count, response().copy(recurrence = count).validate(request()).recurrence)
    }

    @Test
    fun emptyInputIsNotAnErrorOrImplicitSchedule() {
        val empty = ItemParseResponse(1, "", warnings = emptyList(), highlights = emptyList())
        assertNull(empty.validate(request("")).error)
        assertTrue(runCatching { empty.copy(whenValue = "2026-10-15").validate(request("")).error }.isFailure)
    }

    @Test
    fun parserRunsOffCallerThreadAndRethrowsCancellation() = runBlocking {
        val caller = Thread.currentThread()
        val parser = ItemParser {
            assertNotEquals(caller, Thread.currentThread())
            response()
        }
        assertTrue(parser.parseInBackground(request()) is ParserOutcome.Ready)
        try {
            ItemParser { throw CancellationException("cancelled") }.parseInBackground(request())
            fail("Cancellation must not become a parser error")
        } catch (_: CancellationException) {
            // Expected control flow, not a displayed parser failure.
        }
    }

    @Test
    fun cancellationDiscardsSynchronousNativeCompletion() = runBlocking {
        val entered = CompletableDeferred<Unit>()
        val release = CountDownLatch(1)
        val delivered = AtomicBoolean(false)
        val parser = ItemParser {
            entered.complete(Unit)
            check(release.await(5, TimeUnit.SECONDS))
            response()
        }
        val job = launch { parser.parseInBackground(request()); delivered.set(true) }
        try {
            withTimeout(5_000) { entered.await() }
            job.cancel()
        } finally {
            release.countDown()
        }
        job.join()
        assertFalse(delivered.get())
    }
}
