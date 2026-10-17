package com.example.subroutine_simple.data.network

import com.example.subroutine_simple.BuildConfig
import com.example.subroutine_simple.auth.AccessTokenProvider
import com.example.subroutine_simple.auth.AuthenticationUnavailableException
import com.example.subroutine_simple.data.models.ApiErrorCode
import com.jakewharton.retrofit2.converter.kotlinx.serialization.asConverterFactory
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Response
import okhttp3.RequestBody
import okio.BufferedSink
import okhttp3.logging.HttpLoggingInterceptor
import retrofit2.Retrofit

class RetrofitClient(
    authentication: AccessTokenProvider,
    val baseUrl: String = BuildConfig.API_BASE_URL,
    private val requireAuthentication: Boolean = true,
    callTimeoutSeconds: Long = 0,
) {
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }

    val httpClient: OkHttpClient = OkHttpClient.Builder()
        .callTimeout(callTimeoutSeconds, java.util.concurrent.TimeUnit.SECONDS)
        .retryOnConnectionFailure(false)
        .followRedirects(false)
        .followSslRedirects(false)
        .addInterceptor { chain ->
            if (BuildConfig.OFFLINE_ONLY) throw java.io.IOException("Remote access is disabled in this offline-only build")
            val token = try {
                runBlocking(Dispatchers.IO) { authentication.accessToken() }
            } catch (_: kotlinx.coroutines.CancellationException) {
                throw java.io.IOException("Authentication session changed")
            }
            if (requireAuthentication && token == null) throw java.io.IOException("Sign in to synchronize")
            val request = if (token == null) {
                chain.request()
            } else {
                chain.request().newBuilder()
                    .header("Authorization", "Bearer $token")
                    .build()
            }
            val body = request.body
            val guarded = if (body != null && request.method !in setOf("GET", "HEAD", "PUT", "DELETE", "OPTIONS")) {
                request.newBuilder().method(request.method, object : RequestBody() {
                    override fun contentType() = body.contentType()
                    override fun contentLength() = body.contentLength()
                    override fun isOneShot() = true
                    override fun writeTo(sink: BufferedSink) = body.writeTo(sink)
                }).build()
            } else request
            chain.proceed(guarded)
        }
        .addNetworkInterceptor { chain ->
            val response = chain.proceed(chain.request())
            if (!response.isSuccessful) {
                val code = try {
                    val body = response.peekBody(8_193).string()
                    if (body.length > 8_192) null else runCatching {
                        val error = (json.parseToJsonElement(body) as? JsonObject)?.get("error")
                        error?.let { json.decodeFromJsonElement<ApiErrorCode>(it) }
                    }.getOrNull()
                } catch (error: java.io.IOException) {
                    response.close()
                    throw error
                }
                if (code == ApiErrorCode.AUTHENTICATION_UNAVAILABLE) {
                    response.close()
                    throw AuthenticationUnavailableException()
                }
            }
            response
        }
        .authenticator { _, response ->
            val rejectedToken = response.request.header("Authorization")?.removePrefix("Bearer ")
            if (rejectedToken == null) {
                null
            } else if (response.responseCount() >= 2) {
                authentication.rejectSession(rejectedToken)
                null
            } else if (response.request.method !in setOf("GET", "HEAD", "PUT", "DELETE", "OPTIONS")) {
                authentication.rejectSession(rejectedToken)
                null
            } else {
                val token = try {
                    runBlocking(Dispatchers.IO) {
                        authentication.accessToken(
                            forceRefresh = true,
                            rejectedAccessToken = rejectedToken,
                        )
                    }
                } catch (_: kotlinx.coroutines.CancellationException) {
                    throw java.io.IOException("Authentication session changed")
                }
                token?.let {
                    response.request.newBuilder()
                        .header("Authorization", "Bearer $it")
                        .build()
                }
            }
        }
        .addInterceptor(
            HttpLoggingInterceptor().apply {
                redactHeader("Authorization")
                level = HttpLoggingInterceptor.Level.BASIC
            },
        )
        .build()

    val api: SubroutineApi = Retrofit.Builder()
        .baseUrl(baseUrl)
        .client(httpClient)
        .addConverterFactory(json.asConverterFactory("application/json".toMediaType()))
        .build()
        .create(SubroutineApi::class.java)
}

private fun Response.responseCount(): Int {
    var count = 1
    var prior = priorResponse
    while (prior != null) {
        count += 1
        prior = prior.priorResponse
    }
    return count
}
