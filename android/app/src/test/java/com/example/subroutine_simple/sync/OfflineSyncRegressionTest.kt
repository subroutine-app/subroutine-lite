package com.example.subroutine_simple.sync

import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.data.local.LastVerifiedSnapshotStore
import com.example.subroutine_simple.data.models.AccountInfo
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.AllData
import com.example.subroutine_simple.data.models.ApiErrorBody
import com.example.subroutine_simple.data.models.ApiErrorCode
import com.example.subroutine_simple.data.models.DataDelta
import com.example.subroutine_simple.data.models.MutationEffect
import com.example.subroutine_simple.data.models.MutationOperation
import com.example.subroutine_simple.data.models.MutationReceipt
import com.example.subroutine_simple.data.models.MutationRequest
import com.example.subroutine_simple.data.models.MutationResult
import com.example.subroutine_simple.data.models.ResourceValue
import com.example.subroutine_simple.data.models.RoutineStep
import com.example.subroutine_simple.data.network.SubroutineApi
import com.example.subroutine_simple.data.repository.AllItems
import com.example.subroutine_simple.data.repository.RepositorySession
import com.example.subroutine_simple.data.repository.SubroutineRepository
import com.example.subroutine_simple.data.repository.WorkspaceMergeRequiredException
import com.example.subroutine_simple.data.repository.WorkspaceEditConflictException
import com.example.subroutine_simple.ui.creator.CreateItemRequest
import com.example.subroutine_simple.ui.creator.CreatorModel
import com.example.subroutine_simple.ui.creator.CreatorModelIdentity
import com.example.subroutine_simple.ui.creator.toModel
import com.example.subroutine_simple.ui.SyncConnectionSignal
import com.example.subroutine_simple.ui.SyncStatus
import com.example.subroutine_simple.ui.mutationPresentation
import com.example.subroutine_simple.ui.refreshPresentation
import java.io.IOException
import java.lang.reflect.Proxy
import java.time.Duration
import java.time.ZonedDateTime
import java.util.UUID
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import okhttp3.ResponseBody.Companion.toResponseBody
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.SQLiteMode
import retrofit2.HttpException
import retrofit2.Response

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [28])
@SQLiteMode(SQLiteMode.Mode.NATIVE)
class OfflineSyncRegressionTest {
    private val context get() = RuntimeEnvironment.getApplication()
    private val server = Server()
    private val generation = AtomicInteger()
    private val repositories = mutableListOf<SubroutineRepository>()
    private val action = Action(id(), id(), title = "Original")

    @After
    fun closeRepositories() {
        repositories.forEach { it.close() }
    }

    @Test
    fun fallbackSnapshotsAreIsolatedByServerAndAccount() {
        val snapshot = AllData(server.account.datasetId, actions = listOf(action))
        val first = LastVerifiedSnapshotStore(context.filesDir, ORIGIN)
        first.write(server.account.accountId, snapshot)

        assertEquals(
            snapshot,
            LastVerifiedSnapshotStore(context.filesDir, "$ORIGIN:443/").read(server.account.accountId)
        )
        assertNull(
            LastVerifiedSnapshotStore(
                context.filesDir,
                "https://other.example.test"
            ).read(server.account.accountId)
        )
        assertNull(first.read(id()))
        LastVerifiedSnapshotStore(context.filesDir, "https://other.example.test").delete(server.account.accountId)
        assertEquals(snapshot, first.read(server.account.accountId))
    }

    @Test
    fun offlineEditSurvivesReopeningAndFetchReplaysIt() = syncTest {
        server.data = server.data.copy(actions = listOf(action))
        val repo = repository()
        repo.fetchAll(server.account.accountId)
        val edited = action.copy(title = "Offline")
        val saved = repo.updateAction(server.account.accountId, edited, attemptSync = false, expectedAction = action)
        assertEquals(1, saved.items.pendingCount)
        assertTrue(server.requests.isEmpty())
        repo.close()

        val reopened = repository()
        assertEquals(listOf(edited), reopened.loadCached(server.account.accountId)?.actions)
        assertEquals(1, reopened.loadCached(server.account.accountId)?.pendingCount)
        val synchronized = reopened.fetchAll(server.account.accountId)
        assertEquals(listOf(edited), synchronized.actions)
        assertEquals(0, synchronized.pendingCount)
        assertEquals(1, server.requests.size)
    }

    @Test
    fun sessionExpiryAfterCommitKeepsDurableIntentAndReplaysTheSameRequest() = syncTest {
        server.data = server.data.copy(actions = listOf(action))
        val repo = repository()
        repo.fetchAll(server.account.accountId)
        server.afterCommit = { generation.incrementAndGet() }
        val edited = action.copy(title = "Saved before expiry")
        assertTrue(runCatching {
            repo.updateAction(server.account.accountId, edited, expectedAction = action)
        }.exceptionOrNull() is CancellationException)

        val cached = requireNotNull(repo.loadCached(server.account.accountId))
        assertEquals(listOf(edited), cached.actions)
        assertEquals(1, cached.pendingCount)
        server.afterCommit = {}
        val synchronized = repo.fetchAll(server.account.accountId)
        assertEquals(listOf(edited), synchronized.actions)
        assertEquals(0, synchronized.pendingCount)
        assertEquals(2, server.requests.size)
        assertEquals(server.requests.first(), server.requests.last())
    }

    @Test
    fun waitingForCanonicalDataIsPendingNotOffline() = syncTest {
        server.data = server.data.copy(actions = listOf(action))
        val repo = repository()
        repo.fetchAll(server.account.accountId)
        val previous = server.data
        server.readDelta = { DataDelta(previous.datasetId, previous.seq, actions = previous.actions) }
        val outcome = repo.updateAction(server.account.accountId, action.copy(title = "Waiting"))
        val signedIn = AuthenticationState.SignedIn(null, server.account.accountId, 0L)

        assertNull(outcome.syncError)
        assertEquals(1, outcome.items.pendingCount)
        assertEquals(SyncStatus.Ready, outcome.mutationPresentation(signedIn).syncStatus)
        assertTrue(outcome.mutationPresentation(signedIn).message?.contains("waiting") == true)
        val refreshed = repo.fetchAll(server.account.accountId).refreshPresentation()
        assertEquals(SyncStatus.Ready, refreshed.syncStatus)
        assertEquals(SyncConnectionSignal.Connected, refreshed.connectionSignal)
        assertEquals(1, server.requests.size)
        assertEquals(
            SyncStatus.Offline, outcome.mutationPresentation(
                AuthenticationState.InteractionRequired(null, server.account.accountId, 1L),
            ).syncStatus
        )
    }

    @Test
    fun domainConflictKeepsItsMessageWithoutClaimingConnectionFailure() = syncTest {
        server.data = server.data.copy(actions = listOf(action))
        val repo = repository()
        repo.fetchAll(server.account.accountId)
        server.reject = { request ->
            ApiErrorBody(
                ApiErrorCode.DOMAIN_CONFLICT, "Resolve this edit", mutationId = request.mutationId,
                currentDatasetId = request.datasetId, currentSeq = request.baseSeq, retryable = false,
            )
        }
        val outcome = repo.updateAction(server.account.accountId, action.copy(title = "Blocked"))
        assertNull(outcome.syncError)
        assertEquals(1, outcome.items.blockedCount)
        val presentation =
            outcome.mutationPresentation(AuthenticationState.SignedIn(null, server.account.accountId, 0L))
        assertEquals(SyncStatus.Ready, presentation.syncStatus)
        assertEquals("Resolve this edit", presentation.message)
        val refreshed = repo.fetchAll(server.account.accountId).refreshPresentation()
        assertEquals(SyncConnectionSignal.Connected, refreshed.connectionSignal)
        assertEquals("Resolve this edit", refreshed.message)
        assertEquals(1, server.requests.size)
    }

    @Test
    fun failedCanonicalRequestStillReportsOffline() = syncTest {
        server.data = server.data.copy(actions = listOf(action))
        val repo = repository()
        repo.fetchAll(server.account.accountId)
        server.readDelta = { throw IOException("Canonical unavailable") }
        val outcome = repo.updateAction(server.account.accountId, action.copy(title = "Awaiting recovery"))
        val presentation =
            outcome.mutationPresentation(AuthenticationState.SignedIn(null, server.account.accountId, 0L))
        assertEquals(SyncStatus.Offline, presentation.syncStatus)
        assertEquals("Canonical unavailable", presentation.message)
        assertEquals(1, outcome.items.pendingCount)
    }

    @Test
    fun explicitImportSynchronizesWithoutDeletingTheLocalWorkspace() = syncTest {
        val repo = offerImport()
        val merged = repo.mergeLocalData(server.account.accountId)
        assertEquals(listOf(action), merged.items.actions)
        assertEquals(0, merged.items.pendingCount)
        assertEquals(1, server.requests.size)
        assertEquals(listOf(action), repo.loadLocal().actions)
    }

    @Test
    fun localImportRequiresANewDecisionAfterSameAccountReauthentication() = syncTest {
        val repo = offerImport()
        generation.incrementAndGet()
        assertTrue(runCatching { repo.mergeLocalData(server.account.accountId) }.exceptionOrNull() is CancellationException)
        assertTrue(server.requests.isEmpty())
        assertEquals(listOf(action), repo.loadLocal().actions)
    }

    @Test
    fun datasetReplacementInvalidatesImportWithoutMovingLocalData() = syncTest {
        val repo = offerImport()
        server.account = server.account.copy(datasetId = id())
        server.data = AllData(server.account.datasetId)
        val error = runCatching { repo.mergeLocalData(server.account.accountId) }.exceptionOrNull()
        assertTrue(error is IllegalStateException)
        assertTrue(error?.message?.contains("dataset changed") == true)
        assertTrue(server.requests.isEmpty())
        assertEquals(listOf(action), repo.loadLocal().actions)
    }

    @Test
    fun importRechecksNewRemoteCollisionsBeforeEnqueueing() = syncTest {
        val repo = offerImport()
        val remote = action.copy(title = "New remote item with the same id")
        server.data = server.data.copy(seq = 1L, actions = listOf(remote))
        val error = runCatching { repo.mergeLocalData(server.account.accountId) }.exceptionOrNull()
        assertTrue(error is IllegalStateException)
        assertTrue(error?.message?.contains("different content") == true)
        assertTrue(server.requests.isEmpty())
        assertEquals(listOf(action), repo.loadLocal().actions)
        assertEquals(listOf(remote), server.data.actions)
    }

    @Test
    fun localCreationRetriesPreserveOneItemAcrossReopenForEveryKind() = syncTest {
        var repo = repository()
        creationRequests().forEachIndexed { index, request ->
            val creationId = id()
            val first = repo.createItem(null, request, attemptSync = false, creationId = creationId)
            assertEquals(index + 1, first.items.itemCount())
            assertEquals(0, first.items.pendingCount)
            assertTrue(first.items.isLocalOnly)
            repo.close()
            repo = repository()

            val retried = repo.createItem(null, request, creationId = creationId)
            assertEquals(first, retried)
            assertTrue(runCatching {
                repo.createItem(null, request.withTitle("Different input"), creationId = creationId)
            }.exceptionOrNull() is WorkspaceEditConflictException)
            assertEquals(first.items, repo.loadLocal())
        }
        assertTrue(server.requests.isEmpty())
    }

    @Test
    fun accountCreationRetriesPreserveOneQueuedWriteAcrossReopenForEveryKind() = syncTest {
        var repo = repository()
        repo.fetchAll(server.account.accountId)
        creationRequests().forEachIndexed { index, request ->
            val creationId = id()
            val first = repo.createItem(server.account.accountId, request, attemptSync = false, creationId = creationId)
            assertEquals(index + 1, first.items.itemCount())
            assertEquals(index + 1, first.items.pendingCount)
            assertNull(first.syncError)
            repo.close()
            repo = repository()

            val retried = repo.createItem(server.account.accountId, request, creationId = creationId)
            assertEquals(first, retried)
            assertTrue(runCatching {
                repo.createItem(server.account.accountId, request.withTitle("Different input"), creationId = creationId)
            }.exceptionOrNull() is WorkspaceEditConflictException)
            assertEquals(first.items, repo.loadCached(server.account.accountId))
        }
        assertTrue(server.requests.isEmpty())
    }

    @Test
    fun creationRetriesNeverOverwriteLaterCanonicalEditsForAnyKind() = syncTest {
        val creations = creationRequests().map { id() to it }
        server.data = creationSnapshot(creations)
        val repo = repository()
        val initial = repo.fetchAll(server.account.accountId)
        creations.forEach { (creationId, request) ->
            val retried = repo.createItem(server.account.accountId, request, creationId = creationId)
            assertEquals(initial, retried.items)
            assertEquals(0, retried.items.pendingCount)
        }
        server.data =
            creationSnapshot(creations.map { (id, request) -> id to request.withTitle("Edited later") }, seq = 1L)
        val edited = repo.resync(server.account.accountId)

        creations.forEach { (creationId, request) ->
            assertTrue(runCatching {
                repo.createItem(server.account.accountId, request, creationId = creationId)
            }.exceptionOrNull() is WorkspaceEditConflictException)
            assertEquals(edited, repo.loadCached(server.account.accountId))
        }
        assertTrue(server.requests.isEmpty())
    }

    @Test
    fun creationIdentityCannotBeReusedForAnotherKindInEitherWorkspace() = syncTest {
        val repo = repository()
        repo.fetchAll(server.account.accountId)
        val requests = creationRequests()
        for (identity in listOf(null, server.account.accountId)) {
            val creationId = id()
            val saved = repo.createItem(identity, requests[0], attemptSync = false, creationId = creationId)
            assertTrue(runCatching {
                repo.createItem(identity, requests[1], attemptSync = false, creationId = creationId)
            }.exceptionOrNull() is WorkspaceEditConflictException)
            val current = if (identity == null) repo.loadLocal() else repo.loadCached(identity)
            assertEquals(saved.items, current)
            assertEquals(1, saved.items.itemCount())
        }
        assertTrue(server.requests.isEmpty())
    }

    private fun creationRequests(): List<CreateItemRequest> {
        val at = ZonedDateTime.parse("2026-10-15T12:00:00Z")
        return listOf(
            CreateItemRequest.Action("Action", null, null, Duration.ofMinutes(30), queued = false, pinned = false),
            CreateItemRequest.Event("Event", "Notes", at, Duration.ofHours(1)),
            CreateItemRequest.Marker("Marker", null, at.toLocalDate(), at.toLocalDate().plusDays(1)),
            CreateItemRequest.Signal("Signal", null, at),
            CreateItemRequest.Routine("Routine", null, null, listOf(RoutineStep("Step", "PT5M"))),
        )
    }

    private fun CreateItemRequest.withTitle(title: String): CreateItemRequest = when (this) {
        is CreateItemRequest.Action -> copy(title = title)
        is CreateItemRequest.Event -> copy(title = title)
        is CreateItemRequest.Marker -> copy(title = title)
        is CreateItemRequest.Signal -> copy(title = title)
        is CreateItemRequest.Routine -> copy(title = title)
    }

    private fun AllItems.itemCount(): Int = actions.size + events.size + markers.size + signals.size + routines.size

    private fun creationSnapshot(creations: List<Pair<String, CreateItemRequest>>, seq: Long = 0L): AllData {
        val models = creations.map { (id, request) -> request.toModel(CreatorModelIdentity(id)) }
        return AllData(
            datasetId = server.account.datasetId,
            seq = seq,
            actions = models.filterIsInstance<CreatorModel.Action>().map { it.value },
            events = models.filterIsInstance<CreatorModel.Event>().map { it.value },
            markers = models.filterIsInstance<CreatorModel.Marker>().map { it.value },
            signals = models.filterIsInstance<CreatorModel.Signal>().map { it.value },
            routines = models.filterIsInstance<CreatorModel.Routine>().map { it.value },
        )
    }

    private suspend fun offerImport(): SubroutineRepository = repository().also { repo ->
        repo.updateAction(null, action, attemptSync = false)
        assertTrue(runCatching { repo.fetchAll(server.account.accountId) }.exceptionOrNull() is WorkspaceMergeRequiredException)
    }

    private fun repository() = SubroutineRepository(server.api, context, ORIGIN) {
        val captured = generation.get()
        RepositorySession(server.api) {
            if (captured != generation.get()) throw CancellationException("Session changed")
        }
    }.also { repositories += it }

    private class Server {
        var account = AccountInfo(id(), id())
        var data = AllData(account.datasetId)
        val requests = mutableListOf<MutationRequest>()
        private val receipts = mutableMapOf<String, MutationReceipt>()
        var afterCommit: () -> Unit = {}
        var readDelta: () -> DataDelta = {
            DataDelta(
                datasetId = data.datasetId, seq = data.seq, actions = data.actions, events = data.events,
                markers = data.markers, signals = data.signals, routines = data.routines,
                routineOrder = data.routines.map { it.id },
            )
        }
        var reject: ((MutationRequest) -> ApiErrorBody)? = null
        private val unsupportedApi = Proxy.newProxyInstance(
            SubroutineApi::class.java.classLoader,
            arrayOf(SubroutineApi::class.java)
        ) { _, method, _ -> error("Unexpected API call: ${method.name}") } as SubroutineApi

        val api = object : SubroutineApi by unsupportedApi {
            override suspend fun getAccount(): AccountInfo = account
            override suspend fun getAllData(): AllData = data
            override suspend fun getDataDelta(since: Long): DataDelta = readDelta()
            override suspend fun submitMutation(request: MutationRequest): MutationReceipt = submit(request)
        }

        private fun submit(request: MutationRequest): MutationReceipt {
            requests += request
            reject?.invoke(request)?.let { error ->
                throw HttpException(
                    Response.error<MutationReceipt>(
                        409,
                        Json.encodeToString(ApiErrorBody.serializer(), error).toResponseBody()
                    )
                )
            }
            receipts[request.mutationId]?.let { return it.copy(replayed = true) }
            assertEquals(account.datasetId, request.datasetId)
            assertEquals(data.seq, request.baseSeq)
            val (actions, result) = when (val operation = request.operation) {
                is MutationOperation.UpsertAction -> listOf(operation.action) to MutationResult.ActionUpserted(operation.action)
                is MutationOperation.UpsertResources -> operation.resources.map {
                    (it as ResourceValue.Action).value
                } to MutationResult.ResourcesUpserted(operation.resources)

                else -> error("Unexpected mutation: $operation")
            }
            data = data.copy(seq = data.seq + 1L, actions = actions)
            val receipt = MutationReceipt(
                request.protocolVersion, request.datasetId, request.mutationId, request.clientId,
                request.baseSeq, data.seq, MutationEffect.APPLIED, false, result,
            )
            receipts[request.mutationId] = receipt
            afterCommit()
            return receipt
        }
    }

    private companion object {
        const val ORIGIN = "https://sync.example.test"
        fun id() = UUID.randomUUID().toString()
        fun syncTest(block: suspend () -> Unit) = runBlocking { withTimeout(10_000) { block() } }
    }
}
