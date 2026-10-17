package com.example.subroutine_simple.data.repository

import android.content.Context
import android.util.Log
import com.example.subroutine_simple.data.local.LastVerifiedSnapshotStore
import com.example.subroutine_simple.data.local.ReminderCommandStatus
import com.example.subroutine_simple.data.local.SQLiteWorkspaceStore
import com.example.subroutine_simple.data.local.StaleResolutionKind
import com.example.subroutine_simple.data.local.WorkspaceIdentity
import com.example.subroutine_simple.data.local.WorkspaceProjection
import com.example.subroutine_simple.data.local.WorkspaceRegistry
import com.example.subroutine_simple.data.local.WorkspaceSpec
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.ActionTemplate
import com.example.subroutine_simple.data.models.AllData
import com.example.subroutine_simple.data.models.ApiErrorBody
import com.example.subroutine_simple.data.models.ApiErrorCode
import com.example.subroutine_simple.data.models.ClientMutation
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.EventTemplate
import com.example.subroutine_simple.data.models.MUTATION_PROTOCOL_VERSION
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.MarkerTemplate
import com.example.subroutine_simple.data.models.MutationOperation
import com.example.subroutine_simple.data.models.MutationRequest
import com.example.subroutine_simple.data.models.OptimisticPatch

import com.example.subroutine_simple.data.models.ResourceKey
import com.example.subroutine_simple.data.models.ResourceValue
import com.example.subroutine_simple.data.models.Routine
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.data.models.SignalTemplate
import com.example.subroutine_simple.data.models.UuidV7
import com.example.subroutine_simple.data.network.InstantiateRequest
import com.example.subroutine_simple.data.network.PinRequest
import com.example.subroutine_simple.data.network.SubroutineApi
import com.example.subroutine_simple.ui.creator.CreateItemRequest
import com.example.subroutine_simple.ui.creator.CreatorModel
import com.example.subroutine_simple.ui.creator.CreatorModelIdentity
import com.example.subroutine_simple.ui.creator.toModel
import java.io.IOException
import java.nio.charset.StandardCharsets
import java.time.Instant
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.min
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import retrofit2.HttpException

private const val TAG = "SubroutineRepo"
private const val NIL_UUID = "00000000-0000-0000-0000-000000000000"

sealed interface WorkspaceDescriptor {
    val workspaceId: String

    data class Local(
        override val workspaceId: String,
    ) : WorkspaceDescriptor

    data class Remote(
        override val workspaceId: String,
        val accountId: String,
    ) : WorkspaceDescriptor
}

data class AllItems(
    val datasetId: String,
    val seq: Long,
    val actions: List<Action>,
    val events: List<Event>,
    val routines: List<Routine>,
    val markers: List<Marker>,
    val signals: List<Signal>,
    val actionTemplates: List<ActionTemplate>,
    val eventTemplates: List<EventTemplate>,
    val markerTemplates: List<MarkerTemplate>,
    val signalTemplates: List<SignalTemplate>,
    val pendingCount: Int = 0,
    val blockedCount: Int = 0,
    val blockedMutationId: String? = null,
    val blockedMessage: String? = null,
    val isLocalOnly: Boolean = false,
    val syncError: String? = null,

)

data class RepositorySession(val api: SubroutineApi, val checkCurrent: () -> Unit)

data class MutationOutcome(
    val items: AllItems,
    val syncError: String? = null,
)

class WorkspaceEditConflictException : IllegalStateException(
    "This item changed while you were editing it. Please reopen it and try again.",
)

class WorkspaceMergeRequiredException(
    val localItemCount: Int,
    val remoteItemCount: Int,
) : IllegalStateException(
    "Choose whether to import $localItemCount local item${if (localItemCount == 1) "" else "s"} into this account.",
)

private data class PendingWorkspaceMerge(
    val authenticationIdentity: String,
    val remoteAccountId: String,
    val remoteDatasetId: String,
    val localWorkspaceId: String,
    val localData: AllData,
    val checkSession: () -> Unit,
)

class SubroutineRepository(
    api: SubroutineApi,
    context: Context,
    private val serverOrigin: String,
    private val sessionFactory: ((String?) -> RepositorySession)? = null,
) : AutoCloseable {
    private val applicationContext = context.applicationContext
    private val legacySnapshotStore = LastVerifiedSnapshotStore(applicationContext.filesDir, serverOrigin)
    private val workspaceRegistry = WorkspaceRegistry(applicationContext)
    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
        classDiscriminator = "type"
    }

    private var activeAuthenticationIdentity: String? = null
    private var activeRemoteAccountId: String? = null
    private var workspace: SQLiteWorkspaceStore? = null
    private var pendingWorkspaceMerge: PendingWorkspaceMerge? = null
    private val synchronization = Mutex()
    private val closed = AtomicBoolean(false)
    private val fallbackApi = api
    private var operationSession: RepositorySession? = null
    private var operationGuard: (() -> Unit)? = null

    @Volatile
    private var activeBinding: Pair<String?, WorkspaceDescriptor>? = null

    private val api: SubroutineApi
        get() {
            checkOperation()
            return requireNotNull(operationSession).api
        }

    private suspend fun <T> serialized(
        authenticationIdentity: String?,
        isCurrent: () -> Boolean = { true },
        block: suspend () -> T,
    ): T {
        if (closed.get()) throw CancellationException("Repository is closed")
        val session = sessionFactory?.invoke(authenticationIdentity) ?: RepositorySession(fallbackApi) {}
        return withContext(Dispatchers.IO) {
            try {
                synchronization.withLock {
                    val operationContext = currentCoroutineContext()
                    operationSession = session
                    operationGuard = {
                        operationContext.ensureActive()
                        if (closed.get()) throw CancellationException("Repository is closed")
                        session.checkCurrent()
                        if (!isCurrent()) throw CancellationException("Reminder binding is no longer current")
                    }
                    try {
                        checkOperation()
                        block().also { checkOperation() }
                    } finally {
                        operationGuard = null
                        operationSession = null
                    }
                }
            } finally {
                if (closed.get()) close()
            }
        }
    }

    private fun checkOperation() = requireNotNull(operationGuard) {
        "Repository access requires a serialized operation"
    }.invoke()

    private suspend fun <T> network(block: suspend (SubroutineApi) -> T): T {
        try {
            return block(api)
        } finally {
            checkOperation()
        }
    }

    suspend fun loadLocal(): AllItems = serialized(null) {
        localWorkspace().loadProjection().toAllItems().copy(isLocalOnly = true)
    }

    fun activeWorkspaceDescriptor(authenticationIdentity: String?): WorkspaceDescriptor? {
        val binding = activeBinding ?: return null
        return binding.second.takeIf { !closed.get() && binding.first == authenticationIdentity }
    }

    suspend fun removeOfflineAccountData(authenticationIdentity: String) = serialized(authenticationIdentity) {
        val remoteAccountId = workspaceRegistry.remoteAccountId(serverOrigin, authenticationIdentity)
        if (activeAuthenticationIdentity == authenticationIdentity) closeWorkspace()
        if (remoteAccountId != null) {
            val spec = WorkspaceSpec.Remote(serverOrigin = serverOrigin, accountId = remoteAccountId)
            val databaseName = SQLiteWorkspaceStore.databaseNameFor(spec)
            checkOperation()
            if (!applicationContext.deleteDatabase(databaseName) &&
                applicationContext.getDatabasePath(databaseName).exists()
            ) {
                throw IllegalStateException("Could not remove offline account data")
            }
            checkOperation()
            workspaceRegistry.forget(serverOrigin, authenticationIdentity)
        }
        checkOperation()
        legacySnapshotStore.delete(authenticationIdentity)
        pendingWorkspaceMerge = null
    }

    suspend fun loadCached(authenticationIdentity: String): AllItems? = serialized(authenticationIdentity) {
        val remoteAccountId = workspaceRegistry.remoteAccountId(serverOrigin, authenticationIdentity)
        if (remoteAccountId != null) {
            val projection = workspace(authenticationIdentity, remoteAccountId).loadProjection()
            if (projection.authoritativeSnapshot) return@serialized projection.toAllItems()
        }
        legacySnapshotStore.read(authenticationIdentity)?.toAllItems()
    }

    suspend fun resync(authenticationIdentity: String): AllItems =
        fetchAll(authenticationIdentity, replayPending = false)

    suspend fun fetchAll(
        authenticationIdentity: String,
        replayPending: Boolean = true,
    ): AllItems = serialized(authenticationIdentity) {
        val knownRemoteAccount = workspaceRegistry.remoteAccountId(
            serverOrigin,
            authenticationIdentity,
        )
        val localCandidate = if (knownRemoteAccount == null) {
            val localStore = localWorkspace()
            localStore.loadProjection().data.takeIf { it.hasResources() }?.let { data ->
                localStore.identity.workspaceId to data
            }
        } else {
            null
        }
        val account = network { it.getAccount() }
        require(account.datasetId != NIL_UUID) { "The server returned a missing dataset identity" }
        val store = workspace(authenticationIdentity, account.accountId)

        val pulled = pullCanonical(store, account.datasetId)
        if (knownRemoteAccount == null && localCandidate != null) {
            pendingWorkspaceMerge = PendingWorkspaceMerge(
                authenticationIdentity = authenticationIdentity,
                remoteAccountId = account.accountId,
                remoteDatasetId = account.datasetId,
                localWorkspaceId = localCandidate.first,
                localData = localCandidate.second,
                checkSession = requireNotNull(operationSession).checkCurrent,
            )
            throw WorkspaceMergeRequiredException(
                localItemCount = localCandidate.second.resourceCount(),
                remoteItemCount = pulled.data.resourceCount(),
            )
        }
        checkOperation()
        workspaceRegistry.remember(serverOrigin, authenticationIdentity, account.accountId)
        if (pulled.pendingCount == 0) {
            checkOperation()
            runCatching { legacySnapshotStore.write(authenticationIdentity, pulled.data) }
                .onFailure { Log.w(TAG, "Could not update the legacy snapshot fallback", it) }
        }

        val (projection, error) = if (replayPending) replayOutbox(store) else pulled to null
        checkOperation()
        projection.toAllItems().copy(syncError = error)
    }

    private suspend fun pullCanonical(store: SQLiteWorkspaceStore, datasetId: String): WorkspaceProjection {
        checkOperation()
        require(datasetId != NIL_UUID) { "The server returned a missing dataset identity" }
        val beforePull = store.state()
        return if (beforePull.authoritativeSnapshot && beforePull.datasetId == datasetId) {
            val delta = network { it.getDataDelta(beforePull.canonicalSeq) }
            require(delta.datasetId == datasetId) { "The account and delta dataset identities do not match" }
            checkOperation()
            store.applyDelta(delta)
        } else {
            val snapshot = network { it.getAllData() }
            require(snapshot.datasetId == datasetId) { "The account and snapshot dataset identities do not match" }
            checkOperation()
            store.installSnapshot(snapshot, beforePull)
        }
    }

    suspend fun keepLocalDataSeparate(authenticationIdentity: String): AllItems =
        serialized(authenticationIdentity) {
            val pending = requirePendingMerge(authenticationIdentity)
            checkOperation()
            workspaceRegistry.remember(
                serverOrigin,
                authenticationIdentity,
                pending.remoteAccountId,
            )
            pendingWorkspaceMerge = null
            requireNotNull(workspace).loadProjection().toAllItems()
        }

    suspend fun mergeLocalData(authenticationIdentity: String): MutationOutcome =
        serialized(authenticationIdentity) {
            val pending = requirePendingMerge(authenticationIdentity)
            val account = network { it.getAccount() }
            check(account.accountId == pending.remoteAccountId && account.datasetId == pending.remoteDatasetId) {
                "The account dataset changed after this import was offered. Refresh before choosing again."
            }
            val store = workspace(authenticationIdentity, pending.remoteAccountId)
            pullCanonical(store, pending.remoteDatasetId)
            val localResources = pending.localData.resourceValues()
            if (localResources.isNotEmpty()) {
                val state = store.state()
                val remoteData = store.loadProjection().data
                val remoteByIdentity = remoteData.resourceValues().associateBy(::resourceIdentity)
                val collisions = localResources.filter { local ->
                    remoteByIdentity[resourceIdentity(local)]?.let { it != local } == true
                }
                check(collisions.isEmpty()) {
                    "Local import stopped because ${collisions.size} item identifier${if (collisions.size == 1) "" else "s"} already exist in this account with different content. Keep the workspaces separate."
                }
                val resources = localResources.filter { resourceIdentity(it) !in remoteByIdentity }
                val mergedRoutineOrder = (
                        remoteData.routines.map { it.id } + pending.localData.routines.map { it.id }
                        ).distinct()
                val newMutations = buildList {
                    val resourcesMutationId = stableImportMutationId(pending, "resources")
                    if (resources.isNotEmpty() && store.outbox(resourcesMutationId) == null) {
                        add(
                            ClientMutation(
                                request = MutationRequest(
                                    protocolVersion = MUTATION_PROTOCOL_VERSION,
                                    datasetId = requireNotNull(state.datasetId),
                                    mutationId = resourcesMutationId,
                                    clientId = store.identity.clientId,
                                    baseSeq = state.canonicalSeq,
                                    operation = MutationOperation.UpsertResources(resources),
                                ),
                                optimisticPatch = OptimisticPatch(writes = resources),
                            ),
                        )
                    }
                    if (pending.localData.routines.isNotEmpty() &&
                        mergedRoutineOrder != remoteData.routines.map { it.id }
                    ) {
                        val orderMutationId = stableImportMutationId(pending, "routine-order")
                        if (store.outbox(orderMutationId) == null) {
                            add(
                                ClientMutation(
                                    request = MutationRequest(
                                        protocolVersion = MUTATION_PROTOCOL_VERSION,
                                        datasetId = requireNotNull(state.datasetId),
                                        mutationId = orderMutationId,
                                        clientId = store.identity.clientId,
                                        baseSeq = state.canonicalSeq,
                                        operation = MutationOperation.ReorderRoutines(
                                            mergedRoutineOrder,
                                        ),
                                    ),
                                    optimisticPatch = OptimisticPatch(
                                        routineOrder = mergedRoutineOrder,
                                    ),
                                ),
                            )
                        }
                    }
                }
                checkOperation()
                if (newMutations.isNotEmpty()) store.enqueueAll(newMutations)
            }
            checkOperation()
            workspaceRegistry.remember(
                serverOrigin,
                authenticationIdentity,
                pending.remoteAccountId,
            )
            pendingWorkspaceMerge = null
            val (projection, error) = replayOutbox(store)
            MutationOutcome(projection.toAllItems(), error)
        }

    private fun requirePendingMerge(authenticationIdentity: String): PendingWorkspaceMerge {
        val pending = pendingWorkspaceMerge
            ?: throw IllegalStateException("No local-to-account decision is pending.")
        pending.checkSession()
        require(pending.authenticationIdentity == authenticationIdentity) {
            "The pending local-data decision belongs to another account."
        }
        check(
            activeAuthenticationIdentity == authenticationIdentity &&
                    activeRemoteAccountId == pending.remoteAccountId &&
                    workspace?.state()?.datasetId == pending.remoteDatasetId
        ) { "The workspace changed after this import was offered. Refresh before choosing again." }
        return pending
    }

    suspend fun createItem(
        authenticationIdentity: String?,
        request: CreateItemRequest,
        attemptSync: Boolean = true,
        creationId: String = UuidV7.newString(),
    ): MutationOutcome = serialized(authenticationIdentity) {
        require(UUID.fromString(creationId).toString() == creationId && creationId != NIL_UUID) {
            "Creation identity must be a canonical non-nil UUID."
        }
        val value = when (val model = request.toModel(CreatorModelIdentity(id = creationId))) {
            is CreatorModel.Action -> ResourceValue.Action(model.value)
            is CreatorModel.Event -> ResourceValue.Event(model.value)
            is CreatorModel.Marker -> ResourceValue.Marker(model.value)
            is CreatorModel.Signal -> ResourceValue.Signal(model.value)
            is CreatorModel.Routine -> ResourceValue.Routine(model.value)
        }
        val store = if (authenticationIdentity == null) localWorkspace() else {
            activeRemoteWorkspace(authenticationIdentity) ?: throw IllegalStateException(
                "Connect once before creating or editing offline so this account can be verified.",
            )
        }
        val projection = store.loadProjection()
        val existing = projection.data.run {
            listOfNotNull(
                actions.firstOrNull { it.id == creationId }?.let(ResourceValue::Action),
                events.firstOrNull { it.id == creationId }?.let(ResourceValue::Event),
                markers.firstOrNull { it.id == creationId }?.let(ResourceValue::Marker),
                signals.firstOrNull { it.id == creationId }?.let(ResourceValue::Signal),
                routines.firstOrNull { it.id == creationId }?.let(ResourceValue::Routine),
            )
        }
        if (existing.isNotEmpty()) {
            if (existing != listOf(value)) throw WorkspaceEditConflictException()
            return@serialized MutationOutcome(
                projection.toAllItems().copy(isLocalOnly = authenticationIdentity == null)
            )
        }
        enqueueMutation(
            authenticationIdentity,
            operation = if (value is ResourceValue.Action) MutationOperation.UpsertAction(value.value)
            else MutationOperation.UpsertResources(listOf(value)),
            patch = OptimisticPatch(writes = listOf(value)),
            attemptSync = attemptSync,
        )
    }

    suspend fun updateActions(
        authenticationIdentity: String?,
        actions: List<Action>,
        attemptSync: Boolean = true,
        expectedActions: List<Action>? = null,
    ): MutationOutcome = serialized(authenticationIdentity) {
        if (expectedActions != null) {
            requireUnchangedActions(authenticationIdentity, actions.map { it.id }, expectedActions)
        }
        enqueueMutation(
            authenticationIdentity,
            operation = MutationOperation.UpsertActions(actions),
            patch = OptimisticPatch(writes = actions.map(ResourceValue::Action)),
            attemptSync = attemptSync,
        )
    }

    suspend fun deleteActions(
        authenticationIdentity: String?,
        ids: List<String>,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueMutation(
            authenticationIdentity,
            operation = MutationOperation.DeleteResources(ids.map(ResourceKey::Action)),
            patch = OptimisticPatch(deletes = ids.map(ResourceKey::Action)),
            attemptSync = attemptSync,
        )
    }

    suspend fun updateAction(
        authenticationIdentity: String?,
        action: Action,
        attemptSync: Boolean = true,
        expectedAction: Action? = null,
    ): MutationOutcome = serialized(authenticationIdentity) {
        if (expectedAction != null) {
            requireUnchangedActions(authenticationIdentity, listOf(action.id), listOf(expectedAction))
        }
        enqueueUpsert(
            authenticationIdentity,
            MutationOperation.UpsertAction(action),
            ResourceValue.Action(action),
            attemptSync,
        )
    }

    suspend fun updateEvent(
        authenticationIdentity: String?,
        event: Event,
        attemptSync: Boolean = true,
        expectedEvent: Event? = null,
    ): MutationOutcome = serialized(authenticationIdentity) {
        if (expectedEvent != null &&
            editProjection(authenticationIdentity).data.events.firstOrNull { it.id == event.id } != expectedEvent
        ) throw WorkspaceEditConflictException()
        enqueueResource(authenticationIdentity, ResourceValue.Event(event), attemptSync)
    }

    suspend fun deleteAction(
        authenticationIdentity: String?,
        id: String,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueMutation(
            authenticationIdentity,
            operation = MutationOperation.DeleteAction(id),
            patch = OptimisticPatch(deletes = listOf(ResourceKey.Action(id))),
            attemptSync = attemptSync,
        )
    }

    suspend fun deleteEvent(
        authenticationIdentity: String?,
        id: String,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueMutation(
            authenticationIdentity,
            operation = MutationOperation.DeleteResources(listOf(ResourceKey.Event(id))),
            patch = OptimisticPatch(deletes = listOf(ResourceKey.Event(id))),
            attemptSync = attemptSync,
        )
    }

    suspend fun updateMarker(
        authenticationIdentity: String?,
        marker: Marker,
        attemptSync: Boolean = true,
        expectedMarker: Marker? = null,
    ): MutationOutcome = serialized(authenticationIdentity) {
        if (expectedMarker != null &&
            editProjection(authenticationIdentity).data.markers.firstOrNull { it.id == marker.id } != expectedMarker
        ) throw WorkspaceEditConflictException()
        enqueueResource(authenticationIdentity, ResourceValue.Marker(marker), attemptSync)
    }

    suspend fun deleteMarker(
        authenticationIdentity: String?,
        id: String,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueMutation(
            authenticationIdentity,
            operation = MutationOperation.DeleteResources(listOf(ResourceKey.Marker(id))),
            patch = OptimisticPatch(deletes = listOf(ResourceKey.Marker(id))),
            attemptSync = attemptSync,
        )
    }

    suspend fun updateSignal(
        authenticationIdentity: String?,
        signal: Signal,
        attemptSync: Boolean = true,
        expectedSignal: Signal? = null,
    ): MutationOutcome = serialized(authenticationIdentity) {
        if (expectedSignal != null &&
            editProjection(authenticationIdentity).data.signals.firstOrNull { it.id == signal.id } != expectedSignal
        ) throw WorkspaceEditConflictException()
        enqueueResource(authenticationIdentity, ResourceValue.Signal(signal), attemptSync)
    }

    suspend fun updateActionTemplate(
        authenticationIdentity: String?,
        template: ActionTemplate,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueResource(authenticationIdentity, ResourceValue.ActionTemplate(template), attemptSync)
    }

    suspend fun updateEventTemplate(
        authenticationIdentity: String?,
        template: EventTemplate,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueResource(authenticationIdentity, ResourceValue.EventTemplate(template), attemptSync)
    }

    suspend fun deleteActionTemplate(
        authenticationIdentity: String?,
        id: String,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueMutation(
            authenticationIdentity,
            operation = MutationOperation.DeleteResources(listOf(ResourceKey.ActionTemplate(id))),
            patch = OptimisticPatch(deletes = listOf(ResourceKey.ActionTemplate(id))),
            attemptSync = attemptSync,
        )
    }

    suspend fun deleteEventTemplate(
        authenticationIdentity: String?,
        id: String,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueMutation(
            authenticationIdentity,
            operation = MutationOperation.DeleteResources(listOf(ResourceKey.EventTemplate(id))),
            patch = OptimisticPatch(deletes = listOf(ResourceKey.EventTemplate(id))),
            attemptSync = attemptSync,
        )
    }

    suspend fun updateRoutine(
        authenticationIdentity: String?,
        routine: Routine,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueResource(authenticationIdentity, ResourceValue.Routine(routine), attemptSync)
    }

    suspend fun deleteSignal(
        authenticationIdentity: String?,
        id: String,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueMutation(
            authenticationIdentity,
            operation = MutationOperation.DeleteResources(listOf(ResourceKey.Signal(id))),
            patch = OptimisticPatch(deletes = listOf(ResourceKey.Signal(id))),
            attemptSync = attemptSync,
        )
    }

    suspend fun deleteRoutine(
        authenticationIdentity: String?,
        id: String,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        enqueueMutation(
            authenticationIdentity,
            operation = MutationOperation.DeleteResources(listOf(ResourceKey.Routine(id))),
            patch = OptimisticPatch(deletes = listOf(ResourceKey.Routine(id))),
            attemptSync = attemptSync,
        )
    }

    suspend fun completeAction(
        authenticationIdentity: String?,
        action: Action,
        completedAt: Instant = Instant.now(),
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        val projection = requireUnchangedActions(authenticationIdentity, listOf(action.id), listOf(action))
        if (action.completion != null) {
            return@serialized MutationOutcome(
                projection.toAllItems().copy(isLocalOnly = authenticationIdentity == null)
            )
        }
        if (authenticationIdentity == null && action.recurrence != null) {
            throw IllegalStateException(
                "Recurring actions cannot be completed in local-only mode until recurrence generation is available.",
            )
        }
        val completed = action.copy(
            queued = false,
            pinned = false,
            completion = completedAt.toString(),
        )
        enqueueMutation(
            authenticationIdentity,
            operation = MutationOperation.CompleteAction(action.id, completedAt.toString()),
            patch = OptimisticPatch(writes = listOf(ResourceValue.Action(completed))),
            attemptSync = attemptSync,
        )
    }

    private fun editProjection(authenticationIdentity: String?): WorkspaceProjection {
        checkOperation()
        val store = if (authenticationIdentity == null) {
            localWorkspace()
        } else {
            activeRemoteWorkspace(authenticationIdentity)
                ?: throw IllegalStateException("No verified remote workspace is available. Reopen this item after connecting.")
        }
        return store.loadProjection()
    }

    private fun requireUnchangedActions(
        authenticationIdentity: String?,
        targetIds: List<String>,
        expectedActions: List<Action>,
    ): WorkspaceProjection {
        val projection = editProjection(authenticationIdentity)
        val current = projection.data.actions.associateBy { it.id }
        val expected = expectedActions.associateBy { it.id }
        val targets = targetIds.toSet()
        if (targets.size != targetIds.size || expected.size != expectedActions.size ||
            !targets.containsAll(expected.keys) || targets.any { current[it] != expected[it] }
        ) {
            throw WorkspaceEditConflictException()
        }
        return projection
    }

    suspend fun reminderCommandStatus(
        authenticationIdentity: String?,
        workspace: WorkspaceDescriptor,
        commandId: String,
    ): ReminderCommandStatus? = serialized(authenticationIdentity) {
        val spec = when (workspace) {
            is WorkspaceDescriptor.Local -> {
                if (authenticationIdentity != null) throw CancellationException("Local reminder requires signed-out mode")
                WorkspaceSpec.Local
            }

            is WorkspaceDescriptor.Remote -> {
                if (authenticationIdentity == null) throw CancellationException("Remote reminder requires authentication")
                val accountId = activeRemoteAccountId.takeIf { activeAuthenticationIdentity == authenticationIdentity }
                    ?: workspaceRegistry.remoteAccountId(serverOrigin, authenticationIdentity)
                    ?: return@serialized null
                if (accountId != workspace.accountId) {
                    throw CancellationException("Reminder belongs to another server account")
                }
                WorkspaceSpec.Remote(serverOrigin, accountId)
            }
        }
        val cached = this@SubroutineRepository.workspace
        if (cached != null && activeAuthenticationIdentity == authenticationIdentity) {
            verifyReminderWorkspace(cached, workspace)
            return@serialized cached.reminderCommandStatus(commandId)
        }
        val databaseName = SQLiteWorkspaceStore.databaseNameFor(spec)
        if (!applicationContext.getDatabasePath(databaseName).isFile) return@serialized null
        checkOperation()
        SQLiteWorkspaceStore.open(applicationContext, spec).use { store ->
            verifyReminderWorkspace(store, workspace)
            store.reminderCommandStatus(commandId)
        }
    }

    suspend fun executeReminderCommand(
        authenticationIdentity: String?,
        workspace: WorkspaceDescriptor,
        commandId: String,
        datasetId: String?,
        actionId: String,
        expectedStart: String,
        completedAt: Instant,
        complete: Boolean,

        isCurrent: () -> Boolean,
    ): ReminderCommandStatus = serialized(authenticationIdentity, isCurrent) {
        val store = when (workspace) {
            is WorkspaceDescriptor.Local -> {
                if (authenticationIdentity != null) throw CancellationException("Local reminder requires signed-out mode")
                localWorkspace()
            }

            is WorkspaceDescriptor.Remote -> {
                if (authenticationIdentity == null) throw CancellationException("Remote reminder requires authentication")
                activeRemoteWorkspace(authenticationIdentity) ?: run {
                    val account = network { it.getAccount() }
                    if (account.accountId != workspace.accountId) {
                        throw CancellationException("Reminder belongs to another server account")
                    }
                    this@SubroutineRepository.workspace(authenticationIdentity, account.accountId)
                }
            }
        }
        verifyReminderWorkspace(store, workspace)
        val previous = store.reminderCommandStatus(commandId)
        if (previous != null && previous != ReminderCommandStatus.QUEUED) return@serialized previous
        if (previous == null) {
            val state = store.state()
            val cachedAction = store.loadProjection().data.actions.firstOrNull { it.id == actionId }
            val matchingOccurrence = cachedAction != null && cachedAction.queued &&
                    cachedAction.completion == null && cachedAction.start == expectedStart
            if (workspace is WorkspaceDescriptor.Remote &&
                (!state.authoritativeSnapshot || state.datasetId != datasetId || !matchingOccurrence)
            ) {
                val account = network { it.getAccount() }
                if (account.accountId != workspace.accountId) {
                    throw CancellationException("Reminder belongs to another server account")
                }
                pullCanonical(store, account.datasetId)
            }
            checkOperation()
            verifyReminderWorkspace(store, workspace)
            val status = store.enqueueReminderCommand(
                commandId, datasetId, actionId, expectedStart, completedAt, complete,
            )
            if (status != ReminderCommandStatus.QUEUED) return@serialized status
        }
        replayOutbox(store)
        checkOperation()
        requireNotNull(store.reminderCommandStatus(commandId))
    }

    private fun verifyReminderWorkspace(store: SQLiteWorkspaceStore, expected: WorkspaceDescriptor) {
        checkOperation()
        val identity = store.identity
        val matches = identity.workspaceId == expected.workspaceId && when (expected) {
            is WorkspaceDescriptor.Local -> identity is WorkspaceIdentity.Local
            is WorkspaceDescriptor.Remote -> identity is WorkspaceIdentity.Remote && identity.accountId == expected.accountId
        }
        if (!matches) throw CancellationException("Reminder workspace is no longer current")
    }

    private suspend fun enqueueResource(
        authenticationIdentity: String?,
        value: ResourceValue,
        attemptSync: Boolean,
    ): MutationOutcome = enqueueMutation(
        authenticationIdentity,
        operation = MutationOperation.UpsertResources(listOf(value)),
        patch = OptimisticPatch(writes = listOf(value)),
        attemptSync = attemptSync,
    )

    private suspend fun enqueueUpsert(
        authenticationIdentity: String?,
        operation: MutationOperation,
        value: ResourceValue,
        attemptSync: Boolean,
    ): MutationOutcome = enqueueMutation(
        authenticationIdentity,
        operation = operation,
        patch = OptimisticPatch(writes = listOf(value)),
        attemptSync = attemptSync,
    )

    private suspend fun enqueueMutation(
        authenticationIdentity: String?,
        operation: MutationOperation,
        patch: OptimisticPatch,
        attemptSync: Boolean,
    ): MutationOutcome {
        checkOperation()
        if (authenticationIdentity == null) {
            val store = localWorkspace()
            checkOperation()
            return MutationOutcome(
                items = store.applyLocalPatch(patch).toAllItems().copy(isLocalOnly = true),
            )
        }

        val store = activeRemoteWorkspace(authenticationIdentity)
            ?: throw IllegalStateException(
                "Connect once before creating or editing offline so this account can be verified.",
            )
        val state = store.state()
        val datasetId = state.datasetId
            ?: throw IllegalStateException("The local workspace has no verified dataset yet.")
        val mutation = ClientMutation(
            request = MutationRequest(
                protocolVersion = MUTATION_PROTOCOL_VERSION,
                datasetId = datasetId,
                mutationId = UuidV7.newString(),
                clientId = store.identity.clientId,
                baseSeq = state.canonicalSeq,
                operation = operation,
            ),
            optimisticPatch = patch,
        )
        checkOperation()
        val optimistic = store.enqueue(mutation)
        if (!attemptSync) {
            return MutationOutcome(items = optimistic.toAllItems())
        }
        val (projection, syncError) = replayOutbox(store)
        return MutationOutcome(
            items = projection.toAllItems(),
            syncError = syncError,
        )
    }

    private suspend fun replayOutbox(
        store: SQLiteWorkspaceStore,
    ): Pair<WorkspaceProjection, String?> {
        while (true) {
            checkOperation()
            val entry = store.nextSendable() ?: return store.loadProjection() to null
            val mutationId = entry.mutation.request.mutationId
            val receipt = try {
                network { it.submitMutation(entry.mutation.request) }
            } catch (error: HttpException) {
                val structured = error.response()?.errorBody()?.string()?.let { body ->
                    runCatching { json.decodeFromString<ApiErrorBody>(body) }.getOrNull()
                }
                if (structured?.mutationId != null && structured.mutationId != mutationId) {
                    val message = "The server returned a rejection for another request."
                    checkOperation()
                    store.recordRetry(mutationId, retryAt(entry.attemptCount), message)
                    return store.loadProjection() to message
                }
                if (structured?.error == ApiErrorCode.STALE_BASE) {
                    val request = entry.mutation.request
                    val evidenceMatches = structured.mutationId == request.mutationId &&
                            structured.currentDatasetId == request.datasetId &&
                            structured.currentSeq?.let { it > request.baseSeq } == true
                    if (!evidenceMatches) {
                        val message = "The server returned stale-base evidence for another request."
                        checkOperation()
                        store.recordRetry(mutationId, retryAt(entry.attemptCount), message)
                        return store.loadProjection() to message
                    }
                    val resolution = try {
                        val delta = network { it.getDataDelta(request.baseSeq) }
                        checkOperation()
                        store.resolveStaleDelta(
                            mutationId = mutationId,
                            replacementMutationId = UuidV7.newString(),
                            since = request.baseSeq,
                            delta = delta,
                        )
                    } catch (cancelled: CancellationException) {
                        throw cancelled
                    } catch (_: Exception) {
                        val message = "Could not refresh the server changes needed to resolve this edit."
                        checkOperation()
                        store.recordRetry(mutationId, retryAt(entry.attemptCount), message)
                        return store.loadProjection() to message
                    }
                    if (resolution.kind == StaleResolutionKind.REBASED) continue
                    return resolution.projection to null
                }

                if (structured != null && !structured.retryable) {
                    checkOperation()
                    store.block(mutationId, structured)
                    return store.loadProjection() to null
                }
                val message = structured?.message ?: "Synchronization failed with HTTP ${error.code()}"
                checkOperation()
                store.recordRetry(mutationId, retryAt(entry.attemptCount), message)
                return store.loadProjection() to message
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: IOException) {
                val message = error.message ?: "Network unavailable"
                checkOperation()
                store.recordRetry(mutationId, retryAt(entry.attemptCount), message)
                return store.loadProjection() to message
            }

            checkOperation()
            store.recordReceipt(receipt)
            try {
                val state = store.state()
                val delta = network { it.getDataDelta(state.canonicalSeq) }
                checkOperation()
                store.applyDelta(delta)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                checkOperation()
                return store.loadProjection() to (error.message ?: "Waiting for canonical data")
            }
        }
    }

    private fun retryAt(attemptCount: Int): Long {
        val exponent = (attemptCount - 1).coerceIn(0, 5)
        val delayMillis = min(30_000L, 1_000L shl exponent)
        return System.currentTimeMillis() + delayMillis
    }

    private fun workspace(
        authenticationIdentity: String,
        remoteAccountId: String,
    ): SQLiteWorkspaceStore {
        checkOperation()
        if (workspace != null &&
            activeAuthenticationIdentity == authenticationIdentity &&
            activeRemoteAccountId == remoteAccountId
        ) {
            return requireNotNull(workspace)
        }
        closeWorkspace()
        checkOperation()
        return SQLiteWorkspaceStore.open(
            context = applicationContext,
            workspace = WorkspaceSpec.Remote(
                serverOrigin = serverOrigin,
                accountId = remoteAccountId,
            ),
        ).also {
            workspace = it
            activeAuthenticationIdentity = authenticationIdentity
            activeRemoteAccountId = remoteAccountId
            activeBinding =
                authenticationIdentity to WorkspaceDescriptor.Remote(it.identity.workspaceId, remoteAccountId)
        }
    }

    private fun activeRemoteWorkspace(authenticationIdentity: String): SQLiteWorkspaceStore? {
        checkOperation()
        if (workspace != null &&
            activeAuthenticationIdentity == authenticationIdentity &&
            activeRemoteAccountId != null
        ) {
            return workspace
        }
        val remoteAccountId = workspaceRegistry.remoteAccountId(serverOrigin, authenticationIdentity)
            ?: return null
        return workspace(authenticationIdentity, remoteAccountId)
    }

    private fun localWorkspace(): SQLiteWorkspaceStore {
        checkOperation()
        if (workspace != null && activeAuthenticationIdentity == null && activeRemoteAccountId == null) {
            return requireNotNull(workspace)
        }
        closeWorkspace()
        checkOperation()
        return SQLiteWorkspaceStore.open(
            context = applicationContext,
            workspace = WorkspaceSpec.Local,
        ).also {
            workspace = it
            activeAuthenticationIdentity = null
            activeRemoteAccountId = null
            activeBinding = null to WorkspaceDescriptor.Local(it.identity.workspaceId)
        }
    }

    suspend fun useRemoteConflict(
        authenticationIdentity: String,
        mutationId: String,
    ): MutationOutcome = serialized(authenticationIdentity) {
        val store = activeRemoteWorkspace(authenticationIdentity)
            ?: throw IllegalStateException("No verified remote workspace is available.")
        checkOperation()
        MutationOutcome(items = store.discardBlocked(mutationId).toAllItems())
    }

    suspend fun keepLocalConflict(
        authenticationIdentity: String,
        mutationId: String,
        attemptSync: Boolean = true,
    ): MutationOutcome = serialized(authenticationIdentity) {
        val store = activeRemoteWorkspace(authenticationIdentity)
            ?: throw IllegalStateException("No verified remote workspace is available.")
        checkOperation()
        val retried = store.retryBlocked(mutationId)
        if (!attemptSync) return@serialized MutationOutcome(items = retried.toAllItems())
        val (projection, error) = replayOutbox(store)
        MutationOutcome(items = projection.toAllItems(), syncError = error)
    }

    private fun WorkspaceProjection.toAllItems(): AllItems = data.toAllItems(
        pendingCount = pendingCount,
        blockedCount = blockedCount,
        blockedMutationId = conflict?.mutationId,
        blockedMessage = conflict?.error?.message,
    )

    private fun stableImportMutationId(
        pending: PendingWorkspaceMerge,
        suffix: String,
    ): String {
        val fingerprint = buildString {
            append("subroutine-local-import\u0000")
            append(pending.localWorkspaceId)
            append('\u0000')
            append(pending.remoteAccountId)
            append('\u0000')
            append(pending.remoteDatasetId)
            append('\u0000')
            append(suffix)
            append('\u0000')
            append(json.encodeToString(AllData.serializer(), pending.localData))
        }
        return UUID.nameUUIDFromBytes(
            fingerprint.toByteArray(StandardCharsets.UTF_8),
        ).toString()
    }

    private fun resourceIdentity(value: ResourceValue): String = when (value) {
        is ResourceValue.Action -> "action:${value.value.id}"
        is ResourceValue.Event -> "event:${value.value.id}"
        is ResourceValue.Routine -> "routine:${value.value.id}"
        is ResourceValue.Marker -> "marker:${value.value.id}"
        is ResourceValue.Signal -> "signal:${value.value.id}"
        is ResourceValue.ActionTemplate -> "action-template:${value.value.id}"
        is ResourceValue.EventTemplate -> "event-template:${value.value.id}"
        is ResourceValue.MarkerTemplate -> "marker-template:${value.value.id}"
        is ResourceValue.SignalTemplate -> "signal-template:${value.value.id}"
    }

    private fun AllData.hasResources(): Boolean = resourceCount() > 0

    private fun AllData.resourceCount(): Int = actions.size + events.size + routines.size +
            markers.size + signals.size + actionTemplates.size + eventTemplates.size +
            markerTemplates.size + signalTemplates.size

    private fun AllData.resourceValues(): List<ResourceValue> = buildList {
        actions.forEach { add(ResourceValue.Action(it)) }
        events.forEach { add(ResourceValue.Event(it)) }
        routines.forEach { add(ResourceValue.Routine(it)) }
        markers.forEach { add(ResourceValue.Marker(it)) }
        signals.forEach { add(ResourceValue.Signal(it)) }
        actionTemplates.forEach { add(ResourceValue.ActionTemplate(it)) }
        eventTemplates.forEach { add(ResourceValue.EventTemplate(it)) }
        markerTemplates.forEach { add(ResourceValue.MarkerTemplate(it)) }
        signalTemplates.forEach { add(ResourceValue.SignalTemplate(it)) }
    }

    private fun AllData.toAllItems(
        pendingCount: Int = 0,
        blockedCount: Int = 0,
        blockedMutationId: String? = null,
        blockedMessage: String? = null,
    ): AllItems = AllItems(
        datasetId = datasetId,
        seq = seq,
        actions = actions,
        events = events,
        routines = routines,
        markers = markers,
        signals = signals,
        actionTemplates = actionTemplates,
        eventTemplates = eventTemplates,
        markerTemplates = markerTemplates,
        signalTemplates = signalTemplates,
        pendingCount = pendingCount,
        blockedCount = blockedCount,
        blockedMutationId = blockedMutationId,
        blockedMessage = blockedMessage,
    )


    suspend fun pinAction(id: String, pinned: Boolean): Action =
        call("pinAction") { api.pinAction(id, PinRequest(pinned)) }

    suspend fun saveActionAsTemplate(id: String): ActionTemplate =
        call("saveAction") { api.saveAction(id) }

    suspend fun clearActionDuration(id: String): Action =
        call("clearActionDuration") { api.clearActionDuration(id) }

    suspend fun saveEventAsTemplate(id: String): EventTemplate =
        call("saveEvent") { api.saveEvent(id) }

    suspend fun upsertRoutine(routine: Routine): Routine =
        call("upsertRoutine") { api.upsertRoutine(routine.id, routine) }

    suspend fun instantiateRoutine(id: String, startTime: String?): List<Action> =
        call("instantiateRoutine") { api.instantiateRoutine(id, InstantiateRequest(startTime)) }

    suspend fun upsertMarker(marker: Marker): Marker =
        call("upsertMarker") { api.upsertMarker(marker.id, marker) }

    suspend fun upsertMarkerTemplate(template: MarkerTemplate): MarkerTemplate =
        call("upsertMarkerTemplate") { api.upsertMarkerTemplate(template.id, template) }

    suspend fun deleteMarkerTemplate(id: String) =
        call("deleteMarkerTemplate") { api.deleteMarkerTemplate(id) }

    suspend fun upsertSignal(signal: Signal): Signal =
        call("upsertSignal") { api.upsertSignal(signal.id, signal) }

    suspend fun upsertSignalTemplate(template: SignalTemplate): SignalTemplate =
        call("upsertSignalTemplate") { api.upsertSignalTemplate(template.id, template) }

    suspend fun deleteSignalTemplate(id: String) =
        call("deleteSignalTemplate") { api.deleteSignalTemplate(id) }

    suspend fun refreshPipeline(): List<Action> =
        call("refreshPipeline") { api.refreshPipeline() }

    suspend fun expeditePipeline(): List<Action> =
        call("expeditePipeline") { api.expeditePipeline() }

    private suspend fun <T> call(name: String, block: suspend () -> T): T =
        serialized(activeBinding?.first) {
            try {
                network { block() }
            } catch (error: HttpException) {
                Log.e(TAG, "$name failed with HTTP ${error.code()}")
                throw RuntimeException("$name failed with HTTP ${error.code()}", error)
            }
        }

    override fun close() {
        closed.set(true)
        if (synchronization.tryLock()) {
            try {
                closeWorkspace()
            } finally {
                synchronization.unlock()
            }
        }
    }

    private fun closeWorkspace() {
        activeBinding = null
        workspace?.close()
        workspace = null
        pendingWorkspaceMerge = null
        activeAuthenticationIdentity = null
        activeRemoteAccountId = null
    }
}
