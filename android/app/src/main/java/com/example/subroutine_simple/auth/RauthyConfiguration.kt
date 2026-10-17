package com.example.subroutine_simple.auth

import com.example.subroutine_simple.BuildConfig
import java.net.URI

internal data class RauthyConfiguration(
    val issuer: String,
    val clientId: String,
    val apiBaseUrl: String,
    val allowLoopbackHttp: Boolean,
) {
    init {
        require(clientId.isNotEmpty() && clientId.all { it in '!'..'~' }) { "Invalid Rauthy client ID" }
        val uri = endpoint(issuer)
        require(uri.rawQuery == null && uri.rawFragment == null) { "Invalid issuer URL" }
        require(endpoint(apiBaseUrl).rawQuery == null && apiBaseUrl.endsWith("/")) { "Invalid API base URL" }
    }

    fun endpoint(value: String): URI {
        val uri = URI(value)
        require(uri.isAbsolute && !uri.host.isNullOrEmpty() && uri.rawUserInfo == null && uri.rawFragment == null &&
                    (uri.port == -1 || uri.port in 1..65535) && !uri.rawAuthority.endsWith(":")) {
            "Invalid authentication endpoint"
        }
        require(uri.scheme == "https" || (allowLoopbackHttp && uri.scheme == "http" &&
            uri.host in setOf("localhost", "127.0.0.1", "[::1]"))) {
            "Authentication requires HTTPS (debug loopback HTTP needs explicit opt-in)"
        }
        return uri
    }

    val storageBinding: String get() = "$issuer\n$clientId\n$apiBaseUrl"

    companion object {
        const val REDIRECT_URI = "dev.subroutine.lite://oauth/callback"
        const val SCOPE = "openid subroutine:sync"

        fun fromBuild(): RauthyConfiguration? {
            if (BuildConfig.OFFLINE_ONLY) return null
            return RauthyConfiguration(
                BuildConfig.RAUTHY_ISSUER, BuildConfig.RAUTHY_CLIENT_ID, BuildConfig.API_BASE_URL,
                BuildConfig.DEBUG && BuildConfig.RAUTHY_ALLOW_INSECURE_LOOPBACK,
            )
        }
    }
}
