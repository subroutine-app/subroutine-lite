package com.example.subroutine_simple.auth

import android.content.Context
import android.content.Intent
import androidx.core.net.toUri
import com.example.subroutine_simple.data.models.AccountInfo
import com.example.subroutine_simple.data.network.RetrofitClient
import java.io.IOException
import java.net.HttpURLConnection
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import net.openid.appauth.AppAuthConfiguration
import net.openid.appauth.AuthState
import net.openid.appauth.AuthorizationException
import net.openid.appauth.AuthorizationRequest
import net.openid.appauth.AuthorizationResponse
import net.openid.appauth.AuthorizationService
import net.openid.appauth.AuthorizationServiceConfiguration
import net.openid.appauth.CodeVerifierUtil
import net.openid.appauth.ResponseTypeValues
import net.openid.appauth.TokenRequest
import net.openid.appauth.TokenResponse
import net.openid.appauth.connectivity.ConnectionBuilder
import retrofit2.HttpException
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

internal class AppAuthBackend(
    private val context: Context,
    private val configuration: RauthyConfiguration,
) : AuthenticationBackend {
    private val connections = ConnectionBuilder { uri ->
        val url = configuration.endpoint(uri.toString()).toURL()
        (url.openConnection() as HttpURLConnection).apply {
            connectTimeout = 15_000
            readTimeout = 15_000
            instanceFollowRedirects = false
        }
    }
    private val appAuthConfiguration = AppAuthConfiguration.Builder()
        .setConnectionBuilder(connections)
        .setSkipIssuerHttpsCheck(configuration.allowLoopbackHttp && configuration.issuer.startsWith("http:"))
        .setBrowserMatcher { it.useCustomTab }
        .build()

    override suspend fun authorizationRequest(): String {
        val service = withTimeoutOrNull(30_000) {
            suspendCancellableCoroutine<AuthorizationServiceConfiguration> { continuation ->
                AuthorizationServiceConfiguration.fetchFromIssuer(configuration.issuer.toUri(), { value, _ ->
                    if (continuation.isActive) {
                        if (value != null) continuation.resume(value)
                        else continuation.resumeWithException(AuthenticationUnavailableException("Could not discover the configured issuer."))
                    }
                }, connections)
            }
        } ?: throw AuthenticationUnavailableException("Issuer discovery timed out.")
        validateConfiguration(service)
        val methods = service.discoveryDoc?.docJson?.optJSONArray("code_challenge_methods_supported")
        require(methods != null && (0 until methods.length()).any { methods.optString(it) == "S256" }) {
            "The issuer must advertise S256 PKCE"
        }
        val verifier = CodeVerifierUtil.generateRandomCodeVerifier()
        return AuthorizationRequest.Builder(
            service, configuration.clientId, ResponseTypeValues.CODE,
            RauthyConfiguration.REDIRECT_URI.toUri(),
        )
            .setScope(RauthyConfiguration.SCOPE)
            .setCodeVerifier(verifier, CodeVerifierUtil.deriveCodeVerifierChallenge(verifier), "S256")

            .build().jsonSerializeString()
    }

    fun authorizationIntent(request: String): Intent {
        val service = AuthorizationService(context, appAuthConfiguration)
        return try {
            service.getAuthorizationRequestIntent(AuthorizationRequest.jsonDeserialize(request))
        } finally {
            service.dispose()
        }
    }

    override suspend fun exchange(request: String, response: String): Credentials {
        val expected = AuthorizationRequest.jsonDeserialize(request)
        val received = AuthorizationResponse.jsonDeserialize(response)
        validateConfiguration(expected.configuration)
        require(expected.clientId == configuration.clientId && expected.redirectUri.toString() == RauthyConfiguration.REDIRECT_URI)
        require(expected.codeVerifier != null && expected.codeVerifierChallengeMethod == "S256" && expected.nonce != null)
        require(expected.state != null && received.state == expected.state) {
            "Authorization response does not match the pending request"
        }
        require(!received.authorizationCode.isNullOrBlank() && received.accessToken == null)
        val verifiedResponse = AuthorizationResponse.Builder(expected)
            .setAuthorizationCode(received.authorizationCode)
            .setState(received.state)
            .build()
        val state = AuthState(verifiedResponse, null)
        val token = tokenRequest(verifiedResponse.createTokenExchangeRequest())
        if (token.idToken.isNullOrBlank()) throw AuthenticationIndeterminateException()
        return credentials(state, token)
    }

    override suspend fun refresh(credentials: Credentials, beforeSubmit: () -> Unit): Credentials {
        if (credentials.isFresh(System.currentTimeMillis())) {
            throw AuthenticationUnavailableException("Refresh is not yet eligible.")
        }
        val state = AuthState.jsonDeserialize(credentials.serialized)
        validateConfiguration(requireNotNull(state.authorizationServiceConfiguration))
        val request = state.createTokenRefreshRequest()
        require(request.clientId == configuration.clientId)
        beforeSubmit()
        val token = tokenRequest(request)
        return credentials(state, token)
    }

    private suspend fun tokenRequest(request: TokenRequest): TokenResponse = withContext(Dispatchers.Main) {
        val service = AuthorizationService(context, appAuthConfiguration)
        try {
            withTimeoutOrNull(30_000) {
                suspendCancellableCoroutine { continuation ->
                    service.performTokenRequest(request) { token, error ->
                        if (continuation.isActive) {
                            if (token != null && error == null) continuation.resume(token)
                            else continuation.resumeWithException(tokenRequestFailure(error))
                        }
                    }
                }
            } ?: throw AuthenticationIndeterminateException()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (unavailable: AuthenticationUnavailableException) {
            throw unavailable
        } catch (refused: AuthenticationRequiredException) {
            throw refused
        } catch (_: Exception) {
            throw AuthenticationIndeterminateException()
        } finally {
            service.dispose()
        }
    }

    private fun credentials(state: AuthState, response: TokenResponse): Credentials = try {
        requireReplacementRefreshToken(response.request.refreshToken, response.refreshToken)
        require(response.tokenType.equals("Bearer", ignoreCase = true)) { "Expected a Bearer access token" }
        require((response.scope ?: state.scope)?.split(' ')?.contains("subroutine:sync") == true) { "Sync scope was not granted" }
        val accessToken = requireNotNull(response.accessToken).also { require(it.isNotBlank()) }
        val expiration = requireNotNull(response.accessTokenExpirationTime)
        require(expiration > System.currentTimeMillis()) { "The access token has already expired" }
        state.update(response, null)
        Credentials(state.jsonSerializeString(), accessToken, expiration, canRefresh = true)
    } catch (_: Exception) {
        throw AuthenticationIndeterminateException()
    }

    override suspend fun account(credentials: Credentials): AccountInfo {
        val tokens = object : AccessTokenProvider {
            override suspend fun accessToken(forceRefresh: Boolean, rejectedAccessToken: String?): String? =
                if (forceRefresh) null else credentials.accessToken
            override fun rejectSession() = Unit
        }
        val client = RetrofitClient(tokens, configuration.apiBaseUrl, requireAuthentication = true, callTimeoutSeconds = 20)
        return try {
            client.api.getAccount()
        } catch (error: HttpException) {
            if (error.code() == 401 || error.code() == 403) throw AuthenticationRequiredException()
            throw AuthenticationUnavailableException()
        } finally {
            client.httpClient.connectionPool.evictAll()
            client.httpClient.dispatcher.executorService.shutdown()
        }
    }

    private fun validateConfiguration(service: AuthorizationServiceConfiguration) {
        require(service.discoveryDoc?.issuer == configuration.issuer) { "Discovery issuer mismatch" }
        configuration.endpoint(service.authorizationEndpoint.toString())
        configuration.endpoint(service.tokenEndpoint.toString())
    }
}

internal fun tokenRequestFailure(error: AuthorizationException?): IOException = when {
    error?.type != AuthorizationException.TYPE_OAUTH_TOKEN_ERROR || error.error.isNullOrBlank() ->
        AuthenticationIndeterminateException()
    error.error == "server_error" || error.error == "temporarily_unavailable" ->
        AuthenticationUnavailableException()
    else -> AuthenticationRequiredException()
}

internal fun requireReplacementRefreshToken(previous: String?, replacement: String?) {
    if (replacement.isNullOrBlank() || replacement == previous) throw AuthenticationIndeterminateException()
}
