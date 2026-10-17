package com.example.subroutine_simple.data.local

import android.content.Context
import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.util.Locale
import java.util.UUID

class WorkspaceRegistry(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(
        PREFERENCES_NAME,
        Context.MODE_PRIVATE,
    )

    fun remoteAccountId(serverOrigin: String, authenticationIdentity: String): String? {
        val value = preferences.getString(key(serverOrigin, authenticationIdentity), null)
            ?: return null
        return runCatching { UUID.fromString(value).toString() }.getOrNull()
    }

    fun remember(
        serverOrigin: String,
        authenticationIdentity: String,
        remoteAccountId: String,
    ) {
        val canonicalAccountId = UUID.fromString(remoteAccountId).toString()
        check(
            preferences.edit()
                .putString(key(serverOrigin, authenticationIdentity), canonicalAccountId)
                .commit(),
        ) { "Could not persist the workspace account mapping" }
    }

    fun forget(serverOrigin: String, authenticationIdentity: String) {
        check(
            preferences.edit()
                .remove(key(serverOrigin, authenticationIdentity))
                .commit(),
        ) { "Could not remove the workspace account mapping" }
    }

    private fun key(serverOrigin: String, authenticationIdentity: String): String {
        require(authenticationIdentity.isNotBlank()) { "authenticationIdentity must not be blank" }
        val normalizedOrigin = SQLiteWorkspaceStore.normalizeServerOrigin(serverOrigin)
        val digest = MessageDigest.getInstance("SHA-256")
        digest.update(normalizedOrigin.toByteArray(StandardCharsets.UTF_8))
        digest.update(0)
        digest.update(authenticationIdentity.toByteArray(StandardCharsets.UTF_8))
        return KEY_PREFIX + digest.digest().joinToString("") { byte ->
            "%02x".format(Locale.ROOT, byte.toInt() and 0xff)
        }
    }

    private companion object {
        const val PREFERENCES_NAME = "workspace-registry"
        const val KEY_PREFIX = "remote-account-"
    }
}
