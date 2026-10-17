package com.example.subroutine_simple.data.network

import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.sse.EventSource
import okhttp3.sse.EventSourceListener
import okhttp3.sse.EventSources
import java.io.IOException
import retrofit2.HttpException
import okhttp3.ResponseBody.Companion.toResponseBody

class SseManager(
    private val baseUrl: String,
    authenticatedClient: OkHttpClient,
) {
    private val sseClient = authenticatedClient.newBuilder()
        .callTimeout(0, java.util.concurrent.TimeUnit.MILLISECONDS)
        .readTimeout(75, java.util.concurrent.TimeUnit.SECONDS)
        .build()

    fun changeEvents(): Flow<Unit> = callbackFlow {
        val request = Request.Builder()
            .url("${baseUrl}v1/changes/stream")
            .header("Accept", "text/event-stream")
            .build()

        val listener = object : EventSourceListener() {
            override fun onOpen(eventSource: EventSource, response: Response) {
                trySend(Unit)
            }

            override fun onEvent(
                eventSource: EventSource,
                id: String?,
                type: String?,
                data: String,
            ) {
                if (data.isNotBlank() && data != "ping") {
                    trySend(Unit)
                }
            }

            override fun onFailure(
                eventSource: EventSource,
                t: Throwable?,
                response: Response?,
            ) {
                close(response?.takeIf { !it.isSuccessful }?.let {
                    HttpException(retrofit2.Response.error<Unit>(it.code, "".toResponseBody()))
                } ?: t ?: IOException("SSE connection failed"))
            }

            override fun onClosed(eventSource: EventSource) {
                close(IOException("SSE stream closed by server"))
            }
        }

        val eventSource = EventSources.createFactory(sseClient)
            .newEventSource(request, listener)

        awaitClose { eventSource.cancel() }
    }
}
