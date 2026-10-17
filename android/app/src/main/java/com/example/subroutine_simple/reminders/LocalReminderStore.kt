package com.example.subroutine_simple.reminders

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

@Serializable
internal data class ReminderWorkspace(
    val workspaceId: String,
    val accountId: String,
    val datasetId: String,
    val authenticationIdentity: String,
    val serverOrigin: String,
)

internal data class AlertClaim(val silent: Boolean)

internal class LocalReminderStore(
    context: Context,
    name: String = "local-reminders.sqlite",

) : SQLiteOpenHelper(
    context.applicationContext, name, null, 1,
) {
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }


    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE local_commands (command_id TEXT PRIMARY KEY, identity TEXT UNIQUE NOT NULL, occurrence TEXT NOT NULL, payload TEXT NOT NULL, state TEXT NOT NULL)")
        db.execSQL("CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        db.execSQL("CREATE TABLE alerts (occurrence TEXT PRIMARY KEY, shown INTEGER NOT NULL DEFAULT 0, dismissed INTEGER NOT NULL DEFAULT 0, rendered_version TEXT, workspace_id TEXT NOT NULL)")

    }


    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        error("Unsupported local reminder migration; preserve the database")
    }

    private fun <T> transaction(block: () -> T): T {
        val db = writableDatabase
        db.beginTransaction()
        try {
            val value = block()
            db.setTransactionSuccessful()
            return value
        } finally { db.endTransaction() }
    }

    private fun read(key: String): String? = readableDatabase.rawQuery(
        "SELECT value FROM metadata WHERE key = ?", arrayOf(key),
    ).use { if (it.moveToFirst()) it.getString(0) else null }

    private fun write(key: String, value: String?) {
        if (value == null) writableDatabase.delete("metadata", "key = ?", arrayOf(key))
        else writableDatabase.insertWithOnConflict("metadata", null, ContentValues().apply {
            put("key", key); put("value", value)
        }, SQLiteDatabase.CONFLICT_REPLACE)
    }

    @Synchronized fun workspace(): ReminderWorkspace? = read("workspace")?.let { json.decodeFromString(it) }
    @Synchronized fun setWorkspace(workspace: ReminderWorkspace?) = write("workspace", workspace?.let { json.encodeToString(it) })

    @Synchronized fun datasetFor(workspace: WorkspaceDescriptor.Remote): String? = workspace()?.takeIf {
        it.workspaceId == workspace.workspaceId && it.accountId == workspace.accountId
    }?.datasetId

    @Synchronized fun localDatasetIsCurrent(command: LocalReminderCommand): Boolean = when (val workspace = command.workspace) {
        is WorkspaceDescriptor.Local -> command.datasetId == null
        is WorkspaceDescriptor.Remote -> command.datasetId != null && datasetFor(workspace) == command.datasetId
    }

    @Synchronized fun claimAlert(occurrence: String, version: String, workspaceId: String): AlertClaim? = transaction {
        val row = readableDatabase.rawQuery("SELECT shown, dismissed, rendered_version FROM alerts WHERE occurrence = ?", arrayOf(occurrence)).use {
            if (it.moveToFirst()) Triple(it.getInt(0) != 0, it.getInt(1) != 0, it.getString(2)) else null
        }
        if (row?.second == true || row?.third == version) return@transaction null
        writableDatabase.insertWithOnConflict("alerts", null, ContentValues().apply {
            put("occurrence", occurrence); put("workspace_id", workspaceId); put("shown", 1)
        }, SQLiteDatabase.CONFLICT_IGNORE)
        writableDatabase.execSQL("UPDATE alerts SET shown = 1 WHERE occurrence = ?", arrayOf(occurrence))
        AlertClaim(silent = row?.first == true)
    }

    @Synchronized fun rendered(occurrence: String, version: String) = transaction {
        writableDatabase.execSQL("UPDATE alerts SET rendered_version = ? WHERE occurrence = ?", arrayOf(version, occurrence))
        val owner = read("local-feedback-owner:$occurrence")
        if (owner != null && !version.startsWith("local-command:$owner:")) write("local-feedback-owner:$occurrence", null)
    }

    @Synchronized fun canRenderLocalFeedback(entry: StoredLocalReminderCommand, currentExpectedStart: String?): Boolean =
        localFeedbackCanRender(entry.command, entry.state, read("local-feedback-owner:${entry.command.occurrence}"),
            renderedVersion(entry.command.occurrence), currentExpectedStart)

    @Synchronized fun dismiss(occurrence: String, version: String) {
        writableDatabase.execSQL("UPDATE alerts SET dismissed = 1 WHERE occurrence = ? AND rendered_version = ?", arrayOf(occurrence, version))
    }

    @Synchronized fun alertTags(workspaceId: String): List<String> = readableDatabase.rawQuery(
        "SELECT occurrence FROM alerts WHERE workspace_id = ?", arrayOf(workspaceId),
    ).use { cursor -> buildList { while (cursor.moveToNext()) add(cursor.getString(0)) } }

    @Synchronized fun renderedVersion(occurrence: String): String? = readableDatabase.rawQuery(
        "SELECT rendered_version FROM alerts WHERE occurrence = ?", arrayOf(occurrence),
    ).use { if (it.moveToFirst()) it.getString(0) else null }

    @Synchronized fun forgetRendering(occurrence: String) {
        writableDatabase.execSQL("UPDATE alerts SET rendered_version = NULL WHERE occurrence = ?", arrayOf(occurrence))
    }

    @Synchronized fun captureLocal(candidate: LocalReminderCommand): LocalReminderCommand = transaction {
        check(localDatasetIsCurrent(candidate)) { "Notification command dataset is unavailable or changed" }
        val datasetSegment = candidate.datasetId?.let { "$it/" }.orEmpty()
        val identity = "${candidate.epoch}/${candidate.workspaceId}/$datasetSegment${candidate.actionId}/${candidate.expectedStart}"
        val existing = readableDatabase.rawQuery("SELECT payload FROM local_commands WHERE identity = ?", arrayOf(identity)).use {
            if (it.moveToFirst()) json.decodeFromString<LocalReminderCommand>(it.getString(0)) else null
        }
        existing ?: candidate.also {
            writableDatabase.insertOrThrow("local_commands", null, ContentValues().apply {
                put("command_id", it.commandId); put("identity", identity); put("occurrence", it.occurrence)
                put("payload", json.encodeToString(it)); put("state", CommandState.Pending.name)
            })
            write("local-feedback-owner:${it.occurrence}", it.commandId)
        }
    }

    @Synchronized fun localCommand(id: String): StoredLocalReminderCommand? = readableDatabase.rawQuery(
        "SELECT payload, state FROM local_commands WHERE command_id = ?", arrayOf(id),
    ).use { if (it.moveToFirst()) StoredLocalReminderCommand(json.decodeFromString(it.getString(0)), CommandState.valueOf(it.getString(1))) else null }

    @Synchronized fun localCommandFor(occurrence: String): StoredLocalReminderCommand? = readableDatabase.rawQuery(
        "SELECT payload, state FROM local_commands WHERE occurrence = ? ORDER BY rowid DESC LIMIT 1", arrayOf(occurrence),
    ).use { if (it.moveToFirst()) StoredLocalReminderCommand(json.decodeFromString(it.getString(0)), CommandState.valueOf(it.getString(1))) else null }

    @Synchronized fun localCommands(): List<StoredLocalReminderCommand> = readableDatabase.rawQuery("SELECT payload, state FROM local_commands", null).use { cursor ->
        buildList { while (cursor.moveToNext()) add(StoredLocalReminderCommand(json.decodeFromString(cursor.getString(0)), CommandState.valueOf(cursor.getString(1)))) }
    }

    @Synchronized fun retireLocalWorkspace(workspaceId: String) {
        localCommands().filter { it.command.workspaceId == workspaceId && !it.state.terminal }
            .forEach { localCommandState(it.command.commandId, CommandState.Expired) }
    }

    @Synchronized fun localCommandState(id: String, state: CommandState) = transaction {
        val current = localCommand(id)?.state ?: return@transaction
        if (canTransitionCommand(current, state)) {
            writableDatabase.execSQL("UPDATE local_commands SET state = ? WHERE command_id = ? AND state = ?", arrayOf(state.name, id, current.name))
        }
    }

    companion object {
        @Volatile private var instance: LocalReminderStore? = null
        fun get(context: Context): LocalReminderStore = instance ?: synchronized(this) {
            instance ?: LocalReminderStore(context).also { instance = it }
        }
    }
}
