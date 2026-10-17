package com.example.subroutine_simple.auth

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import com.example.subroutine_simple.data.models.AccountInfo
import java.io.File
import java.io.IOException
import java.security.KeyStore
import java.security.MessageDigest
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

@Serializable
internal class Credentials(
    val serialized: String,
    val accessToken: String,
    val expiresAt: Long,
    val canRefresh: Boolean,
) {
    fun isFresh(now: Long): Boolean = expiresAt > now + 60_000
    override fun toString(): String = "Credentials(redacted)"
}

@Serializable
internal data class SavedSession(val account: AccountInfo, val credentials: Credentials?)

@Serializable
internal class PendingAuthorization(val id: String, val request: String?, val createdAt: Long) {
    fun isFresh(now: Long): Boolean = now >= createdAt && now - createdAt < 10 * 60_000
    override fun toString(): String = "PendingAuthorization(redacted)"
}

@Serializable
internal data class SavedAuthentication(
    val session: SavedSession? = null,
    val pending: PendingAuthorization? = null,
)

internal interface AuthenticationStorage {
    fun read(): SavedAuthentication
    fun write(value: SavedAuthentication)
}

internal class KeystoreAuthenticationStorage(context: Context, binding: String) : AuthenticationStorage {
    private val aad = binding.toByteArray(Charsets.UTF_8)
    private val keyId = MessageDigest.getInstance("SHA-256").digest(aad).joinToString("") { "%02x".format(it) }
    private val alias = "subroutine-lite.auth.$keyId"
    private val file = AtomicFile(File(context.noBackupFilesDir, "auth-$keyId.enc"))
    private val json = Json { ignoreUnknownKeys = true }

    override fun read(): SavedAuthentication {
        if (!file.baseFile.exists()) return SavedAuthentication()
        try {
            val bytes = file.openRead().use { input ->
                val size = input.channel.size()
                require(size in 30..1_048_576)
                ByteArray(size.toInt()).also { bytes ->
                    java.io.DataInputStream(input).readFully(bytes)
                    require(bytes[0] == 1.toByte())
                }
            }
            val key = key(create = false) ?: throw IOException("Authentication key unavailable")
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, bytes.copyOfRange(1, 13)))
            cipher.updateAAD(aad)
            return json.decodeFromString<SavedAuthentication>(cipher.doFinal(bytes, 13, bytes.size - 13).toString(Charsets.UTF_8))
        } catch (_: Exception) {
            throw IOException("Saved authentication cannot be read. Sign out to reset credentials; offline workspaces are retained.")
        }
    }

    override fun write(value: SavedAuthentication) {
        if (value == SavedAuthentication()) {
            file.delete()
            if (file.baseFile.exists()) throw IOException("Could not remove saved authentication")
            try {
                KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.deleteEntry(alias)
            } catch (_: Exception) {
                throw IOException("Could not remove the authentication key")
            }
            return
        }
        try {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, requireNotNull(key(create = true)))
            cipher.updateAAD(aad)
            val bytes = byteArrayOf(1) + cipher.iv + cipher.doFinal(json.encodeToString(value).toByteArray(Charsets.UTF_8))
            val stream = file.startWrite()
            try {
                stream.write(bytes)
                file.finishWrite(stream)
            } catch (error: Exception) {
                file.failWrite(stream)
                throw error
            }
        } catch (_: Exception) {
            throw IOException("Could not securely save authentication")
        }
    }

    private fun key(create: Boolean): SecretKey? {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(alias, null) as? SecretKey)?.let { return it }
        if (!create) return null
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setKeySize(256)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .build())
        }.generateKey()
    }
}
