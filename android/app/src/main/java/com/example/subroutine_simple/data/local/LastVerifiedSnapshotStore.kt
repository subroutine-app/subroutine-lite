package com.example.subroutine_simple.data.local

import com.example.subroutine_simple.data.models.AllData
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.nio.charset.StandardCharsets
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.util.UUID
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

class LastVerifiedSnapshotStore(filesDir: File, serverOrigin: String) {
    private val origin = SQLiteWorkspaceStore.normalizeServerOrigin(serverOrigin)
    private val cacheDirectory = File(filesDir, CACHE_DIRECTORY)
    private val json = Json {
        encodeDefaults = true
        ignoreUnknownKeys = true
    }

    fun read(accountIdentity: String): AllData? {
        val accountKey = accountKey(accountIdentity)
        val cacheFile = cacheFile(accountKey)
        if (!cacheFile.isFile) return null

        val cached = try {
            json.decodeFromString<CachedSnapshot>(cacheFile.readText(StandardCharsets.UTF_8))
        } catch (_: SerializationException) {
            quarantine(cacheFile)
            return null
        } catch (_: IllegalArgumentException) {
            quarantine(cacheFile)
            return null
        } catch (_: IOException) {
            return null
        } catch (_: SecurityException) {
            return null
        }

        if (cached.version != CACHE_VERSION || cached.accountKey != accountKey) {
            quarantine(cacheFile)
            return null
        }
        return cached.snapshot
    }

    @Throws(IOException::class)
    fun write(accountIdentity: String, snapshot: AllData) {
        val accountKey = accountKey(accountIdentity)
        ensureCacheDirectory()
        val target = cacheFile(accountKey)
        val temporary = File.createTempFile("${target.name}.", TEMP_SUFFIX, cacheDirectory)

        try {
            val bytes = json.encodeToString(
                CachedSnapshot(
                    accountKey = accountKey,
                    snapshot = snapshot,
                ),
            ).toByteArray(StandardCharsets.UTF_8)
            FileOutputStream(temporary).use { output ->
                output.write(bytes)
                output.flush()
                output.fd.sync()
            }
            try {
                Files.move(
                    temporary.toPath(),
                    target.toPath(),
                    StandardCopyOption.ATOMIC_MOVE,
                    StandardCopyOption.REPLACE_EXISTING,
                )
            } catch (unsupported: AtomicMoveNotSupportedException) {
                throw IOException("The app files directory does not support atomic cache replacement", unsupported)
            }
        } finally {
            if (temporary.exists()) temporary.delete()
        }
    }

    fun delete(accountIdentity: String) {
        val file = cacheFile(accountKey(accountIdentity))
        if (file.exists() && !file.delete()) {
            throw IOException("Could not remove the last-verified account snapshot")
        }
    }

    private fun ensureCacheDirectory() {
        if (cacheDirectory.isDirectory) return
        if (!cacheDirectory.mkdirs() && !cacheDirectory.isDirectory) {
            throw IOException("Could not create the last-verified snapshot cache directory")
        }
    }

    private fun cacheFile(accountKey: String): File =
        File(cacheDirectory, "$CACHE_FILE_PREFIX$accountKey$CACHE_FILE_SUFFIX")

    private fun quarantine(cacheFile: File) {
        try {
            val quarantineDirectory = File(cacheDirectory, QUARANTINE_DIRECTORY)
            if (!quarantineDirectory.isDirectory &&
                !quarantineDirectory.mkdirs() &&
                !quarantineDirectory.isDirectory
            ) {
                return
            }
            val quarantined = File(
                quarantineDirectory,
                "${cacheFile.name}.corrupt-${System.currentTimeMillis()}-${UUID.randomUUID()}",
            )
            try {
                Files.move(cacheFile.toPath(), quarantined.toPath(), StandardCopyOption.ATOMIC_MOVE)
            } catch (_: AtomicMoveNotSupportedException) {
            }
        } catch (_: IOException) {
        } catch (_: SecurityException) {
        }
    }

    private fun accountKey(accountIdentity: String): String {
        require(accountIdentity.isNotBlank()) { "accountIdentity must not be blank" }
        return MessageDigest.getInstance("SHA-256")
            .digest("$origin\u0000$accountIdentity".toByteArray(StandardCharsets.UTF_8))
            .joinToString(separator = "") { byte -> "%02x".format(byte.toInt() and 0xff) }
    }

    @Serializable
    private data class CachedSnapshot(
        val version: Int = CACHE_VERSION,
        @SerialName("account_key") val accountKey: String,
        val snapshot: AllData,
    )

    private companion object {
        const val CACHE_VERSION = 1
        const val CACHE_DIRECTORY = "last-verified-snapshots"
        const val QUARANTINE_DIRECTORY = "quarantine"
        const val CACHE_FILE_PREFIX = "snapshot-"
        const val CACHE_FILE_SUFFIX = ".json"
        const val TEMP_SUFFIX = ".tmp"
    }
}
