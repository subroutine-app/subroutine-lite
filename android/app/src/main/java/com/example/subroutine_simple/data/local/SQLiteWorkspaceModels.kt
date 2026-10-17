package com.example.subroutine_simple.data.local

import com.example.subroutine_simple.data.models.ApiErrorBody
import com.example.subroutine_simple.data.models.ClientMutation
import com.example.subroutine_simple.data.models.MutationOperation
import com.example.subroutine_simple.data.models.MutationReceipt
import com.example.subroutine_simple.data.models.AllData

sealed interface WorkspaceSpec {
    data object Local : WorkspaceSpec

    data class Remote(
        val serverOrigin: String,
        val accountId: String,
    ) : WorkspaceSpec
}

sealed interface WorkspaceIdentity {
    val workspaceId: String
    val clientId: String

    data class Local(
        override val workspaceId: String,
        override val clientId: String,
    ) : WorkspaceIdentity

    data class Remote(
        override val workspaceId: String,
        override val clientId: String,
        val serverOrigin: String,
        val accountId: String,
    ) : WorkspaceIdentity
}

data class WorkspaceSyncState(
    val datasetId: String?,
    val canonicalSeq: Long,
    val authoritativeSnapshot: Boolean,
)

data class WorkspaceProjection(
    val data: AllData,
    val authoritativeSnapshot: Boolean,
    val pendingCount: Int,
    val blockedCount: Int,
    val conflict: BlockedConflict?,
)

data class BlockedConflict(
    val mutationId: String,
    val operation: MutationOperation,
    val error: ApiErrorBody,
)

enum class ReminderCommandStatus {
    QUEUED,
    CONFIRMED,
    SUPERSEDED,
    CONFLICT,
}

enum class OutboxStatus(val storedValue: String) {
    PENDING("pending"),
    AWAITING_CANONICAL("awaiting_canonical"),
    BLOCKED("blocked");

    companion object {
        internal fun fromStored(value: String): OutboxStatus =
            entries.firstOrNull { it.storedValue == value }
                ?: throw WorkspaceInvariantException("Unknown outbox state: $value")
    }
}

data class OutboxEntry(
    val position: Long,
    val mutation: ClientMutation,
    val status: OutboxStatus,
    val sealed: Boolean,
    val attemptCount: Int,
    val nextAttemptAtMillis: Long?,
    val lastError: String?,
    val receipt: MutationReceipt?,
    val blockedError: ApiErrorBody?,
)

enum class StaleResolutionKind {
    REBASED,
    BLOCKED,
}

data class StaleResolution(
    val projection: WorkspaceProjection,
    val kind: StaleResolutionKind,
)

data class WorkspaceDiagnostics(
    val applicationId: Int,
    val schemaVersion: Int,
    val journalMode: String,
    val foreignKeys: Boolean,
    val synchronousFull: Boolean,
    val busyTimeoutMillis: Long,
)

open class SQLiteWorkspaceException(message: String, cause: Throwable? = null) :
    IllegalStateException(message, cause)

class WorkspaceOpenException(message: String, cause: Throwable? = null) :
    SQLiteWorkspaceException(message, cause)

class CorruptWorkspaceDatabaseException(message: String, cause: Throwable? = null) :
    SQLiteWorkspaceException(message, cause)

class ForeignWorkspaceDatabaseException(message: String) : SQLiteWorkspaceException(message)

class UnsupportedWorkspaceSchemaException(actual: Int, supported: Int) :
    SQLiteWorkspaceException(
        "Workspace schema version $actual is newer than supported version $supported",
    )

class WorkspaceIdentityMismatchException(message: String) : SQLiteWorkspaceException(message)

class StaleCanonicalStateException :
    SQLiteWorkspaceException("Canonical state changed while the request was in flight")

class NoAuthoritativeSnapshotException :
    SQLiteWorkspaceException("An authoritative snapshot must be installed before applying a delta")

class SequenceRegressionException(current: Long, incoming: Long) :
    SQLiteWorkspaceException("Canonical sequence would regress from $current to $incoming")

class DatasetMismatchException(current: String?, incoming: String?) :
    SQLiteWorkspaceException("Incoming dataset $incoming does not match canonical dataset $current")

class DatasetChangedWithPendingIntentException(
    current: String?,
    incoming: String?,
    pending: Int,
) : SQLiteWorkspaceException(
    "Cannot replace dataset $current with $incoming while $pending local mutations remain",
)

class InvalidWorkspaceMutationException(message: String) : SQLiteWorkspaceException(message)

class OutboxMutationNotFoundException(mutationId: String) :
    SQLiteWorkspaceException("Outbox mutation $mutationId was not found")

class WorkspaceInvariantException(message: String, cause: Throwable? = null) :
    SQLiteWorkspaceException(message, cause)
