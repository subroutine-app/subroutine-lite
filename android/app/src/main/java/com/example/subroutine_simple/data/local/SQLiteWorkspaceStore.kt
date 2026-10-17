package com.example.subroutine_simple.data.local

import android.content.ContentValues
import android.content.Context
import android.database.DatabaseErrorHandler
import android.database.sqlite.SQLiteConstraintException
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteDatabaseCorruptException
import android.database.sqlite.SQLiteException
import android.database.sqlite.SQLiteOpenHelper
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.ActionTemplate
import com.example.subroutine_simple.data.models.AllData
import com.example.subroutine_simple.data.models.ApiErrorBody
import com.example.subroutine_simple.data.models.ApiErrorCode
import com.example.subroutine_simple.data.models.ClientMutation
import com.example.subroutine_simple.data.models.DataDelta
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.EventTemplate
import com.example.subroutine_simple.data.models.MUTATION_PROTOCOL_VERSION
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.MarkerTemplate
import com.example.subroutine_simple.data.models.MutationEffect
import com.example.subroutine_simple.data.models.MutationOperation
import com.example.subroutine_simple.data.models.MutationReceipt
import com.example.subroutine_simple.data.models.MutationRequest
import com.example.subroutine_simple.data.models.OptimisticPatch

import com.example.subroutine_simple.data.models.ResourceKey
import com.example.subroutine_simple.data.models.ResourceValue
import com.example.subroutine_simple.data.models.Routine
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.data.models.SignalTemplate
import com.example.subroutine_simple.data.models.UuidV7
import java.net.IDN
import java.net.URI
import java.nio.ByteBuffer
import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.time.Instant
import java.util.Locale
import java.util.UUID
import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject

class SQLiteWorkspaceStore private constructor(
    context: Context,
    private val requestedWorkspace: NormalizedWorkspaceSpec,
    val databaseName: String,
    private val expectedClientId: String?,
) : AutoCloseable {
    private val json = Json {
        encodeDefaults = true
        ignoreUnknownKeys = true
        classDiscriminator = "type"
    }
    private val helper = WorkspaceOpenHelper(context.applicationContext ?: context, databaseName)
    private var openedIdentity: WorkspaceIdentity? = null

    val identity: WorkspaceIdentity
        @Synchronized get() = openedIdentity
            ?: throw WorkspaceOpenException("Workspace has not completed opening")

    private fun initialize() {
        try {
            val database = helper.writableDatabase
            database.enableWriteAheadLogging()
            configureRuntime(database)
            verifyRuntimeConfiguration(database)
            runQuickCheck(database)
            openedIdentity = initializeIdentity(database)
            blockUnsupportedRequests()
        } catch (error: SQLiteWorkspaceException) {
            helper.close()
            throw error
        } catch (error: SQLiteDatabaseCorruptException) {
            helper.close()
            throw CorruptWorkspaceDatabaseException(
                "Workspace database is corrupt and was left in place for recovery",
                error,
            )
        } catch (error: SQLiteException) {
            helper.close()
            throw WorkspaceOpenException("Could not open workspace database without modifying it", error)
        }
    }

    @OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)
    private fun blockUnsupportedRequests() = transaction { database ->
        val descriptor = MutationRequest.serializer().descriptor
        val supported = (0 until descriptor.elementsCount).map(descriptor::getElementName).toSet()
        val unsupported = database.rawQuery(
            "SELECT mutation_id, request_json FROM $TABLE_OUTBOX WHERE state = ?",
            arrayOf(OutboxStatus.PENDING.storedValue),
        ).use { cursor ->
            buildList {
                while (cursor.moveToNext()) {
                    val fields = json.parseToJsonElement(cursor.getBlob(1).toString(StandardCharsets.UTF_8)).jsonObject.keys
                    if (!supported.containsAll(fields)) add(cursor.getString(0))
                }
            }
        }
        unsupported.forEach { mutationId ->
            database.update(TABLE_OUTBOX, ContentValues().apply {
                put("state", OutboxStatus.BLOCKED.storedValue)
                put("blocked_error_json", encode(ApiErrorBody(
                    ApiErrorCode.UNSUPPORTED_PROTOCOL,
                    "This saved change uses fields no longer supported. Review it before retrying.",
                    mutationId = mutationId, retryable = false,
                ), ApiErrorBody.serializer()))
                putNull("next_attempt_at_ms")
                putNull("last_error")
            }, "mutation_id = ?", arrayOf(mutationId))
        }
    }

    @Synchronized
    fun state(): WorkspaceSyncState = loadSyncState(database())

    @Synchronized
    fun loadProjection(): WorkspaceProjection = loadProjectionFrom(database())


    @Synchronized
    fun installSnapshot(data: AllData, expected: WorkspaceSyncState): WorkspaceProjection =
        transaction { database ->
            val incomingDataset = nonNilUuid(data.datasetId)
            if (requestedWorkspace is NormalizedWorkspaceSpec.Remote && incomingDataset == null) {
                throw InvalidWorkspaceMutationException(
                    "Remote snapshots must carry a non-nil dataset identity",
                )
            }
            if (data.seq < 0L) {
                throw InvalidWorkspaceMutationException("Snapshot sequence cannot be negative")
            }

            val current = loadSyncState(database)
            if (current != expected) throw StaleCanonicalStateException()
            val pending = outboxCount(database)
            if (current.datasetId != incomingDataset && pending > 0) {
                throw DatasetChangedWithPendingIntentException(
                    current.datasetId,
                    incomingDataset,
                    pending,
                )
            }
            if (current.datasetId == incomingDataset && data.seq < current.canonicalSeq) {
                throw SequenceRegressionException(current.canonicalSeq, data.seq)
            }

            database.delete(TABLE_CANONICAL, null, null)
            snapshotPayloads(data).forEach { payload ->
                insertCanonical(
                    database,
                    payload,
                    payload.snapshotOrdinal
                        ?: throw WorkspaceInvariantException("Snapshot resource has no ordinal"),
                )
            }
            database.update(
                TABLE_SYNC_STATE,
                ContentValues().apply {
                    putNullableString("dataset_id", incomingDataset)
                    put("canonical_seq", data.seq)
                    put("authoritative_snapshot", 1)
                },
                "singleton = 1",
                null,
            )
            retireConfirmed(database, incomingDataset, data.seq)
            loadProjectionFrom(database)
        }

    @Synchronized
    fun applyDelta(delta: DataDelta): WorkspaceProjection = transaction { database ->
        val incomingDataset = nonNilUuid(delta.datasetId)
            ?: throw InvalidWorkspaceMutationException("Delta is missing its dataset identity")
        val current = loadSyncState(database)
        if (!current.authoritativeSnapshot) throw NoAuthoritativeSnapshotException()
        if (current.datasetId != incomingDataset) {
            throw DatasetMismatchException(current.datasetId, incomingDataset)
        }
        if (delta.seq < current.canonicalSeq) {
            throw SequenceRegressionException(current.canonicalSeq, delta.seq)
        }

        deltaPayloads(delta).forEach { upsertCanonical(database, it) }
        tombstoneKeys(delta).forEach { key ->
            database.delete(
                TABLE_CANONICAL,
                "kind = ? AND resource_id = ?",
                arrayOf(key.kind, key.id),
            )
        }
        delta.routineOrder.forEachIndexed { ordinal, id ->
            database.update(
                TABLE_CANONICAL,
                ContentValues().apply { put("ordinal", ordinal) },
                "kind = ? AND resource_id = ?",
                arrayOf(KIND_ROUTINE, id),
            )
        }
        database.update(
            TABLE_SYNC_STATE,
            ContentValues().apply { put("canonical_seq", delta.seq) },
            "singleton = 1",
            null,
        )
        retireConfirmed(database, incomingDataset, delta.seq)
        loadProjectionFrom(database)
    }

    @Synchronized
    fun resolveStaleDelta(
        mutationId: String,
        replacementMutationId: String,
        since: Long,
        delta: DataDelta,
    ): StaleResolution = transaction { database ->
        val incomingDataset = nonNilUuid(delta.datasetId)
            ?: throw InvalidWorkspaceMutationException("Delta is missing its dataset identity")
        val state = loadSyncState(database)
        val entry = loadOldestOutbox(database)
            ?: throw OutboxMutationNotFoundException(mutationId)
        if (entry.mutation.request.mutationId != mutationId ||
            entry.status != OutboxStatus.PENDING
        ) {
            throw InvalidWorkspaceMutationException(
                "Stale resolution no longer targets the pending FIFO head",
            )
        }
        if (entry.mutation.request.baseSeq != since) {
            throw InvalidWorkspaceMutationException(
                "Stale delta starts at $since but the request starts at ${entry.mutation.request.baseSeq}",
            )
        }
        if (state.datasetId != incomingDataset || entry.mutation.request.datasetId != incomingDataset) {
            throw DatasetMismatchException(state.datasetId, incomingDataset)
        }
        if (delta.seq < state.canonicalSeq || delta.seq <= since) {
            throw SequenceRegressionException(maxOf(state.canonicalSeq, since + 1L), delta.seq)
        }
        requireUuid(replacementMutationId, "replacement_mutation_id")
        if (entry.sealed && replacementMutationId == mutationId) {
            throw InvalidWorkspaceMutationException(
                "A rejected sealed request requires a fresh mutation identity",
            )
        }

        if (replacementMutationId != mutationId) requireUnusedMutationId(database, replacementMutationId)
        val conflict = firstPatchDeltaConflict(entry.mutation.optimisticPatch, delta)
        deltaPayloads(delta).forEach { upsertCanonical(database, it) }
        tombstoneKeys(delta).forEach { key ->
            database.delete(
                TABLE_CANONICAL,
                "kind = ? AND resource_id = ?",
                arrayOf(key.kind, key.id),
            )
        }
        delta.routineOrder.forEachIndexed { ordinal, id ->
            database.update(
                TABLE_CANONICAL,
                ContentValues().apply { put("ordinal", ordinal) },
                "kind = ? AND resource_id = ?",
                arrayOf(KIND_ROUTINE, id),
            )
        }
        database.update(
            TABLE_SYNC_STATE,
            ContentValues().apply { put("canonical_seq", delta.seq) },
            "singleton = 1",
            null,
        )
        retireConfirmed(database, incomingDataset, delta.seq)

        val kind = if (conflict != null) {
            val error = ApiErrorBody(
                error = ApiErrorCode.DOMAIN_CONFLICT,
                message = "The same resource changed remotely while this edit was pending.",
                mutationId = mutationId,
                resource = conflict,
                currentSeq = delta.seq,
                currentDatasetId = incomingDataset,
                retryable = false,
            )
            database.update(
                TABLE_OUTBOX,
                ContentValues().apply {
                    put("state", OutboxStatus.BLOCKED.storedValue)
                    put("blocked_error_json", encode(error, ApiErrorBody.serializer()))
                    putNull("next_attempt_at_ms")
                    putNull("last_error")
                },
                "mutation_id = ? AND state = ?",
                arrayOf(mutationId, OutboxStatus.PENDING.storedValue),
            ).also { requireOutboxChange(it, mutationId) }
            StaleResolutionKind.BLOCKED
        } else {
            val replacement = entry.mutation.request.copy(
                mutationId = replacementMutationId,
                baseSeq = delta.seq,
            )
            database.update(
                TABLE_OUTBOX,
                ContentValues().apply {
                    put("mutation_id", replacementMutationId)
                    put(
                        "request_json",
                        encode(
                            replacement,
                            com.example.subroutine_simple.data.models.MutationRequest.serializer(),
                        ),
                    )
                    put("sealed", 0)
                    put("attempt_count", 0)
                    putNull("next_attempt_at_ms")
                    putNull("last_error")
                    putNull("blocked_error_json")
                },
                "mutation_id = ? AND state = ?",
                arrayOf(mutationId, OutboxStatus.PENDING.storedValue),
            ).also { requireOutboxChange(it, mutationId) }
            remapReminderCommand(database, mutationId, replacementMutationId)
            StaleResolutionKind.REBASED
        }
        StaleResolution(loadProjectionFrom(database), kind)
    }

    @Synchronized
    fun applyLocalPatch(patch: OptimisticPatch): WorkspaceProjection = transaction { database ->
        applyLocalPatchIn(database, patch)
        loadProjectionFrom(database)
    }

    private fun applyLocalPatchIn(database: SQLiteDatabase, patch: OptimisticPatch) {
        if (identity !is WorkspaceIdentity.Local) {
            throw InvalidWorkspaceMutationException(
                "Direct local patches are only valid in the signed-out workspace",
            )
        }
        val current = loadProjectionFrom(database).data
        val updated = applyPatch(current, patch)
        database.delete(TABLE_CANONICAL, null, null)
        snapshotPayloads(updated).forEach { payload ->
            insertCanonical(
                database,
                payload,
                payload.snapshotOrdinal
                    ?: throw WorkspaceInvariantException("Local resource has no ordinal"),
            )
        }
    }

    @Synchronized
    fun reminderCommandStatus(commandId: String): ReminderCommandStatus? =
        loadReminderCommand(database(), commandId)?.status

    @Synchronized
    fun enqueueReminderCommand(
        commandId: String,
        datasetId: String?,
        actionId: String,
        expectedStart: String,
        completedAt: Instant,
        complete: Boolean,

    ): ReminderCommandStatus = transaction { database ->
        val uuid = requireUuid(commandId, "command_id")
        if (uuid.toString() != commandId || uuid == UUID(0L, 0L)) {
            throw InvalidWorkspaceMutationException("Command identity must be a canonical non-nil UUID")
        }
        val fingerprint = MessageDigest.getInstance("SHA-256").digest(
            json.encodeToString(
                listOf(
                    "reminder-command-v1", datasetId, actionId, expectedStart,
                    completedAt.toString(), complete.toString(), null, null,
                ),
            ).toByteArray(StandardCharsets.UTF_8),
        )
        loadReminderCommand(database, commandId)?.let { existing ->
            return@transaction if (existing.fingerprint.contentEquals(fingerprint)) {
                existing.status
            } else {
                ReminderCommandStatus.CONFLICT
            }
        }
        if (mutationIdReserved(database, commandId)) return@transaction ReminderCommandStatus.CONFLICT

        val state = loadSyncState(database)
        val remote = identity is WorkspaceIdentity.Remote
        if (remote && (!state.authoritativeSnapshot || state.datasetId == null)) {
            throw NoAuthoritativeSnapshotException()
        }
        val action = loadProjectionFrom(database).data.actions.firstOrNull { it.id == actionId }
        val status = when {
            datasetId != state.datasetId || action == null || !action.queued ||
                    action.completion != null || action.start != expectedStart -> ReminderCommandStatus.SUPERSEDED


            !remote && complete && action.recurrence != null -> ReminderCommandStatus.CONFLICT
            remote -> ReminderCommandStatus.QUEUED
            else -> ReminderCommandStatus.CONFIRMED
        }
        if (status == ReminderCommandStatus.QUEUED || status == ReminderCommandStatus.CONFIRMED) {
            val current = requireNotNull(action)
            val updated = if (complete) {
                current.copy(queued = false, pinned = false, completion = completedAt.toString())
            } else {
                current.copy(queued = false, pinned = false, start = null)
            }
            val patch = OptimisticPatch(writes = listOf(ResourceValue.Action(updated)))
            if (remote) {
                enqueueAllIn(
                    database,
                    listOf(
                        ClientMutation(
                            request = MutationRequest(
                                protocolVersion = MUTATION_PROTOCOL_VERSION,
                                datasetId = requireNotNull(state.datasetId),
                                mutationId = commandId,
                                clientId = identity.clientId,
                                baseSeq = state.canonicalSeq,
                                operation = if (complete) {
                                    MutationOperation.CompleteAction(actionId, completedAt.toString())
                                } else {
                                    MutationOperation.UpsertAction(updated)
                                },

                            ),
                            optimisticPatch = patch,
                        ),
                    ),
                )
            } else {
                applyLocalPatchIn(database, patch)
            }
        }
        database.insertOrThrow(
            TABLE_REMINDER_COMMANDS,
            null,
            ContentValues().apply {
                put("command_id", commandId)
                put("fingerprint", fingerprint)
                putNullableString("mutation_id", commandId.takeIf { status == ReminderCommandStatus.QUEUED })
                put("status", status.name)
            },
        )
        status
    }

    fun enqueue(mutation: ClientMutation): WorkspaceProjection = enqueueAll(listOf(mutation))

    @Synchronized
    fun enqueueAll(mutations: List<ClientMutation>): WorkspaceProjection = transaction { database ->
        enqueueAllIn(database, mutations)
        loadProjectionFrom(database)
    }

    private fun enqueueAllIn(database: SQLiteDatabase, mutations: List<ClientMutation>) {
        val currentIdentity = identity
        if (currentIdentity !is WorkspaceIdentity.Remote) {
            throw InvalidWorkspaceMutationException(
                "A replay-safe server mutation requires a remote workspace",
            )
        }
        val state = loadSyncState(database)
        if (!state.authoritativeSnapshot) {
            throw InvalidWorkspaceMutationException(
                "Remote mutations require an authoritative snapshot",
            )
        }
        mutations.forEach { mutation ->
            val request = mutation.request
            if (request.protocolVersion != MUTATION_PROTOCOL_VERSION) {
                throw InvalidWorkspaceMutationException(
                    "Unsupported mutation protocol version ${request.protocolVersion}",
                )
            }
            if (request.clientId != currentIdentity.clientId) {
                throw InvalidWorkspaceMutationException(
                    "Request client identity does not match this workspace",
                )
            }
            if (request.datasetId != state.datasetId) {
                throw InvalidWorkspaceMutationException(
                    "Request dataset does not match canonical state",
                )
            }
            if (request.baseSeq != state.canonicalSeq) {
                throw InvalidWorkspaceMutationException(
                    "Request base sequence ${request.baseSeq} does not match canonical sequence ${state.canonicalSeq}",
                )
            }
            requireUuid(request.mutationId, "mutation_id")
            requireUnusedMutationId(database, request.mutationId)
            val values = ContentValues().apply {
                put("mutation_id", request.mutationId)
                put(
                    "request_json",
                    encode(
                        request,
                        com.example.subroutine_simple.data.models.MutationRequest.serializer(),
                    ),
                )
                put("patch_json", encode(mutation.optimisticPatch, OptimisticPatch.serializer()))
                put("created_at_ms", System.currentTimeMillis())
            }
            try {
                database.insertOrThrow(TABLE_OUTBOX, null, values)
            } catch (error: SQLiteConstraintException) {
                throw InvalidWorkspaceMutationException(
                    "Mutation ${request.mutationId} is already present in the outbox",
                )
            }
        }
    }

    @Synchronized
    fun oldestOutbox(): OutboxEntry? = loadOldestOutbox(database())

    @Synchronized
    fun outbox(mutationId: String): OutboxEntry? = loadOutbox(database(), mutationId)

    @Synchronized
    fun nextSendable(nowMillis: Long = System.currentTimeMillis()): OutboxEntry? =
        transaction { database ->
            val entry = loadOldestOutbox(database) ?: return@transaction null
            if (entry.status != OutboxStatus.PENDING ||
                entry.nextAttemptAtMillis?.let { it > nowMillis } == true
            ) {
                return@transaction null
            }
            markAttemptIn(database, entry.mutation.request.mutationId, requireHead = true)
            loadOutbox(database, entry.mutation.request.mutationId)
        }

    @Synchronized
    fun markSealed(mutationId: String) {
        transaction { database ->
            val head = loadOldestOutbox(database)
                ?: throw OutboxMutationNotFoundException(mutationId)
            if (head.mutation.request.mutationId != mutationId ||
                head.status != OutboxStatus.PENDING
            ) {
                throw InvalidWorkspaceMutationException(
                    "Only the pending FIFO head may be sealed",
                )
            }
            val changed = database.update(
                TABLE_OUTBOX,
                ContentValues().apply { put("sealed", 1) },
                "mutation_id = ? AND state = ?",
                arrayOf(mutationId, OutboxStatus.PENDING.storedValue),
            )
            requireOutboxChange(changed, mutationId)
        }
    }

    @Synchronized
    fun markAttempt(mutationId: String): OutboxEntry = transaction { database ->
        markAttemptIn(database, mutationId, requireHead = true)
        loadOutbox(database, mutationId) ?: throw OutboxMutationNotFoundException(mutationId)
    }

    @Synchronized
    fun recordRetry(mutationId: String, nextAttemptAtMillis: Long, error: String) {
        transaction { database ->
            val changed = database.update(
                TABLE_OUTBOX,
                ContentValues().apply {
                    put("state", OutboxStatus.PENDING.storedValue)
                    put("next_attempt_at_ms", nextAttemptAtMillis)
                    put("last_error", error)
                },
                "mutation_id = ? AND state = ?",
                arrayOf(mutationId, OutboxStatus.PENDING.storedValue),
            )
            requireOutboxChange(changed, mutationId)
        }
    }

    @Synchronized
    fun recordReceipt(receipt: MutationReceipt) {
        transaction { database ->
            val entry = loadOutbox(database, receipt.mutationId)
                ?: throw OutboxMutationNotFoundException(receipt.mutationId)
            if (entry.status != OutboxStatus.PENDING || !entry.sealed) {
                throw InvalidWorkspaceMutationException(
                    "A receipt may only complete a sealed pending mutation",
                )
            }
            val request = entry.mutation.request
            val validCommitSequence = when (receipt.effect) {
                MutationEffect.APPLIED -> request.baseSeq != Long.MAX_VALUE &&
                        receipt.commitSeq == request.baseSeq + 1L

                MutationEffect.NO_OP -> receipt.commitSeq == request.baseSeq
            }
            if (receipt.protocolVersion != request.protocolVersion ||
                receipt.datasetId != request.datasetId ||
                receipt.mutationId != request.mutationId ||
                receipt.clientId != request.clientId ||
                receipt.baseSeq != request.baseSeq ||
                !validCommitSequence
            ) {
                throw InvalidWorkspaceMutationException(
                    "Receipt does not match the durable request",
                )
            }
            val changed = database.update(
                TABLE_OUTBOX,
                ContentValues().apply {
                    put("state", OutboxStatus.AWAITING_CANONICAL.storedValue)
                    put("receipt_json", encode(receipt, MutationReceipt.serializer()))
                    put("receipt_commit_seq", receipt.commitSeq)
                    putNull("next_attempt_at_ms")
                    putNull("last_error")
                    putNull("blocked_error_json")
                },
                "mutation_id = ?",
                arrayOf(receipt.mutationId),
            )
            requireOutboxChange(changed, receipt.mutationId)
            val state = loadSyncState(database)
            retireConfirmed(database, state.datasetId, state.canonicalSeq)
        }
    }

    @Synchronized
    fun block(mutationId: String, error: ApiErrorBody): WorkspaceProjection =
        transaction { database ->
            val changed = database.update(
                TABLE_OUTBOX,
                ContentValues().apply {
                    put("state", OutboxStatus.BLOCKED.storedValue)
                    put("blocked_error_json", encode(error, ApiErrorBody.serializer()))
                    putNull("next_attempt_at_ms")
                    putNull("last_error")
                },
                "mutation_id = ? AND state = ?",
                arrayOf(mutationId, OutboxStatus.PENDING.storedValue),
            )
            requireOutboxChange(changed, mutationId)
            loadProjectionFrom(database)
        }

    @Synchronized
    fun retryBlocked(
        mutationId: String,
        replacementMutationId: String = UuidV7.newString(),
    ): WorkspaceProjection = transaction { database ->
        requireUuid(replacementMutationId, "replacement_mutation_id")
        val head = loadOldestOutbox(database)
            ?: throw OutboxMutationNotFoundException(mutationId)
        if (head.mutation.request.mutationId != mutationId || head.status != OutboxStatus.BLOCKED) {
            throw InvalidWorkspaceMutationException(
                "Only the blocked FIFO head may be retried",
            )
        }
        val state = loadSyncState(database)
        requireUnusedMutationId(database, replacementMutationId)
        val replacement = head.mutation.request.copy(
            mutationId = replacementMutationId,
            baseSeq = state.canonicalSeq,
        )
        val changed = database.update(
            TABLE_OUTBOX,
            ContentValues().apply {
                put("mutation_id", replacementMutationId)
                put(
                    "request_json",
                    encode(
                        replacement,
                        com.example.subroutine_simple.data.models.MutationRequest.serializer(),
                    ),
                )
                put("state", OutboxStatus.PENDING.storedValue)
                put("sealed", 0)
                put("attempt_count", 0)
                putNull("next_attempt_at_ms")
                putNull("last_error")
                putNull("receipt_json")
                putNull("receipt_commit_seq")
                putNull("blocked_error_json")
            },
            "mutation_id = ? AND state = ?",
            arrayOf(mutationId, OutboxStatus.BLOCKED.storedValue),
        )
        requireOutboxChange(changed, mutationId)
        remapReminderCommand(database, mutationId, replacementMutationId)
        loadProjectionFrom(database)
    }

    @Synchronized
    fun discardBlocked(mutationId: String): WorkspaceProjection = transaction { database ->
        val head = loadOldestOutbox(database)
            ?: throw OutboxMutationNotFoundException(mutationId)
        if (head.mutation.request.mutationId != mutationId || head.status != OutboxStatus.BLOCKED) {
            throw InvalidWorkspaceMutationException(
                "Only the blocked FIFO head may be discarded",
            )
        }
        requireOutboxChange(
            database.delete(
                TABLE_OUTBOX,
                "mutation_id = ? AND state = ?",
                arrayOf(mutationId, OutboxStatus.BLOCKED.storedValue),
            ),
            mutationId,
        )

        finishReminderCommand(database, mutationId, ReminderCommandStatus.SUPERSEDED)
        protectFollowingIntent(database)
        loadProjectionFrom(database)
    }


    private fun protectFollowingIntent(database: SQLiteDatabase) {
        val next = loadOldestOutbox(database)
        if (next?.status == OutboxStatus.PENDING) {
            val nextId = next.mutation.request.mutationId
            val state = loadSyncState(database)
            val safetyBlock = ApiErrorBody(
                error = ApiErrorCode.DOMAIN_CONFLICT,
                message = "An earlier local change was discarded; confirm this queued change before synchronizing it",
                mutationId = nextId,
                resource = firstPatchResource(next.mutation.optimisticPatch),
                currentSeq = state.canonicalSeq,
                currentDatasetId = state.datasetId,
                retryable = false,
            )
            database.update(
                TABLE_OUTBOX,
                ContentValues().apply {
                    put("state", OutboxStatus.BLOCKED.storedValue)
                    put("blocked_error_json", encode(safetyBlock, ApiErrorBody.serializer()))
                    putNull("next_attempt_at_ms")
                    putNull("last_error")
                },
                "mutation_id = ?",
                arrayOf(nextId),
            )
        }
    }

    @Synchronized
    fun pendingCount(): Int = outboxCount(database())

    @Synchronized
    fun blockedCount(): Int = scalarLong(
        database(),
        "SELECT COUNT(*) FROM $TABLE_OUTBOX WHERE state = ?",
        arrayOf(OutboxStatus.BLOCKED.storedValue),
    ).toInt()

    @Synchronized
    fun oneConflict(): BlockedConflict? = blockedConflict(database())

    @Synchronized
    fun diagnostics(): WorkspaceDiagnostics {
        val database = database()
        return WorkspaceDiagnostics(
            applicationId = pragmaLong(database, "application_id").toInt(),
            schemaVersion = pragmaLong(database, "user_version").toInt(),
            journalMode = pragmaString(database, "journal_mode"),
            foreignKeys = pragmaLong(database, "foreign_keys") != 0L,
            synchronousFull = pragmaLong(database, "synchronous") == SQLITE_SYNCHRONOUS_FULL,
            busyTimeoutMillis = pragmaLong(database, "busy_timeout"),
        )
    }

    @Synchronized
    override fun close() {
        helper.close()
        openedIdentity = null
    }

    private fun database(): SQLiteDatabase = helper.writableDatabase

    private inline fun <T> transaction(block: (SQLiteDatabase) -> T): T {
        val database = database()
        database.beginTransactionNonExclusive()
        return try {
            val result = block(database)
            database.setTransactionSuccessful()
            result
        } finally {
            database.endTransaction()
        }
    }

    private fun initializeIdentity(database: SQLiteDatabase): WorkspaceIdentity =
        transaction { transactionalDatabase ->
            val existing = transactionalDatabase.rawQuery(
                "SELECT workspace_kind, workspace_id, client_id, server_origin, account_id " +
                        "FROM $TABLE_WORKSPACE WHERE singleton = 1",
                null,
            ).use { cursor ->
                if (!cursor.moveToFirst()) {
                    null
                } else {
                    StoredIdentity(
                        kind = cursor.getString(0),
                        workspaceId = cursor.getString(1),
                        clientId = cursor.getString(2),
                        serverOrigin = cursor.getNullableString(3),
                        accountId = cursor.getNullableString(4),
                    )
                }
            }

            val identity = if (existing == null) {
                val workspaceId = UUID.randomUUID().toString()
                val clientId = expectedClientId?.also { requireUuid(it, "expectedClientId") }
                    ?: UUID.randomUUID().toString()
                val values = ContentValues().apply {
                    put("singleton", 1)
                    put("workspace_id", workspaceId)
                    put("client_id", clientId)
                    when (val spec = requestedWorkspace) {
                        NormalizedWorkspaceSpec.Local -> put("workspace_kind", "local")
                        is NormalizedWorkspaceSpec.Remote -> {
                            put("workspace_kind", "remote")
                            put("server_origin", spec.serverOrigin)
                            put("account_id", spec.accountId)
                        }
                    }
                }
                transactionalDatabase.insertOrThrow(TABLE_WORKSPACE, null, values)
                identityFrom(values)
            } else {
                identityFrom(existing)
            }
            verifyIdentity(identity)
            identity
        }

    private fun identityFrom(values: ContentValues): WorkspaceIdentity = when (values.getAsString("workspace_kind")) {
        "local" -> WorkspaceIdentity.Local(
            workspaceId = values.getAsString("workspace_id"),
            clientId = values.getAsString("client_id"),
        )

        "remote" -> WorkspaceIdentity.Remote(
            workspaceId = values.getAsString("workspace_id"),
            clientId = values.getAsString("client_id"),
            serverOrigin = values.getAsString("server_origin"),
            accountId = values.getAsString("account_id"),
        )

        else -> throw WorkspaceIdentityMismatchException("Stored workspace kind is invalid")
    }

    private fun identityFrom(stored: StoredIdentity): WorkspaceIdentity {
        requireUuid(stored.workspaceId, "workspace_id")
        requireUuid(stored.clientId, "client_id")
        return when {
            stored.kind == "local" && stored.serverOrigin == null && stored.accountId == null ->
                WorkspaceIdentity.Local(stored.workspaceId, stored.clientId)

            stored.kind == "remote" && stored.serverOrigin != null && stored.accountId != null -> {
                requireUuid(stored.accountId, "account_id")
                WorkspaceIdentity.Remote(
                    stored.workspaceId,
                    stored.clientId,
                    stored.serverOrigin,
                    stored.accountId,
                )
            }

            else -> throw WorkspaceIdentityMismatchException(
                "Stored workspace metadata is inconsistent",
            )
        }
    }

    private fun verifyIdentity(identity: WorkspaceIdentity) {
        val matches = when {
            identity is WorkspaceIdentity.Local && requestedWorkspace is NormalizedWorkspaceSpec.Local -> true
            identity is WorkspaceIdentity.Remote && requestedWorkspace is NormalizedWorkspaceSpec.Remote ->
                identity.serverOrigin == requestedWorkspace.serverOrigin &&
                        identity.accountId == requestedWorkspace.accountId

            else -> false
        }
        if (!matches) {
            throw WorkspaceIdentityMismatchException(
                "Stored workspace identity does not match the requested account and server origin",
            )
        }
        if (expectedClientId != null && identity.clientId != expectedClientId) {
            throw WorkspaceIdentityMismatchException(
                "Stored client_id does not match the expected client identity",
            )
        }
    }

    private fun loadSyncState(database: SQLiteDatabase): WorkspaceSyncState = database.rawQuery(
        "SELECT dataset_id, canonical_seq, authoritative_snapshot " +
                "FROM $TABLE_SYNC_STATE WHERE singleton = 1",
        null,
    ).use { cursor ->
        if (!cursor.moveToFirst()) {
            throw WorkspaceInvariantException("Workspace sync state is missing")
        }
        val datasetId = cursor.getNullableString(0)?.also { requireUuid(it, "dataset_id") }
        WorkspaceSyncState(
            datasetId = datasetId,
            canonicalSeq = cursor.getLong(1),
            authoritativeSnapshot = cursor.getInt(2) != 0,
        )
    }

    private fun loadProjectionFrom(database: SQLiteDatabase): WorkspaceProjection {
        val state = loadSyncState(database)
        var data = AllData(
            datasetId = state.datasetId ?: NIL_UUID,
            seq = state.canonicalSeq,
            actions = loadRows(database, KIND_ACTION, Action.serializer()),
            events = loadRows(database, KIND_EVENT, Event.serializer()),
            routines = loadRows(database, KIND_ROUTINE, Routine.serializer()),
            markers = loadRows(database, KIND_MARKER, Marker.serializer()),
            signals = loadRows(database, KIND_SIGNAL, Signal.serializer()),
            actionTemplates = loadRows(database, KIND_ACTION_TEMPLATE, ActionTemplate.serializer())
                .sortedBy { it.sortOrder },
            eventTemplates = loadRows(database, KIND_EVENT_TEMPLATE, EventTemplate.serializer())
                .sortedBy { it.sortOrder },
            markerTemplates = loadRows(database, KIND_MARKER_TEMPLATE, MarkerTemplate.serializer()),
            signalTemplates = loadRows(database, KIND_SIGNAL_TEMPLATE, SignalTemplate.serializer()),
        )
        database.rawQuery(
            "SELECT request_json, patch_json FROM $TABLE_OUTBOX ORDER BY position",
            null,
        ).use { cursor ->
            while (cursor.moveToNext()) {
                val request =
                    decode(cursor.getBlob(0), com.example.subroutine_simple.data.models.MutationRequest.serializer())
                val patch = decode(cursor.getBlob(1), OptimisticPatch.serializer())
                data = applyRemotePatch(data, patch, request.operation)
            }
        }
        return WorkspaceProjection(
            data = data,
            authoritativeSnapshot = state.authoritativeSnapshot,
            pendingCount = outboxCount(database),
            blockedCount = scalarLong(
                database,
                "SELECT COUNT(*) FROM $TABLE_OUTBOX WHERE state = ?",
                arrayOf(OutboxStatus.BLOCKED.storedValue),
            ).toInt(),
            conflict = blockedConflict(database),
        )
    }

    private fun blockedConflict(database: SQLiteDatabase): BlockedConflict? {
        val head = loadOldestOutbox(database) ?: return null
        if (head.status != OutboxStatus.BLOCKED) return null
        val error = head.blockedError
            ?: throw WorkspaceInvariantException("Blocked outbox entry has no conflict details")
        return BlockedConflict(
            mutationId = head.mutation.request.mutationId,
            operation = head.mutation.request.operation,
            error = error,
        )
    }

    private fun <T> loadRows(
        database: SQLiteDatabase,
        kind: String,
        serializer: KSerializer<T>,
    ): List<T> = database.rawQuery(
        "SELECT value_version, value_json FROM $TABLE_CANONICAL " +
                "WHERE kind = ? ORDER BY ordinal, resource_id",
        arrayOf(kind),
    ).use { cursor ->
        buildList {
            while (cursor.moveToNext()) {
                val version = cursor.getLong(0)
                if (version != RESOURCE_VALUE_VERSION) {
                    throw WorkspaceInvariantException(
                        "Unsupported $kind value version $version",
                    )
                }
                add(decode(cursor.getBlob(1), serializer))
            }
        }
    }

    private fun insertCanonical(
        database: SQLiteDatabase,
        payload: ResourcePayload,
        ordinal: Long,
    ) {
        database.insertOrThrow(
            TABLE_CANONICAL,
            null,
            ContentValues().apply {
                put("kind", payload.kind)
                put("resource_id", payload.id)
                put("value_version", RESOURCE_VALUE_VERSION)
                put("value_json", payload.json)
                put("ordinal", ordinal)
            },
        )
    }

    private fun upsertCanonical(database: SQLiteDatabase, payload: ResourcePayload) {
        val existingOrdinal = database.rawQuery(
            "SELECT ordinal FROM $TABLE_CANONICAL WHERE kind = ? AND resource_id = ?",
            arrayOf(payload.kind, payload.id),
        ).use { cursor -> if (cursor.moveToFirst()) cursor.getLong(0) else null }
        if (existingOrdinal == null) {
            val ordinal = scalarLong(
                database,
                "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM $TABLE_CANONICAL WHERE kind = ?",
                arrayOf(payload.kind),
            )
            insertCanonical(database, payload, ordinal)
        } else {
            database.update(
                TABLE_CANONICAL,
                ContentValues().apply {
                    put("value_version", RESOURCE_VALUE_VERSION)
                    put("value_json", payload.json)
                },
                "kind = ? AND resource_id = ?",
                arrayOf(payload.kind, payload.id),
            )
        }
    }

    private fun loadReminderCommand(database: SQLiteDatabase, commandId: String): StoredReminderCommand? =
        database.rawQuery(
            "SELECT c.fingerprint, c.status, o.state FROM $TABLE_REMINDER_COMMANDS c " +
                    "LEFT JOIN $TABLE_OUTBOX o ON o.mutation_id = c.mutation_id WHERE c.command_id = ?",
            arrayOf(commandId),
        ).use { cursor ->
            if (!cursor.moveToFirst()) return@use null
            val stored = ReminderCommandStatus.entries.firstOrNull { it.name == cursor.getString(1) }
                ?: throw WorkspaceInvariantException("Unknown reminder command status")
            val status = if (stored == ReminderCommandStatus.QUEUED) {
                val outboxState = cursor.getNullableString(2)
                    ?: throw WorkspaceInvariantException("Queued reminder command has no outbox entry")
                if (OutboxStatus.fromStored(outboxState) == OutboxStatus.BLOCKED) {
                    ReminderCommandStatus.CONFLICT
                } else {
                    ReminderCommandStatus.QUEUED
                }
            } else {
                stored
            }
            StoredReminderCommand(cursor.getBlob(0), status)
        }


    private fun mutationIdReserved(database: SQLiteDatabase, mutationId: String): Boolean =
        scalarLong(
            database,
            "SELECT (SELECT COUNT(*) FROM $TABLE_OUTBOX WHERE mutation_id = ?) + " +
                    "(SELECT COUNT(*) FROM $TABLE_REMINDER_COMMANDS WHERE command_id = ? OR mutation_id = ?)",
            arrayOf(mutationId, mutationId, mutationId),
        ) > 0L

    private fun requireUnusedMutationId(database: SQLiteDatabase, mutationId: String) {
        if (mutationIdReserved(database, mutationId)) {
            throw InvalidWorkspaceMutationException("Mutation $mutationId is already owned by durable intent")
        }
    }

    private fun remapReminderCommand(database: SQLiteDatabase, mutationId: String, replacementId: String) {
        database.update(
            TABLE_REMINDER_COMMANDS,
            ContentValues().apply { put("mutation_id", replacementId) },
            "mutation_id = ? AND status = ?",
            arrayOf(mutationId, ReminderCommandStatus.QUEUED.name),
        )
    }

    private fun finishReminderCommand(database: SQLiteDatabase, mutationId: String, status: ReminderCommandStatus) {
        database.update(
            TABLE_REMINDER_COMMANDS,
            ContentValues().apply { put("status", status.name) },
            "mutation_id = ? AND status = ?",
            arrayOf(mutationId, ReminderCommandStatus.QUEUED.name),
        )
    }

    private fun loadOldestOutbox(database: SQLiteDatabase): OutboxEntry? =
        loadOutboxQuery(
            database,
            "SELECT position, request_json, patch_json, state, sealed, attempt_count, " +
                    "next_attempt_at_ms, last_error, receipt_json, blocked_error_json " +
                    "FROM $TABLE_OUTBOX ORDER BY position LIMIT 1",
            null,
        )

    private fun loadOutbox(database: SQLiteDatabase, mutationId: String): OutboxEntry? =
        loadOutboxQuery(
            database,
            "SELECT position, request_json, patch_json, state, sealed, attempt_count, " +
                    "next_attempt_at_ms, last_error, receipt_json, blocked_error_json " +
                    "FROM $TABLE_OUTBOX WHERE mutation_id = ?",
            arrayOf(mutationId),
        )

    private fun loadOutboxQuery(
        database: SQLiteDatabase,
        sql: String,
        arguments: Array<String>?,
    ): OutboxEntry? = database.rawQuery(sql, arguments).use { cursor ->
        if (!cursor.moveToFirst()) return@use null
        val request = decode(
            cursor.getBlob(1),
            com.example.subroutine_simple.data.models.MutationRequest.serializer(),
        )
        val patch = decode(cursor.getBlob(2), OptimisticPatch.serializer())
        OutboxEntry(
            position = cursor.getLong(0),
            mutation = ClientMutation(request, patch),
            status = OutboxStatus.fromStored(cursor.getString(3)),
            sealed = cursor.getInt(4) != 0,
            attemptCount = cursor.getInt(5),
            nextAttemptAtMillis = cursor.getNullableLong(6),
            lastError = cursor.getNullableString(7),
            receipt = cursor.getNullableBlob(8)?.let { decode(it, MutationReceipt.serializer()) },
            blockedError = cursor.getNullableBlob(9)?.let { decode(it, ApiErrorBody.serializer()) },
        )
    }

    private fun markAttemptIn(
        database: SQLiteDatabase,
        mutationId: String,
        requireHead: Boolean,
    ) {
        if (requireHead) {
            val head = loadOldestOutbox(database)
                ?: throw OutboxMutationNotFoundException(mutationId)
            if (head.mutation.request.mutationId != mutationId ||
                head.status != OutboxStatus.PENDING
            ) {
                throw InvalidWorkspaceMutationException(
                    "Only the pending FIFO head may be attempted",
                )
            }
        }
        database.execSQL(
            "UPDATE $TABLE_OUTBOX SET sealed = 1, attempt_count = attempt_count + 1, " +
                    "last_error = NULL WHERE mutation_id = ? AND state = ?",
            arrayOf(mutationId, OutboxStatus.PENDING.storedValue),
        )
        val entry = loadOutbox(database, mutationId)
        if (entry == null || entry.status != OutboxStatus.PENDING) {
            throw OutboxMutationNotFoundException(mutationId)
        }
    }

    private fun retireConfirmed(
        database: SQLiteDatabase,
        datasetId: String?,
        canonicalSeq: Long,
    ) {
        if (datasetId == null) return
        val confirmedHeadCommit = loadOldestOutbox(database)
            ?.takeIf { entry ->
                entry.status == OutboxStatus.AWAITING_CANONICAL &&
                        entry.receipt?.datasetId == datasetId &&
                        (entry.receipt.commitSeq <= canonicalSeq)
            }
            ?.receipt
            ?.commitSeq

        val retiredIds = database.rawQuery(
            "SELECT mutation_id, receipt_json FROM $TABLE_OUTBOX " +
                    "WHERE state = ? AND receipt_commit_seq <= ?",
            arrayOf(OutboxStatus.AWAITING_CANONICAL.storedValue, canonicalSeq.toString()),
        ).use { cursor ->
            buildList {
                while (cursor.moveToNext()) {
                    val receipt = decode(cursor.getBlob(1), MutationReceipt.serializer())
                    if (receipt.datasetId == datasetId) add(cursor.getString(0))
                }
            }
        }
        retiredIds.forEach { mutationId ->
            finishReminderCommand(database, mutationId, ReminderCommandStatus.CONFIRMED)
            database.delete(TABLE_OUTBOX, "mutation_id = ?", arrayOf(mutationId))
        }

        if (confirmedHeadCommit != null) {
            val next = loadOldestOutbox(database)
            if (next?.status == OutboxStatus.PENDING && !next.sealed &&
                next.mutation.request.baseSeq != confirmedHeadCommit
            ) {
                val rebased = next.mutation.request.copy(baseSeq = confirmedHeadCommit)
                database.update(
                    TABLE_OUTBOX,
                    ContentValues().apply {
                        put(
                            "request_json",
                            encode(
                                rebased,
                                com.example.subroutine_simple.data.models.MutationRequest.serializer(),
                            ),
                        )
                    },
                    "mutation_id = ?",
                    arrayOf(rebased.mutationId),
                )
            }
        }
    }

    private fun applyRemotePatch(
        data: AllData,
        patch: OptimisticPatch,
        operation: MutationOperation,
    ): AllData = applyPatch(
        data,
        OptimisticPatch(
            writes = patch.writes.filter { operationProjectsWrite(operation, resourceValueKey(it)) },
            deletes = patch.deletes.filter { operationProjectsDelete(operation, it) },
            routineOrder = patch.routineOrder.takeIf {
                operation is MutationOperation.ReorderRoutines
            },
        ),
    )

    private fun applyPatch(data: AllData, patch: OptimisticPatch): AllData {
        val actions = data.actions.toMutableList()
        val events = data.events.toMutableList()
        val routines = data.routines.toMutableList()
        val markers = data.markers.toMutableList()
        val signals = data.signals.toMutableList()
        val actionTemplates = data.actionTemplates.toMutableList()
        val eventTemplates = data.eventTemplates.toMutableList()
        val markerTemplates = data.markerTemplates.toMutableList()
        val signalTemplates = data.signalTemplates.toMutableList()

        patch.writes.forEach { value ->
            when (value) {
                is ResourceValue.Action -> actions.upsert(value.value) { it.id }
                is ResourceValue.Event -> events.upsert(value.value) { it.id }
                is ResourceValue.Routine -> routines.upsert(value.value) { it.id }
                is ResourceValue.Marker -> markers.upsert(value.value) { it.id }
                is ResourceValue.Signal -> signals.upsert(value.value) { it.id }
                is ResourceValue.ActionTemplate -> actionTemplates.upsert(value.value) { it.id }
                is ResourceValue.EventTemplate -> eventTemplates.upsert(value.value) { it.id }
                is ResourceValue.MarkerTemplate -> markerTemplates.upsert(value.value) { it.id }
                is ResourceValue.SignalTemplate -> signalTemplates.upsert(value.value) { it.id }
            }
        }
        patch.deletes.forEach { key ->
            when (key) {
                is ResourceKey.Action -> actions.removeAll { it.id == key.id }
                is ResourceKey.Event -> events.removeAll { it.id == key.id }
                is ResourceKey.Routine -> routines.removeAll { it.id == key.id }
                is ResourceKey.Marker -> markers.removeAll { it.id == key.id }
                is ResourceKey.Signal -> signals.removeAll { it.id == key.id }
                is ResourceKey.ActionTemplate -> actionTemplates.removeAll { it.id == key.id }
                is ResourceKey.EventTemplate -> eventTemplates.removeAll { it.id == key.id }
                is ResourceKey.MarkerTemplate -> markerTemplates.removeAll { it.id == key.id }
                is ResourceKey.SignalTemplate -> signalTemplates.removeAll { it.id == key.id }
            }
        }
        patch.routineOrder?.let { order ->
            if (order.size != routines.size || order.toSet().size != order.size ||
                order.toSet() != routines.map { it.id }.toSet()
            ) {
                throw InvalidWorkspaceMutationException(
                    "Routine order must contain every resulting routine exactly once",
                )
            }
            val byId = routines.associateBy { it.id }
            routines.clear()
            routines.addAll(order.map { id -> byId.getValue(id) })
        }

        return data.copy(
            actions = actions,
            events = events,
            routines = routines,
            markers = markers,
            signals = signals,
            actionTemplates = actionTemplates.sortedBy { it.sortOrder },
            eventTemplates = eventTemplates.sortedBy { it.sortOrder },
            markerTemplates = markerTemplates,
            signalTemplates = signalTemplates,
        )
    }

    private fun operationProjectsWrite(operation: MutationOperation, key: TypedKey): Boolean =
        when (operation) {
            is MutationOperation.UpsertAction -> key == TypedKey(KIND_ACTION, operation.action.id)
            is MutationOperation.UpsertActions -> operation.actions.any {
                key == TypedKey(KIND_ACTION, it.id)
            }

            is MutationOperation.UpsertResources -> operation.resources.any {
                resourceValueKey(it) == key
            }

            is MutationOperation.CompleteAction -> true
            is MutationOperation.ConvertEventToMarker ->
                key == TypedKey(KIND_MARKER, operation.markerId)

            is MutationOperation.DeleteResources,
            is MutationOperation.ReorderRoutines,
            is MutationOperation.DeleteAction -> false
        }

    private fun operationProjectsDelete(operation: MutationOperation, key: ResourceKey): Boolean =
        when (operation) {
            is MutationOperation.DeleteResources -> operation.resources.contains(key)
            is MutationOperation.DeleteAction ->
                key == ResourceKey.Action(operation.actionId)

            is MutationOperation.ConvertEventToMarker ->
                key == ResourceKey.Event(operation.eventId)

            is MutationOperation.UpsertAction,
            is MutationOperation.UpsertActions,
            is MutationOperation.UpsertResources,
            is MutationOperation.ReorderRoutines,
            is MutationOperation.CompleteAction -> false
        }

    private fun firstPatchResource(patch: OptimisticPatch): ResourceKey? =
        patch.writes.firstOrNull()?.let(::resourceValueAsKey)
            ?: patch.deletes.firstOrNull()
            ?: patch.routineOrder?.firstOrNull()?.let(ResourceKey::Routine)

    private fun firstPatchDeltaConflict(
        patch: OptimisticPatch,
        delta: DataDelta,
    ): ResourceKey? {
        if (patch.routineOrder != null &&
            (delta.routines.isNotEmpty() ||
                    delta.tombstones.routines.isNotEmpty() ||
                    delta.routineOrder.isNotEmpty())
        ) {
            val id = delta.routines.firstOrNull()?.id
                ?: delta.tombstones.routines.firstOrNull()
                ?: delta.routineOrder.firstOrNull()
                ?: patch.routineOrder.firstOrNull()
            if (id != null) return ResourceKey.Routine(id)
        }
        return patch.writes.asSequence()
            .map(::resourceValueAsKey)
            .plus(patch.deletes.asSequence())
            .firstOrNull { deltaTouches(delta, it) }
    }

    private fun deltaTouches(delta: DataDelta, key: ResourceKey): Boolean = when (key) {
        is ResourceKey.Action -> delta.actions.any { it.id == key.id } || key.id in delta.tombstones.actions
        is ResourceKey.Event -> delta.events.any { it.id == key.id } || key.id in delta.tombstones.events
        is ResourceKey.Routine -> delta.routines.any { it.id == key.id } || key.id in delta.tombstones.routines
        is ResourceKey.Marker -> delta.markers.any { it.id == key.id } || key.id in delta.tombstones.markers
        is ResourceKey.Signal -> delta.signals.any { it.id == key.id } || key.id in delta.tombstones.signals
        is ResourceKey.ActionTemplate ->
            delta.actionTemplates.any { it.id == key.id } || key.id in delta.tombstones.actionTemplates

        is ResourceKey.EventTemplate ->
            delta.eventTemplates.any { it.id == key.id } || key.id in delta.tombstones.eventTemplates

        is ResourceKey.MarkerTemplate ->
            delta.markerTemplates.any { it.id == key.id } || key.id in delta.tombstones.markerTemplates

        is ResourceKey.SignalTemplate ->
            delta.signalTemplates.any { it.id == key.id } || key.id in delta.tombstones.signalTemplates
    }

    private fun resourceValueKey(value: ResourceValue): TypedKey = when (value) {
        is ResourceValue.Action -> TypedKey(KIND_ACTION, value.value.id)
        is ResourceValue.Event -> TypedKey(KIND_EVENT, value.value.id)
        is ResourceValue.Routine -> TypedKey(KIND_ROUTINE, value.value.id)
        is ResourceValue.Marker -> TypedKey(KIND_MARKER, value.value.id)
        is ResourceValue.Signal -> TypedKey(KIND_SIGNAL, value.value.id)
        is ResourceValue.ActionTemplate -> TypedKey(KIND_ACTION_TEMPLATE, value.value.id)
        is ResourceValue.EventTemplate -> TypedKey(KIND_EVENT_TEMPLATE, value.value.id)
        is ResourceValue.MarkerTemplate -> TypedKey(KIND_MARKER_TEMPLATE, value.value.id)
        is ResourceValue.SignalTemplate -> TypedKey(KIND_SIGNAL_TEMPLATE, value.value.id)
    }

    private fun resourceValueAsKey(value: ResourceValue): ResourceKey = when (value) {
        is ResourceValue.Action -> ResourceKey.Action(value.value.id)
        is ResourceValue.Event -> ResourceKey.Event(value.value.id)
        is ResourceValue.Routine -> ResourceKey.Routine(value.value.id)
        is ResourceValue.Marker -> ResourceKey.Marker(value.value.id)
        is ResourceValue.Signal -> ResourceKey.Signal(value.value.id)
        is ResourceValue.ActionTemplate -> ResourceKey.ActionTemplate(value.value.id)
        is ResourceValue.EventTemplate -> ResourceKey.EventTemplate(value.value.id)
        is ResourceValue.MarkerTemplate -> ResourceKey.MarkerTemplate(value.value.id)
        is ResourceValue.SignalTemplate -> ResourceKey.SignalTemplate(value.value.id)
    }

    private fun snapshotPayloads(data: AllData): List<ResourcePayload> = buildList {
        data.actions.forEach { add(payload(KIND_ACTION, it.id, it, Action.serializer())) }
        data.events.forEach { add(payload(KIND_EVENT, it.id, it, Event.serializer())) }
        data.routines.forEach { add(payload(KIND_ROUTINE, it.id, it, Routine.serializer())) }
        data.markers.forEach { add(payload(KIND_MARKER, it.id, it, Marker.serializer())) }
        data.signals.forEach { add(payload(KIND_SIGNAL, it.id, it, Signal.serializer())) }
        data.actionTemplates.forEach {
            add(payload(KIND_ACTION_TEMPLATE, it.id, it, ActionTemplate.serializer()))
        }
        data.eventTemplates.forEach {
            add(payload(KIND_EVENT_TEMPLATE, it.id, it, EventTemplate.serializer()))
        }
        data.markerTemplates.forEach {
            add(payload(KIND_MARKER_TEMPLATE, it.id, it, MarkerTemplate.serializer()))
        }
        data.signalTemplates.forEach {
            add(payload(KIND_SIGNAL_TEMPLATE, it.id, it, SignalTemplate.serializer()))
        }
    }.withPerKindOrdinals()

    private fun deltaPayloads(delta: DataDelta): List<ResourcePayload> = buildList {
        delta.actions.forEach { add(payload(KIND_ACTION, it.id, it, Action.serializer())) }
        delta.events.forEach { add(payload(KIND_EVENT, it.id, it, Event.serializer())) }
        delta.routines.forEach { add(payload(KIND_ROUTINE, it.id, it, Routine.serializer())) }
        delta.markers.forEach { add(payload(KIND_MARKER, it.id, it, Marker.serializer())) }
        delta.signals.forEach { add(payload(KIND_SIGNAL, it.id, it, Signal.serializer())) }
        delta.actionTemplates.forEach {
            add(payload(KIND_ACTION_TEMPLATE, it.id, it, ActionTemplate.serializer()))
        }
        delta.eventTemplates.forEach {
            add(payload(KIND_EVENT_TEMPLATE, it.id, it, EventTemplate.serializer()))
        }
        delta.markerTemplates.forEach {
            add(payload(KIND_MARKER_TEMPLATE, it.id, it, MarkerTemplate.serializer()))
        }
        delta.signalTemplates.forEach {
            add(payload(KIND_SIGNAL_TEMPLATE, it.id, it, SignalTemplate.serializer()))
        }
    }

    private fun List<ResourcePayload>.withPerKindOrdinals(): List<ResourcePayload> {
        val next = mutableMapOf<String, Long>()
        return map { payload ->
            payload.copy(snapshotOrdinal = next.getOrDefault(payload.kind, 0L).also {
                next[payload.kind] = it + 1L
            })
        }
    }

    private fun tombstoneKeys(delta: DataDelta): List<TypedKey> = buildList {
        delta.tombstones.actions.forEach { add(TypedKey(KIND_ACTION, it)) }
        delta.tombstones.events.forEach { add(TypedKey(KIND_EVENT, it)) }
        delta.tombstones.routines.forEach { add(TypedKey(KIND_ROUTINE, it)) }
        delta.tombstones.markers.forEach { add(TypedKey(KIND_MARKER, it)) }
        delta.tombstones.signals.forEach { add(TypedKey(KIND_SIGNAL, it)) }
        delta.tombstones.actionTemplates.forEach { add(TypedKey(KIND_ACTION_TEMPLATE, it)) }
        delta.tombstones.eventTemplates.forEach { add(TypedKey(KIND_EVENT_TEMPLATE, it)) }
        delta.tombstones.markerTemplates.forEach { add(TypedKey(KIND_MARKER_TEMPLATE, it)) }
        delta.tombstones.signalTemplates.forEach { add(TypedKey(KIND_SIGNAL_TEMPLATE, it)) }
    }

    private fun <T> payload(
        kind: String,
        id: String,
        value: T,
        serializer: KSerializer<T>,
    ): ResourcePayload = ResourcePayload(kind, id, encode(value, serializer))

    private fun <T> encode(value: T, serializer: KSerializer<T>): ByteArray =
        json.encodeToString(serializer, value).toByteArray(StandardCharsets.UTF_8)

    private fun <T> decode(bytes: ByteArray, serializer: KSerializer<T>): T = try {
        json.decodeFromString(serializer, bytes.toString(StandardCharsets.UTF_8))
    } catch (error: SerializationException) {
        throw WorkspaceInvariantException("Stored workspace JSON is invalid", error)
    } catch (error: IllegalArgumentException) {
        throw WorkspaceInvariantException("Stored workspace JSON is invalid", error)
    }

    private fun outboxCount(database: SQLiteDatabase): Int = scalarLong(
        database,
        "SELECT COUNT(*) FROM $TABLE_OUTBOX",
        null,
    ).toInt()

    private fun requireOutboxChange(changed: Int, mutationId: String) {
        if (changed == 0) throw OutboxMutationNotFoundException(mutationId)
    }

    private fun configureRuntime(database: SQLiteDatabase) {
        database.setForeignKeyConstraintsEnabled(true)
        database.execSQL("PRAGMA synchronous = FULL")
        applyQueryPragma(database, "busy_timeout = $BUSY_TIMEOUT_MILLIS")
    }

    private fun verifyRuntimeConfiguration(database: SQLiteDatabase) {
        val journalMode = pragmaString(database, "journal_mode")
        val foreignKeys = pragmaLong(database, "foreign_keys")
        val synchronous = pragmaLong(database, "synchronous")
        val busyTimeout = pragmaLong(database, "busy_timeout")
        if (!journalMode.equals("wal", ignoreCase = true) || foreignKeys == 0L ||
            synchronous != SQLITE_SYNCHRONOUS_FULL || busyTimeout < BUSY_TIMEOUT_MILLIS
        ) {
            throw WorkspaceOpenException(
                "SQLite durability configuration was not accepted " +
                        "(journal=$journalMode, foreign_keys=$foreignKeys, " +
                        "synchronous=$synchronous, busy_timeout=$busyTimeout)",
            )
        }
    }

    private fun runQuickCheck(database: SQLiteDatabase) {
        val result = pragmaString(database, "quick_check(1)")
        if (result != "ok") {
            throw CorruptWorkspaceDatabaseException(
                "SQLite quick_check failed ($result); the database was left in place for recovery",
            )
        }
    }

    private fun requireUuid(value: String, field: String): UUID = try {
        UUID.fromString(value)
    } catch (error: IllegalArgumentException) {
        throw WorkspaceInvariantException("Stored or supplied $field is not a UUID", error)
    }

    companion object {
        const val APPLICATION_ID: Int = 0x53554252


        const val SCHEMA_VERSION: Int = 2
        const val RESOURCE_VALUE_VERSION: Long = 1L
        const val BUSY_TIMEOUT_MILLIS: Long = 5_000L
        private const val SQLITE_SYNCHRONOUS_FULL = 2L
        private const val NIL_UUID = "00000000-0000-0000-0000-000000000000"

        private const val TABLE_WORKSPACE = "workspace"
        private const val TABLE_SYNC_STATE = "sync_state"
        private const val TABLE_CANONICAL = "canonical_resources"
        private const val TABLE_OUTBOX = "outbox"
        private const val TABLE_REMINDER_COMMANDS = "reminder_commands"

        private const val KIND_ACTION = "action"
        private const val KIND_EVENT = "event"
        private const val KIND_ROUTINE = "routine"
        private const val KIND_MARKER = "marker"
        private const val KIND_SIGNAL = "signal"
        private const val KIND_ACTION_TEMPLATE = "action_template"
        private const val KIND_EVENT_TEMPLATE = "event_template"
        private const val KIND_MARKER_TEMPLATE = "marker_template"
        private const val KIND_SIGNAL_TEMPLATE = "signal_template"

        @JvmStatic
        fun open(
            context: Context,
            workspace: WorkspaceSpec,
            expectedClientId: String? = null,
            databaseNamePrefix: String = "",
        ): SQLiteWorkspaceStore {
            require(databaseNamePrefix.matches(Regex("[A-Za-z0-9_.-]*"))) {
                "databaseNamePrefix contains unsupported characters"
            }
            val normalized = normalize(workspace)
            val store = SQLiteWorkspaceStore(
                context = context,
                requestedWorkspace = normalized,
                databaseName = databaseNamePrefix + databaseNameFor(normalized),
                expectedClientId = expectedClientId,
            )
            store.initialize()
            return store
        }

        @JvmStatic
        fun databaseNameFor(workspace: WorkspaceSpec): String = databaseNameFor(normalize(workspace))

        private fun databaseNameFor(workspace: NormalizedWorkspaceSpec): String = when (workspace) {
            NormalizedWorkspaceSpec.Local -> "subroutine-local.sqlite3"
            is NormalizedWorkspaceSpec.Remote -> {
                val digest = MessageDigest.getInstance("SHA-256")
                digest.update(workspace.serverOrigin.toByteArray(StandardCharsets.UTF_8))
                digest.update(0)
                val account = UUID.fromString(workspace.accountId)
                digest.update(
                    ByteBuffer.allocate(16)
                        .putLong(account.mostSignificantBits)
                        .putLong(account.leastSignificantBits)
                        .array(),
                )
                val hash = digest.digest().joinToString("") { byte ->
                    "%02x".format(Locale.ROOT, byte.toInt() and 0xff)
                }
                "subroutine-remote-$hash.sqlite3"
            }
        }

        private fun normalize(workspace: WorkspaceSpec): NormalizedWorkspaceSpec = when (workspace) {
            WorkspaceSpec.Local -> NormalizedWorkspaceSpec.Local
            is WorkspaceSpec.Remote -> {
                val accountId = try {
                    UUID.fromString(workspace.accountId)
                } catch (error: IllegalArgumentException) {
                    throw InvalidWorkspaceMutationException("Remote account identity must be a UUID")
                }
                if (accountId == UUID(0L, 0L)) {
                    throw InvalidWorkspaceMutationException(
                        "Remote account identity must not be the nil UUID",
                    )
                }
                NormalizedWorkspaceSpec.Remote(
                    serverOrigin = normalizeServerOrigin(workspace.serverOrigin),
                    accountId = accountId.toString(),
                )
            }
        }

        @JvmStatic
        fun normalizeServerOrigin(value: String): String {
            val uri = try {
                URI(value.trim())
            } catch (error: Exception) {
                throw InvalidWorkspaceMutationException("Invalid server origin: ${error.message}")
            }
            val scheme = uri.scheme?.lowercase(Locale.ROOT)
            if ((scheme != "http" && scheme != "https") || uri.isOpaque || uri.host == null) {
                throw InvalidWorkspaceMutationException(
                    "Server origin must be an absolute HTTP or HTTPS origin",
                )
            }
            if (uri.rawUserInfo != null || uri.rawQuery != null || uri.rawFragment != null ||
                (!uri.rawPath.isNullOrEmpty() && uri.rawPath != "/")
            ) {
                throw InvalidWorkspaceMutationException(
                    "Server origin must not contain credentials, a path, a query, or a fragment",
                )
            }
            val rawHost = uri.host.lowercase(Locale.ROOT)
            val host = if (rawHost.contains(':')) "[$rawHost]" else IDN.toASCII(rawHost)
            val defaultPort = (scheme == "http" && uri.port == 80) ||
                    (scheme == "https" && uri.port == 443)
            val port = if (uri.port == -1 || defaultPort) "" else ":${uri.port}"
            return "$scheme://$host$port"
        }

        private fun nonNilUuid(value: String): String? {
            val uuid = try {
                UUID.fromString(value)
            } catch (error: IllegalArgumentException) {
                throw InvalidWorkspaceMutationException("Dataset identity must be a UUID")
            }
            return uuid.takeUnless { it == UUID(0L, 0L) }?.toString()
        }

        private fun scalarLong(
            database: SQLiteDatabase,
            sql: String,
            arguments: Array<String>?,
        ): Long = database.rawQuery(sql, arguments).use { cursor ->
            if (!cursor.moveToFirst()) throw WorkspaceInvariantException("SQLite query returned no row")
            cursor.getLong(0)
        }

        private fun applyQueryPragma(database: SQLiteDatabase, pragma: String) {
            database.rawQuery("PRAGMA $pragma", null).use { cursor -> cursor.moveToFirst() }
        }

        private fun pragmaLong(database: SQLiteDatabase, pragma: String): Long =
            scalarLong(database, "PRAGMA $pragma", null)

        private fun pragmaString(database: SQLiteDatabase, pragma: String): String =
            database.rawQuery("PRAGMA $pragma", null).use { cursor ->
                if (!cursor.moveToFirst()) {
                    throw WorkspaceInvariantException("PRAGMA $pragma returned no row")
                }
                cursor.getString(0)
            }
    }

    private class WorkspaceOpenHelper(context: Context, name: String) : SQLiteOpenHelper(
        context,
        name,
        null,
        SCHEMA_VERSION,
        DatabaseErrorHandler { database ->
            throw SQLiteDatabaseCorruptException(
                "Corrupt workspace database at ${database.path}; automatic deletion is disabled",
            )
        },
    ) {
        init {
            setWriteAheadLoggingEnabled(true)
        }

        override fun onConfigure(database: SQLiteDatabase) {
            super.onConfigure(database)
            database.setForeignKeyConstraintsEnabled(true)
            database.execSQL("PRAGMA synchronous = FULL")
            applyQueryPragma(database, "busy_timeout = $BUSY_TIMEOUT_MILLIS")

            val applicationId = pragmaLong(database, "application_id").toInt()
            if (applicationId == APPLICATION_ID) return
            if (applicationId != 0) {
                throw ForeignWorkspaceDatabaseException(
                    "Unexpected SQLite application id 0x${applicationId.toString(16)}; " +
                            "expected 0x${APPLICATION_ID.toString(16)}",
                )
            }
            val userTables = scalarLong(
                database,
                "SELECT COUNT(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' " +
                        "AND name != 'android_metadata'",
                null,
            )
            if (userTables != 0L || database.version != 0) {
                throw ForeignWorkspaceDatabaseException(
                    "Refusing to claim a non-empty SQLite database without Subroutine Lite's application id",
                )
            }
        }

        override fun onCreate(database: SQLiteDatabase) {
            database.execSQL(
                "CREATE TABLE $TABLE_WORKSPACE (" +
                        "singleton INTEGER PRIMARY KEY CHECK (singleton = 1)," +
                        "workspace_kind TEXT NOT NULL CHECK (workspace_kind IN ('local', 'remote'))," +
                        "workspace_id TEXT NOT NULL," +
                        "client_id TEXT NOT NULL," +
                        "server_origin TEXT," +
                        "account_id TEXT," +
                        "CHECK ((workspace_kind = 'local' AND server_origin IS NULL AND account_id IS NULL) " +
                        "OR (workspace_kind = 'remote' AND server_origin IS NOT NULL AND account_id IS NOT NULL))" +
                        ")",
            )
            database.execSQL(
                "CREATE TABLE $TABLE_SYNC_STATE (" +
                        "singleton INTEGER PRIMARY KEY CHECK (singleton = 1)," +
                        "dataset_id TEXT," +
                        "canonical_seq INTEGER NOT NULL DEFAULT 0 CHECK (canonical_seq >= 0)," +
                        "authoritative_snapshot INTEGER NOT NULL DEFAULT 0 " +
                        "CHECK (authoritative_snapshot IN (0, 1))" +
                        ")",
            )
            database.execSQL("INSERT INTO $TABLE_SYNC_STATE (singleton) VALUES (1)")
            database.execSQL(
                "CREATE TABLE $TABLE_CANONICAL (" +
                        "kind TEXT NOT NULL," +
                        "resource_id TEXT NOT NULL," +
                        "value_version INTEGER NOT NULL," +
                        "value_json BLOB NOT NULL," +
                        "ordinal INTEGER NOT NULL CHECK (ordinal >= 0)," +
                        "PRIMARY KEY (kind, resource_id)" +
                        ")",
            )
            database.execSQL(
                "CREATE INDEX canonical_resources_order ON $TABLE_CANONICAL" +
                        "(kind, ordinal, resource_id)",
            )
            database.execSQL(
                "CREATE TABLE $TABLE_OUTBOX (" +
                        "position INTEGER PRIMARY KEY AUTOINCREMENT," +
                        "mutation_id TEXT NOT NULL UNIQUE," +
                        "request_json BLOB NOT NULL," +
                        "patch_json BLOB NOT NULL," +
                        "state TEXT NOT NULL DEFAULT 'pending' " +
                        "CHECK (state IN ('pending', 'awaiting_canonical', 'blocked'))," +
                        "sealed INTEGER NOT NULL DEFAULT 0 CHECK (sealed IN (0, 1))," +
                        "attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0)," +
                        "next_attempt_at_ms INTEGER," +
                        "last_error TEXT," +
                        "receipt_json BLOB," +
                        "receipt_commit_seq INTEGER," +
                        "blocked_error_json BLOB," +
                        "created_at_ms INTEGER NOT NULL" +
                        ")",
            )
            database.execSQL(
                "CREATE INDEX outbox_state_position ON $TABLE_OUTBOX(state, position)",
            )
            createReminderCommands(database)
            database.execSQL("PRAGMA application_id = $APPLICATION_ID")
        }

        private fun createReminderCommands(database: SQLiteDatabase) {
            database.execSQL(
                "CREATE TABLE $TABLE_REMINDER_COMMANDS (" +
                        "command_id TEXT PRIMARY KEY NOT NULL," +
                        "fingerprint BLOB NOT NULL," +
                        "mutation_id TEXT UNIQUE," +
                        "status TEXT NOT NULL CHECK (status IN ('QUEUED', 'CONFIRMED', 'SUPERSEDED', 'CONFLICT'))," +
                        "CHECK (status != 'QUEUED' OR mutation_id IS NOT NULL)" +
                        ")",
            )
        }

        override fun onUpgrade(database: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
            if (oldVersion == 1 && newVersion == 2) {
                createReminderCommands(database)
            } else {
                throw UnsupportedWorkspaceSchemaException(oldVersion, SCHEMA_VERSION)
            }
        }

        override fun onDowngrade(database: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
            throw UnsupportedWorkspaceSchemaException(oldVersion, SCHEMA_VERSION)
        }
    }
}

private sealed interface NormalizedWorkspaceSpec {
    data object Local : NormalizedWorkspaceSpec
    data class Remote(val serverOrigin: String, val accountId: String) : NormalizedWorkspaceSpec
}

private data class StoredIdentity(
    val kind: String,
    val workspaceId: String,
    val clientId: String,
    val serverOrigin: String?,
    val accountId: String?,
)

private data class StoredReminderCommand(
    val fingerprint: ByteArray,
    val status: ReminderCommandStatus,
)

private data class ResourcePayload(
    val kind: String,
    val id: String,
    val json: ByteArray,
    val snapshotOrdinal: Long? = null,
)

private data class TypedKey(val kind: String, val id: String)

private fun android.database.Cursor.getNullableString(index: Int): String? =
    if (isNull(index)) null else getString(index)

private fun android.database.Cursor.getNullableLong(index: Int): Long? =
    if (isNull(index)) null else getLong(index)

private fun android.database.Cursor.getNullableBlob(index: Int): ByteArray? =
    if (isNull(index)) null else getBlob(index)

private fun ContentValues.putNullableString(key: String, value: String?) {
    if (value == null) putNull(key) else put(key, value)
}

private inline fun <T> MutableList<T>.upsert(value: T, id: (T) -> String) {
    val index = indexOfFirst { id(it) == id(value) }
    if (index == -1) add(value) else this[index] = value
}
