package com.example.subroutine_simple.ui

import android.app.Activity
import android.app.Application
import android.os.SystemClock
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.auth.AuthenticationRecovery
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.ActionTemplate
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.EventTemplate
import com.example.subroutine_simple.data.models.asTemplate
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.MarkerTemplate
import com.example.subroutine_simple.data.models.QueueItem
import com.example.subroutine_simple.data.models.Routine
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.data.models.SignalTemplate
import com.example.subroutine_simple.data.models.UuidV7
import com.example.subroutine_simple.data.models.isBacklogged
import com.example.subroutine_simple.data.models.isNotFullyPassed
import com.example.subroutine_simple.data.models.isQueued
import com.example.subroutine_simple.data.models.parseIsoDurationSeconds
import com.example.subroutine_simple.data.models.parseSchedulePoint
import com.example.subroutine_simple.data.network.SseManager
import com.example.subroutine_simple.data.network.RetrofitClient
import com.example.subroutine_simple.sync.SyncRecovery
import com.example.subroutine_simple.sync.isRetryableSyncFailure
import com.example.subroutine_simple.sync.syncRetryDelay
import com.example.subroutine_simple.sync.availableNetwork
import com.example.subroutine_simple.sync.canRetryInitialization
import com.example.subroutine_simple.sync.periodicallyRecoverSync
import java.io.IOException
import com.example.subroutine_simple.data.repository.AllItems
import com.example.subroutine_simple.data.repository.MutationOutcome
import com.example.subroutine_simple.data.repository.WorkspaceDescriptor
import com.example.subroutine_simple.data.repository.WorkspaceMergeRequiredException
import com.example.subroutine_simple.data.repository.WorkspaceEditConflictException
import com.example.subroutine_simple.reminders.ReminderScheduler
import com.example.subroutine_simple.reminders.LocalReminderRuntime
import com.example.subroutine_simple.reminders.canReconcileRemoteReminders
import com.example.subroutine_simple.sync.AppServices
import com.example.subroutine_simple.sync.BackgroundSyncWorker
import com.example.subroutine_simple.ui.settings.reminderSettingsKey
import com.example.subroutine_simple.settings.SettingsStore
import com.example.subroutine_simple.ui.creator.CreateItemRequest
import com.example.subroutine_simple.ui.creator.CreatorItemType
import com.example.subroutine_simple.ui.creator.CreatorSchedule
import com.example.subroutine_simple.ui.creator.ItemCreatorState
import com.example.subroutine_simple.ui.creator.CreatorSession
import com.example.subroutine_simple.ui.creator.InlineCreatorSource
import com.example.subroutine_simple.ui.timeline.TimelineDataIndex
import com.example.subroutine_simple.ui.timeline.TimelineEdit
import com.example.subroutine_simple.ui.timeline.TimelineEditMode
import com.example.subroutine_simple.ui.timeline.TimelineItemKind
import com.example.subroutine_simple.ui.timeline.validateDates
import java.time.Duration
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.retryWhen
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull

enum class SyncStatus {
    InitialLoading,
    Refreshing,
    Ready,
    Offline,
    AuthenticationRequired,
}

data class ActionsContent(
    val datasetId: String,
    val seq: Long,
    val queueItems: List<QueueItem>,
    val backlogged: List<Action>,
    val actions: List<Action>,
    val events: List<Event>,
    val routines: List<Routine>,
    val markers: List<Marker>,
    val signals: List<Signal>,
    val actionTemplates: List<ActionTemplate>,
    val eventTemplates: List<EventTemplate>,
    val markerTemplates: List<MarkerTemplate>,
    val signalTemplates: List<SignalTemplate>,
    val pendingCount: Int,
    val blockedCount: Int,
    val blockedMutationId: String?,
    val blockedMessage: String?,
    val isLocalOnly: Boolean,
)

data class WorkspaceMergePrompt(
    val localItemCount: Int,
    val remoteItemCount: Int,
)

data class ActionsUiState(
    val content: ActionsContent? = null,
    val syncStatus: SyncStatus = SyncStatus.InitialLoading,
    val message: String? = null,
    val isFromCache: Boolean = false,
    val operationError: String? = null,
) {
    val isRefreshing: Boolean get() = syncStatus == SyncStatus.Refreshing

    val attentionMessage: String?
        get() = if ((content?.blockedCount ?: 0) > 0) {
            content?.blockedMessage ?: "An offline change needs attention."
        } else {
            operationError
        }
}

internal fun ActionsUiState.waitingForNetwork(): ActionsUiState = copy(
    syncStatus = SyncStatus.Offline,
    message = if (content == null) {
        "Connect to download your account data."
    } else {
        "Offline. Showing saved data."
    },
)

internal data class RefreshPresentation(
    val syncStatus: SyncStatus,
    val connectionSignal: SyncConnectionSignal,
    val message: String?,
)

internal fun AllItems.refreshPresentation(): RefreshPresentation = RefreshPresentation(
    syncStatus = if (syncError == null) SyncStatus.Ready else SyncStatus.Offline,
    connectionSignal = if (syncError == null) SyncConnectionSignal.Connected else SyncConnectionSignal.Disconnected,
    message = syncError ?: blockedMessage ?: pendingSyncMessage(),
)

private fun AllItems.pendingSyncMessage(): String? =
    if (!isLocalOnly && pendingCount > 0) "Saved on this device. Changes are waiting to synchronize." else null

internal fun MutationOutcome.mutationPresentation(state: AuthenticationState): RefreshPresentation {
    val offline = !items.isLocalOnly && state !is AuthenticationState.SignedIn
    return items.copy(syncError = syncError ?: items.syncError).refreshPresentation().let { presentation ->
        if (offline) presentation.copy(
            syncStatus = SyncStatus.Offline,
            connectionSignal = if (state is AuthenticationState.InteractionRequired) SyncConnectionSignal.SignInRequired
            else SyncConnectionSignal.Disconnected,
            message = syncError ?: items.syncError ?: items.blockedMessage
            ?: "Saved on this device. Sign in or reconnect to synchronize.",
        ) else presentation
    }
}

internal fun AuthenticationState.sessionIdentity(): Pair<String, Long>? = when (this) {
    is AuthenticationState.SignedIn -> accountId to sessionGeneration
    is AuthenticationState.InteractionRequired -> accountId to sessionGeneration
    is AuthenticationState.Error -> accountId?.let { id -> sessionGeneration?.let { id to it } }
    else -> null
}

internal fun shouldInvalidateReminderSession(previous: Pair<String, Long>?, state: AuthenticationState): Boolean {
    if (previous == null) return false
    val incoming = state.sessionIdentity()
    return (incoming != null && incoming.first != previous.first) ||
            (state is AuthenticationState.SignedIn && incoming != previous) ||
            state == AuthenticationState.SignedOut
}

internal sealed interface ReminderOpenResult {
    data class Ready(val uiEpoch: Long) : ReminderOpenResult
    data object SignInRequired : ReminderOpenResult
    data object Unavailable : ReminderOpenResult
}

internal fun timelineEditedAction(current: Action, edit: TimelineEdit.Interval, zoneId: ZoneId): Action {
    checkTimelineTiming(edit, zoneId, action = current)
    if (!edit.changed) return current
    return current.copy(
        start = when {
            edit.start == edit.original.start -> current.start
            edit.original.isDateSpan -> edit.start.atZone(zoneId).toLocalDate().toString()
            else -> edit.start.toString()
        },
        duration = if (edit.mode == TimelineEditMode.Move) current.duration
        else Duration.between(edit.start, edit.endExclusive).toString(),
    )
}

internal fun timelineEditedEvent(current: Event, edit: TimelineEdit.Interval, zoneId: ZoneId): Event {
    checkTimelineTiming(edit, zoneId, event = current)
    if (!edit.changed) return current
    return current.copy(
        start = if (edit.start == edit.original.start) current.start else edit.start.toString(),
        duration = if (edit.mode == TimelineEditMode.Move) current.duration
        else Duration.between(edit.start, edit.endExclusive).toString(),
    )
}

internal fun timelineEditedMarker(current: Marker, edit: TimelineEdit.Interval, zoneId: ZoneId): Marker {
    checkTimelineTiming(edit, zoneId, marker = current)
    if (!edit.changed) return current
    val date = edit.start.atZone(zoneId).toLocalDate()
    val inclusiveEnd = edit.endExclusive.atZone(zoneId).toLocalDate().minusDays(1)
    return current.copy(
        date = date.toString(),
        endDate = if (inclusiveEnd == date && current.endDate == null) null else inclusiveEnd.toString(),
    )
}

internal fun timelineEditedSignal(current: Signal, edit: TimelineEdit.Signal): Signal {
    if (edit.original.id != current.id || edit.key != "signal:${current.id}" ||
        'T' !in current.datetime || parseSchedulePoint(current.datetime) != edit.original.instant
    ) throw WorkspaceEditConflictException()
    return if (edit.changed) current.copy(datetime = edit.instant.toString()) else current
}

private fun checkTimelineTiming(
    edit: TimelineEdit.Interval,
    zoneId: ZoneId,
    action: Action? = null,
    event: Event? = null,
    marker: Marker? = null,
) {
    val current = TimelineDataIndex.build(
        zoneId, listOfNotNull(action), listOfNotNull(event), listOfNotNull(marker), emptyList(),
        defaultActionDuration = Duration.between(edit.original.start, edit.original.endExclusive),
    ).intervals.singleOrNull()
    if (current == null || current.copy(
            title = edit.original.title,
            content = edit.original.content
        ) != edit.original
    ) {
        throw WorkspaceEditConflictException()
    }
    edit.validateDates(zoneId)
}

class MainViewModel(application: Application) : AndroidViewModel(application) {

    private val authentication = AppServices.authentication(application)
    private val network = AppServices.network(application)
    private val repository = AppServices.repository(application)
    private var sseJob: Job? = null
    private var pollJob: Job? = null
    private var foreground = false

    private enum class NetworkAvailability { Unknown, Available, Unavailable }

    private var networkAvailability = NetworkAvailability.Unknown
    private val networkAvailable get() = networkAvailability == NetworkAvailability.Available
    private var networkJob: Job? = null
    private val recovery = SyncRecovery(
        viewModelScope,
        onIdle = { _isResyncing.value = false },
        canRetry = { foreground && networkAvailable },
    )
    private var sseGeneration: Long? = null
    private var presentedAuthenticationIdentity: String? = null
    private var refreshRequestId = 0L

    private val sessionBoundaryInProgress = MutableStateFlow(false)
    private var uiSessionEpoch = 0L
    private var retiredSessionGeneration: Long? = null
    private var observedSession: Pair<String, Long>? = null
    private val reminderLifecycle = Mutex()
    private var reminderJob: Job? = null

    private var preparedReminderWorkspace: Pair<WorkspaceDescriptor.Remote, String>? = null

    val authenticationState: StateFlow<AuthenticationState> = combine(
        authentication.state,
        sessionBoundaryInProgress,
    ) { state, changing ->
        when {
            changing -> AuthenticationState.Initializing
            state is AuthenticationState.SignedIn && state.sessionGeneration == retiredSessionGeneration ->
                AuthenticationState.InteractionRequired(state.username, state.accountId, state.sessionGeneration)

            else -> state
        }
    }.stateIn(viewModelScope, SharingStarted.Eagerly, authentication.state.value)
    private val _initializing = MutableStateFlow(true)
    val initializing: StateFlow<Boolean> = _initializing.asStateFlow()
    val settingsStore = SettingsStore(application)

    private val _workspaceDescriptor = MutableStateFlow<WorkspaceDescriptor?>(null)
    val workspaceDescriptor: StateFlow<WorkspaceDescriptor?> = _workspaceDescriptor.asStateFlow()

    private val actionCache = mutableMapOf<String, Action>()
    private val eventCache = mutableMapOf<String, Event>()

    private val _uiState = MutableStateFlow(ActionsUiState())
    val uiState: StateFlow<ActionsUiState> = _uiState.asStateFlow()

    private val _isResyncing = MutableStateFlow(false)
    val isResyncing: StateFlow<Boolean> = _isResyncing.asStateFlow()

    private val _connectionStatus = MutableStateFlow(SyncConnectionStatus.Connecting)
    val connectionStatus: StateFlow<SyncConnectionStatus> = _connectionStatus.asStateFlow()
    private var connectionTracker = SyncConnectionTracker()
    private var connectionStatusJob: Job? = null

    private val _completing = MutableStateFlow<Set<String>>(emptySet())
    val completing: StateFlow<Set<String>> = _completing.asStateFlow()

    private val _showCreateSheet = MutableStateFlow(false)
    val showCreateSheet: StateFlow<Boolean> = _showCreateSheet.asStateFlow()

    private val _creatorSession = MutableStateFlow(CreatorSession(ItemCreatorState.initial()))
    val creatorSession: StateFlow<CreatorSession> = _creatorSession.asStateFlow()
    private val _creatorSubmission = MutableStateFlow<String?>(null)
    val creatorSubmission: StateFlow<String?> = _creatorSubmission.asStateFlow()

    private val _saving = MutableStateFlow(false)
    val saving: StateFlow<Boolean> = _saving.asStateFlow()

    private val _workspaceMergePrompt = MutableStateFlow<WorkspaceMergePrompt?>(null)
    val workspaceMergePrompt: StateFlow<WorkspaceMergePrompt?> =
        _workspaceMergePrompt.asStateFlow()

    init {
        updateConnectionStatus(SyncConnectionSignal.Checking)
        viewModelScope.launch {
            try {
                authentication.initialize()
            } finally {
                _initializing.value = false
            }
        }
        viewModelScope.launch {
            combine(authentication.state, sessionBoundaryInProgress) { state, changing -> state to changing }
                .collect { (state, changing) ->
                    if (changing || state != authentication.state.value) return@collect
                    val incomingSession = state.sessionIdentity()
                    if (shouldInvalidateReminderSession(observedSession, state)) {
                        sessionBoundaryInProgress.value = true
                        retireUiSession()
                        try {
                            withContext(NonCancellable) {
                                reminderLifecycle.withLock { LocalReminderRuntime.invalidate(getApplication()) }
                            }
                            clearAccountPresentation()
                            _uiState.value = ActionsUiState(syncStatus = SyncStatus.InitialLoading)
                            observedSession = incomingSession
                        } catch (cancelled: CancellationException) {
                            throw cancelled
                        } catch (error: Exception) {
                            retiredSessionGeneration = incomingSession?.second
                            observedSession = incomingSession
                            reportOperationFailure(error.message ?: "Could not deactivate reminders. Sign in again.")
                        } finally {
                            sessionBoundaryInProgress.value = false
                        }
                        currentCoroutineContext().ensureActive()
                        if (state != authentication.state.value) return@collect
                    }
                    if (observedSession != null && incomingSession != null && observedSession != incomingSession) {
                        retireUiSession()
                        _saving.value = false
                        _completing.value = emptySet()
                    }
                    if (incomingSession != null || state == AuthenticationState.SignedOut) {
                        observedSession = incomingSession
                    }
                    when (state) {
                        AuthenticationState.Initializing -> {
                            if (_uiState.value.content == null) {
                                _uiState.value = ActionsUiState(syncStatus = SyncStatus.InitialLoading)
                            }
                        }

                        AuthenticationState.SignedOut -> {
                            clearAccountPresentation()
                            updateConnectionStatus(SyncConnectionSignal.LocalOnly)
                            _uiState.value = ActionsUiState(syncStatus = SyncStatus.InitialLoading)
                            loadLocalWorkspace()
                        }

                        is AuthenticationState.InteractionRequired -> {
                            restoreOfflineAccount(state.accountId, state.sessionGeneration)
                            if (state != authentication.state.value) return@collect
                            stopServerChanges()
                            updateConnectionStatus(SyncConnectionSignal.SignInRequired)
                            _uiState.value = _uiState.value.copy(
                                syncStatus = if (_uiState.value.content == null) {
                                    SyncStatus.AuthenticationRequired
                                } else {
                                    SyncStatus.Offline
                                },
                                message = if (_uiState.value.content == null) {
                                    "Sign in again to sync."
                                } else {
                                    "Sign in again to sync. Showing saved data."
                                },
                            )
                        }

                        is AuthenticationState.SignedIn -> {
                            if (state.sessionGeneration == retiredSessionGeneration) {
                                stopServerChanges()
                                updateConnectionStatus(SyncConnectionSignal.SignInRequired)
                                _uiState.value = _uiState.value.copy(
                                    syncStatus = SyncStatus.AuthenticationRequired,
                                    message = "Sign in again before synchronizing this workspace.",
                                )
                                return@collect
                            }
                            if (presentedAuthenticationIdentity != state.accountId) {
                                retireUiSession()
                                clearAccountPresentation()
                                _uiState.value = ActionsUiState(syncStatus = SyncStatus.InitialLoading)
                            }
                            presentedAuthenticationIdentity = state.accountId
                            startServerSync(
                                accountIdentity = state.accountId,
                                sessionGeneration = state.sessionGeneration,
                            )
                        }

                        is AuthenticationState.Error -> {
                            if (state.accountId != null && state.sessionGeneration != null) {
                                restoreOfflineAccount(state.accountId, state.sessionGeneration)
                            }
                            if (state != authentication.state.value) return@collect
                            val content = _uiState.value.content
                            if (!state.retryable || state.recovery == AuthenticationRecovery.SignOut) stopServerChanges()
                            updateConnectionStatus(SyncConnectionSignal.Disconnected)
                            _uiState.value = ActionsUiState(
                                content = content,
                                syncStatus = if (content == null && !state.retryable) {
                                    SyncStatus.AuthenticationRequired
                                } else {
                                    SyncStatus.Offline
                                },
                                message = state.message,
                                isFromCache = _uiState.value.isFromCache,
                            )
                            if (state.retryable && foreground) recoverConnection()
                        }
                    }
                }
        }
        val initialReminderSettings = settingsStore.settings.value.reminderSettingsKey()
        viewModelScope.launch {
            var previous = initialReminderSettings
            settingsStore.settings.map { it.reminderSettingsKey() }.distinctUntilChanged().collect { current ->
                if (current != previous) {
                    previous = current
                    try {

                        reconcileReminders()
                    } catch (cancelled: CancellationException) {
                        throw cancelled
                    } catch (error: Exception) {
                        reportOperationFailure(error.message ?: "Could not update reminder settings")
                    }
                }
            }
        }
        viewModelScope.launch {
            AppServices.workspaceChanges.collect {
                val epoch = uiSessionEpoch
                val state = authentication.state.value
                val identity = currentAuthenticationIdentity()
                val generation = currentSessionGeneration()
                fun isCurrent(): Boolean = epoch == uiSessionEpoch && !sessionBoundaryInProgress.value &&
                        state == authentication.state.value &&
                        (state == AuthenticationState.SignedOut || (generation != null && isCurrentSession(generation)))
                if (!isCurrent()) return@collect
                try {
                    val cached = if (state == AuthenticationState.SignedOut) repository.loadLocal()
                    else repository.loadCached(identity ?: return@collect) ?: return@collect
                    if (isCurrent()) {
                        val previous = _uiState.value
                        installItems(cached, previous.syncStatus, previous.message, previous.isFromCache)
                        _uiState.value = _uiState.value.copy(operationError = previous.operationError)
                        refreshReminderCommandStatuses(::isCurrent)
                    }
                } catch (_: CancellationException) {
                    currentCoroutineContext().ensureActive()
                } catch (error: Exception) {
                    if (isCurrent()) {
                        reportOperationFailure(error.message ?: "Could not update cached data")
                    }
                }
            }
        }
    }

    private fun updateConnectionStatus(signal: SyncConnectionSignal) {
        connectionStatusJob?.cancel()
        val effectiveSignal = when (val state = authentication.state.value) {
            is AuthenticationState.InteractionRequired -> SyncConnectionSignal.SignInRequired
            is AuthenticationState.Error -> if (state.retryable) {
                SyncConnectionSignal.Disconnected
            } else SyncConnectionSignal.SignInRequired

            AuthenticationState.SignedOut -> SyncConnectionSignal.LocalOnly
            else -> signal
        }
        connectionTracker.observe(effectiveSignal, SystemClock.uptimeMillis())
        _connectionStatus.value = connectionTracker.status
        connectionStatusJob = connectionTracker.nextUpdateAtMillis?.let { deadline ->
            viewModelScope.launch {
                delay((deadline - SystemClock.uptimeMillis()).coerceAtLeast(0L))
                connectionTracker.advance(SystemClock.uptimeMillis())
                _connectionStatus.value = connectionTracker.status
            }
        }
    }

    val supportsSignIn: Boolean get() = authentication.supportsSignIn

    fun signIn(activity: Activity) {
        if (!supportsSignIn) return
        viewModelScope.launch {
            try {
                authentication.signIn(activity)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                reportOperationFailure("Could not open sign-in.")
            }
        }
    }

    fun signOut() = replaceSession { authentication.signOut() }

    private fun retireUiSession() {
        uiSessionEpoch += 1
        _workspaceMergePrompt.value = null
        preparedReminderWorkspace = null
        reminderJob?.cancel()
        stopServerChanges()
        refreshRequestId += 1
    }

    private fun replaceSession(operation: suspend () -> Unit) {
        if (sessionBoundaryInProgress.value) return
        sessionBoundaryInProgress.value = true
        retiredSessionGeneration = currentSessionGeneration()
        retireUiSession()
        viewModelScope.launch {
            try {
                withContext(NonCancellable) {
                    reminderLifecycle.withLock { LocalReminderRuntime.invalidate(getApplication()) }
                }
                currentCoroutineContext().ensureActive()
                operation()
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                reportOperationFailure(error.message ?: "Could not change the session. Sign in again to continue.")
            } finally {
                observedSession = authentication.state.value.sessionIdentity()
                sessionBoundaryInProgress.value = false
            }
        }
    }

    private suspend fun restoreOfflineAccount(accountId: String, generation: Long) {
        if (presentedAuthenticationIdentity != accountId) {
            clearAccountPresentation()
            _uiState.value = ActionsUiState(syncStatus = SyncStatus.InitialLoading)
        } else if (_uiState.value.content == null) {
            _uiState.value = _uiState.value.copy(syncStatus = SyncStatus.InitialLoading)
        }
        presentedAuthenticationIdentity = accountId
        val epoch = uiSessionEpoch
        try {
            val cached = repository.loadCached(accountId) ?: return
            if (epoch == uiSessionEpoch && !sessionBoundaryInProgress.value &&
                authentication.state.value.sessionIdentity() == (accountId to generation) &&
                authentication.state.value !is AuthenticationState.SignedIn
            ) {
                installItems(
                    cached, SyncStatus.Offline, "Sign in or reconnect to synchronize. Saved data remains editable.",
                    isFromCache = true, scheduleBackgroundWork = false
                )
            }
        } catch (_: CancellationException) {
            // A replaced repository session must not cancel the authentication observer itself.
            currentCoroutineContext().ensureActive()
        } catch (_: Exception) {
            if (epoch == uiSessionEpoch) reportOperationFailure("Could not read the saved account workspace.")
        }
    }

    private fun loadLocalWorkspace() {
        if (sessionBoundaryInProgress.value) return
        val epoch = uiSessionEpoch
        viewModelScope.launch {
            runCatching { repository.loadLocal() }
                .onSuccess { items ->
                    if (!sessionBoundaryInProgress.value && epoch == uiSessionEpoch &&
                        authentication.state.value == AuthenticationState.SignedOut
                    ) {
                        installItems(
                            all = items,
                            syncStatus = SyncStatus.Ready,
                            message = null,
                            isFromCache = false,
                        )
                    }
                }
                .onFailure { error ->
                    if (!sessionBoundaryInProgress.value && epoch == uiSessionEpoch &&
                        authentication.state.value == AuthenticationState.SignedOut
                    ) {
                        _uiState.value = ActionsUiState(
                            syncStatus = SyncStatus.AuthenticationRequired,
                            message = error.message ?: "Could not open local data",
                        )
                    }
                }
        }
    }

    fun onForeground() {
        if (foreground) return
        foreground = true
        networkAvailability = NetworkAvailability.Unknown
        networkJob = viewModelScope.launch {
            availableNetwork(getApplication()).catch {
                Log.w("MainViewModel", "Network monitoring unavailable (${it::class.simpleName})")
                emit(null)
            }.collect { network ->
                networkAvailability =
                    if (network != null) NetworkAvailability.Available else NetworkAvailability.Unavailable
                if (!networkAvailable) stopServerChanges()
                recoverConnection()
            }
        }
        pollJob = viewModelScope.launch {
            periodicallyRecoverSync { recoverConnection(restartStream = false) }
        }
        recoverConnection()
    }

    fun onBackground() {
        foreground = false
        networkAvailability = NetworkAvailability.Unknown
        pollJob?.cancel()
        pollJob = null
        networkJob?.cancel()
        networkJob = null
        stopServerChanges()
    }

    fun recoverConnection() = recoverConnection(restartStream = true)

    private fun recoverConnection(restartStream: Boolean) {
        if (!foreground || sessionBoundaryInProgress.value) return
        when (val state = authentication.state.value) {
            is AuthenticationState.SignedIn -> {
                if (observedSession != state.sessionIdentity() ||
                    !authentication.isCurrentSession(state.accountId, state.sessionGeneration)
                ) return
                if (restartStream) {
                    sseJob?.cancel()
                    sseJob = null
                }
                startServerSync(state.accountId, state.sessionGeneration)
            }

            is AuthenticationState.Error -> if (canRetryInitialization(state, networkAvailable)) {
                val epoch = authentication.sessionEpoch()
                recovery.request {
                    if (sessionBoundaryInProgress.value || authentication.sessionEpoch() != epoch ||
                        !canRetryInitialization(authentication.state.value, networkAvailable)
                    ) false
                    else {
                        authentication.initialize()
                        (authentication.state.value as? AuthenticationState.Error)?.retryable == true
                    }
                }
            }

            else -> Unit
        }
    }

    private fun startServerSync(accountIdentity: String, sessionGeneration: Long) {
        if (!foreground) return
        launchRefresh(accountIdentity, sessionGeneration)
        observeServerChanges(sessionGeneration)
    }

    private fun observeServerChanges(sessionGeneration: Long) {
        if (!networkAvailable) return
        if (sseJob?.isActive == true && sseGeneration == sessionGeneration) return
        sseJob?.cancel()
        sseJob = null
        sseGeneration = sessionGeneration
        val sseManager = SseManager(
            network.baseUrl, RetrofitClient(
                authentication.sessionBoundProvider(), network.baseUrl, requireAuthentication = true,
            ).httpClient
        )
        sseJob = viewModelScope.launch {
            sseManager.changeEvents()
                .retryWhen { cause, attempt ->
                    if (!networkAvailable || cause is CancellationException || !isCurrentSession(sessionGeneration) ||
                        authentication.state.value !is AuthenticationState.SignedIn ||
                        attempt >= 3 || !isRetryableSyncFailure(cause)
                    ) return@retryWhen false
                    val delayMs = syncRetryDelay(attempt)
                    Log.w(
                        "MainViewModel",
                        "SSE disconnected (${cause::class.simpleName}), retrying in ${delayMs}ms",
                    )
                    delay(delayMs)
                    networkAvailable && isCurrentSession(sessionGeneration) &&
                            authentication.state.value is AuthenticationState.SignedIn
                }
                .catch { error ->
                    if (error is CancellationException) throw error
                    Log.w(
                        "MainViewModel",
                        "SSE paused (${error::class.simpleName}); foreground/network/poll will recover"
                    )
                }
                .collect {
                    if (isCurrentSession(sessionGeneration)) loadActions()
                }
        }
    }

    private fun stopServerChanges() {
        sseJob?.cancel()
        recovery.stop()
        sseJob = null
        _isResyncing.value = false
        sseGeneration = null
    }

    private fun clearAccountPresentation() {
        stopServerChanges()
        connectionStatusJob?.cancel()
        connectionTracker = SyncConnectionTracker()
        _connectionStatus.value = connectionTracker.status
        refreshRequestId += 1
        presentedAuthenticationIdentity = null
        actionCache.clear()
        eventCache.clear()
        _completing.value = emptySet()
        _saving.value = false
        _creatorSubmission.value = null
        _showCreateSheet.value = false
        _workspaceMergePrompt.value = null
        _workspaceDescriptor.value = null
        preparedReminderWorkspace = null
    }

    fun resync() {
        if (sessionBoundaryInProgress.value || _isResyncing.value) return
        when (val state = authentication.state.value) {
            is AuthenticationState.SignedIn -> launchRefresh(
                state.accountId,
                state.sessionGeneration,
                manualResync = true,
            )

            is AuthenticationState.Error -> if (state.retryable) recoverConnection() else {
                _uiState.value = _uiState.value.copy(operationError = "Sign in again to resync.")
            }

            else -> _uiState.value = _uiState.value.copy(
                operationError = if (state == AuthenticationState.SignedOut) {
                    "This workspace is local only. Sign in to sync."
                } else {
                    "Sign in again to resync."
                },
            )
        }
    }

    fun loadActions() {
        if (sessionBoundaryInProgress.value) return
        when (val state = authentication.state.value) {
            AuthenticationState.SignedOut -> loadLocalWorkspace()
            is AuthenticationState.SignedIn -> launchRefresh(
                state.accountId,
                state.sessionGeneration,
            )

            is AuthenticationState.Error -> if (state.retryable) recoverConnection() else {
                _uiState.value = _uiState.value.copy(
                    syncStatus = if (_uiState.value.content == null) SyncStatus.AuthenticationRequired else SyncStatus.Offline,
                )
            }

            else -> _uiState.value = _uiState.value.copy(
                syncStatus = if (_uiState.value.content == null) {
                    SyncStatus.AuthenticationRequired
                } else {
                    SyncStatus.Offline
                },
            )
        }
    }

    internal suspend fun prepareReminderOpen(workspaceId: String, actionId: String): ReminderOpenResult {
        val epoch = uiSessionEpoch
        return withTimeoutOrNull(20_000L) {
            while (authentication.state.value == AuthenticationState.Initializing &&
                !sessionBoundaryInProgress.value && epoch == uiSessionEpoch
            ) delay(100)
            if (sessionBoundaryInProgress.value || epoch != uiSessionEpoch) {
                return@withTimeoutOrNull ReminderOpenResult.Unavailable
            }
            var requestedLoad = false
            while (true) {
                if (epoch != uiSessionEpoch || sessionBoundaryInProgress.value) {
                    return@withTimeoutOrNull ReminderOpenResult.Unavailable
                }
                val state = authenticationState.value
                if (state is AuthenticationState.Error && state.retryable) {
                    return@withTimeoutOrNull ReminderOpenResult.Unavailable
                }
                if (state !is AuthenticationState.SignedIn && state != AuthenticationState.SignedOut) {
                    return@withTimeoutOrNull ReminderOpenResult.SignInRequired
                }
                if (canOpenReminder(workspaceId, actionId, epoch)) {
                    return@withTimeoutOrNull ReminderOpenResult.Ready(epoch)
                }
                val loading = _uiState.value.syncStatus in setOf(SyncStatus.InitialLoading, SyncStatus.Refreshing)
                if (!loading) {
                    if (state == AuthenticationState.SignedOut && _workspaceDescriptor.value?.workspaceId != workspaceId) {
                        return@withTimeoutOrNull ReminderOpenResult.SignInRequired
                    }
                    if (requestedLoad) return@withTimeoutOrNull ReminderOpenResult.Unavailable
                    loadActions()
                    requestedLoad = true
                } else {
                    requestedLoad = true
                }
                delay(100)
            }
            @Suppress("UNREACHABLE_CODE")
            ReminderOpenResult.Unavailable
        } ?: ReminderOpenResult.Unavailable
    }

    internal fun canOpenReminder(workspaceId: String, actionId: String, epoch: Long): Boolean {
        if (sessionBoundaryInProgress.value || epoch != uiSessionEpoch ||
            _workspaceDescriptor.value?.workspaceId != workspaceId
        ) return false
        val state = authentication.state.value
        val eligible = when (state) {
            AuthenticationState.SignedOut -> _workspaceDescriptor.value is WorkspaceDescriptor.Local
            is AuthenticationState.SignedIn -> isCurrentSession(state.sessionGeneration) &&
                    observedSession == state.sessionIdentity() &&
                    authentication.isCurrentSession(state.accountId, state.sessionGeneration)

            else -> false
        }
        return eligible && _uiState.value.content?.actions?.any { it.id == actionId } == true
    }

    private fun launchRefresh(
        accountIdentity: String,
        sessionGeneration: Long,
        manualResync: Boolean = false,
    ) {
        if (!foreground || !isCurrentSession(sessionGeneration) ||
            !authentication.isCurrentSession(accountIdentity, sessionGeneration)
        ) return
        if (manualResync) _isResyncing.value = true
        recovery.request { refresh(accountIdentity, sessionGeneration, manualResync || _isResyncing.value) }
    }

    private suspend fun refresh(
        accountIdentity: String,
        sessionGeneration: Long,
        manualResync: Boolean = false,
    ): Boolean {
        currentCoroutineContext().ensureActive()
        if (!isCurrentSession(sessionGeneration) ||
            !authentication.isCurrentSession(accountIdentity, sessionGeneration)
        ) return false
        val requestId = ++refreshRequestId
        val epoch = uiSessionEpoch
        fun isCurrent(): Boolean = foreground && epoch == uiSessionEpoch && isCurrentSession(sessionGeneration) &&
                authentication.isCurrentSession(accountIdentity, sessionGeneration) && requestId == refreshRequestId
        if (_uiState.value.content == null) {
            try {
                val cached = repository.loadCached(accountIdentity)
                if (cached != null && isCurrent()) {
                    installItems(
                        cached,
                        SyncStatus.Refreshing,
                        null,
                        isFromCache = true,
                        scheduleBackgroundWork = false
                    )
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
            }
        }
        if (!isCurrent()) return false
        if (networkAvailability == NetworkAvailability.Unknown) return false
        if (!networkAvailable) {
            updateConnectionStatus(SyncConnectionSignal.Disconnected)
            _uiState.value = _uiState.value.waitingForNetwork()
            return false
        }
        var retryable = false
        updateConnectionStatus(SyncConnectionSignal.Checking)
        _uiState.value = _uiState.value.copy(
            syncStatus = if (_uiState.value.content == null) {
                SyncStatus.InitialLoading
            } else {
                SyncStatus.Refreshing
            },
            message = null,
        )
        runCatching {

            if (!isCurrent() || !networkAvailable) return false
            withTimeoutOrNull(45_000L) {
                if (manualResync) repository.resync(accountIdentity) else repository.fetchAll(accountIdentity)
            } ?: throw IOException("Synchronization timed out")
        }
            .onSuccess { all ->
                if (!isCurrent()) return@onSuccess
                _workspaceMergePrompt.value = null
                val presentation = all.refreshPresentation()
                updateConnectionStatus(presentation.connectionSignal)
                installItems(
                    all = all,
                    syncStatus = presentation.syncStatus,
                    message = presentation.message,
                    isFromCache = false,
                    scheduleBackgroundWork = !manualResync,
                )

                refreshReminderCommandStatuses(::isCurrent)
            }
            .onFailure { error ->
                if (error is CancellationException) throw error
                if (isCurrent()) {
                    retryable = isRetryableSyncFailure(error)
                    updateConnectionStatus(
                        if (error is WorkspaceMergeRequiredException) SyncConnectionSignal.Connected
                        else SyncConnectionSignal.Disconnected,
                    )
                    if (error is WorkspaceMergeRequiredException) {
                        _workspaceMergePrompt.value = WorkspaceMergePrompt(
                            localItemCount = error.localItemCount,
                            remoteItemCount = error.remoteItemCount,
                        )
                    }
                    _uiState.value = _uiState.value.copy(
                        syncStatus = SyncStatus.Offline,
                        message = error.message ?: "Could not refresh",
                        operationError = if (manualResync) {
                            "Could not resync. ${error.message ?: "Check your connection and try again."}"
                        } else {
                            _uiState.value.operationError
                        },
                    )
                }
            }
        return retryable && isCurrent()
    }

    private fun installItems(
        all: AllItems,
        syncStatus: SyncStatus,
        message: String?,
        isFromCache: Boolean,
        scheduleBackgroundWork: Boolean = true,
    ) {
        if (sessionBoundaryInProgress.value) return
        actionCache.clear()
        eventCache.clear()
        all.actions.forEach { actionCache[it.id] = it }
        all.events.forEach { eventCache[it.id] = it }

        val queuedActions = all.actions
            .filter { it.isQueued }
            .map { QueueItem.ActionItem(it) }
        val relevantEvents = all.events
            .filter { it.isNotFullyPassed }
            .map { QueueItem.EventItem(it) }
        val queueItems = (queuedActions + relevantEvents).sortedBy { it.sortInstant }

        _workspaceDescriptor.value = repository.activeWorkspaceDescriptor(currentAuthenticationIdentity())
        _uiState.value = ActionsUiState(
            content = ActionsContent(
                datasetId = all.datasetId,
                seq = all.seq,
                queueItems = queueItems,
                backlogged = all.actions.filter { it.isBacklogged },
                actions = all.actions,
                events = all.events,
                routines = all.routines,
                markers = all.markers,
                signals = all.signals,
                actionTemplates = all.actionTemplates,
                eventTemplates = all.eventTemplates,
                markerTemplates = all.markerTemplates,
                signalTemplates = all.signalTemplates,
                pendingCount = all.pendingCount,
                blockedCount = all.blockedCount,
                blockedMutationId = all.blockedMutationId,
                blockedMessage = all.blockedMessage,
                isLocalOnly = all.isLocalOnly,
            ),
            syncStatus = syncStatus,
            message = message,
            isFromCache = isFromCache,
        )
        reconcileReminders(scheduleBackgroundWork)
    }

    fun reconcileReminders(scheduleBackgroundWork: Boolean = true) {
        if (sessionBoundaryInProgress.value) return
        val descriptor = _workspaceDescriptor.value ?: return
        val content = _uiState.value.content ?: return
        val state = authentication.state.value
        val epoch = uiSessionEpoch
        val settings = settingsStore.settings.value
        fun isCurrent(): Boolean = epoch == uiSessionEpoch && !sessionBoundaryInProgress.value &&
                authentication.state.value == state && descriptor == _workspaceDescriptor.value &&
                descriptor == repository.activeWorkspaceDescriptor(currentAuthenticationIdentity()) &&
                content == _uiState.value.content && settings == settingsStore.settings.value &&
                when (descriptor) {
                    is WorkspaceDescriptor.Local -> state == AuthenticationState.SignedOut && content.isLocalOnly
                    is WorkspaceDescriptor.Remote -> canReconcileRemoteReminders(state, descriptor.accountId) &&
                            state.sessionIdentity()?.second?.let(::isCurrentSession) == true && !content.isLocalOnly &&
                            observedSession == state.sessionIdentity()
                }
        if (!isCurrent()) return
        reminderJob?.cancel()
        reminderJob = viewModelScope.launch {
            try {
                reminderLifecycle.withLock {
                    if (!isCurrent()) return@withLock
                    if (descriptor is WorkspaceDescriptor.Remote && state is AuthenticationState.SignedIn &&
                        preparedReminderWorkspace != (descriptor to content.datasetId)
                    ) {
                        LocalReminderRuntime.onWorkspaceReady(
                            getApplication(), descriptor, content.datasetId, state.accountId,
                            scheduleBackgroundWork = scheduleBackgroundWork,
                        )
                        if (isCurrent() && scheduleBackgroundWork) {
                            preparedReminderWorkspace = descriptor to content.datasetId
                        }
                    }
                    currentCoroutineContext().ensureActive()
                    if (!isCurrent()) return@withLock
                    withContext(Dispatchers.IO) {
                        ReminderScheduler.reconcile(getApplication(), descriptor, content.actions, settings)
                    }
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                if (isCurrent()) reportOperationFailure(error.message ?: "Could not update reminders")
            }
        }
    }

    fun removeOfflineAccountData() {
        val identity = currentAuthenticationIdentity() ?: return
        replaceSession {
            _saving.value = true
            try {
                repository.removeOfflineAccountData(identity)
                clearAccountPresentation()
                _uiState.value = ActionsUiState(syncStatus = SyncStatus.InitialLoading)
                authentication.signOut()
            } finally {
                _saving.value = false
            }
        }
    }

    fun keepLocalDataSeparate() {
        val signedIn = authentication.state.value as? AuthenticationState.SignedIn ?: return
        viewModelScope.launch {
            if (!isCurrentSession(signedIn.sessionGeneration)) return@launch
            _saving.value = true
            runCatching { repository.keepLocalDataSeparate(signedIn.accountId) }
                .onSuccess { items ->
                    if (isCurrentSession(signedIn.sessionGeneration)) {
                        _workspaceMergePrompt.value = null
                        installItems(items, SyncStatus.Ready, null, isFromCache = false)
                    }
                }
                .onFailure { error ->
                    if (isCurrentSession(signedIn.sessionGeneration)) {
                        reportOperationFailure(error.message ?: "Could not keep workspaces separate")
                    }
                }
            if (isCurrentSession(signedIn.sessionGeneration)) _saving.value = false
        }
    }

    fun mergeLocalData() {
        val signedIn = authentication.state.value as? AuthenticationState.SignedIn ?: return
        viewModelScope.launch {
            if (!isCurrentSession(signedIn.sessionGeneration)) return@launch
            _saving.value = true
            runCatching { repository.mergeLocalData(signedIn.accountId) }
                .onSuccess { outcome ->
                    if (isCurrentSession(signedIn.sessionGeneration)) {
                        _workspaceMergePrompt.value = null
                        installMutationOutcome(outcome)
                    }
                }
                .onFailure { error ->
                    if (isCurrentSession(signedIn.sessionGeneration)) {
                        reportOperationFailure(error.message ?: "Could not import local items")
                    }
                }
            if (isCurrentSession(signedIn.sessionGeneration)) _saving.value = false
        }
    }

    fun refreshPipeline() {
        launchForCurrentSession { sessionGeneration ->
            _saving.value = true
            runCatching { repository.refreshPipeline() }
                .onSuccess {
                    if (isCurrentSession(sessionGeneration)) loadActions()
                }
                .onFailure { error ->
                    if (isCurrentSession(sessionGeneration)) {
                        reportOperationFailure(error.message ?: "Could not refresh the schedule")
                    }
                }
            if (isCurrentSession(sessionGeneration)) _saving.value = false
        }
    }

    fun expeditePipeline() {
        launchForCurrentSession { sessionGeneration ->
            _saving.value = true
            runCatching { repository.expeditePipeline() }
                .onSuccess {
                    if (isCurrentSession(sessionGeneration)) loadActions()
                }
                .onFailure { error ->
                    if (isCurrentSession(sessionGeneration)) {
                        reportOperationFailure(error.message ?: "Could not bring work forward")
                    }
                }
            if (isCurrentSession(sessionGeneration)) _saving.value = false
        }
    }

    fun completeAction(id: String) {
        val action = resolveAction(id)?.takeIf { it.completion == null } ?: return
        launchForCurrentWorkspace { workspaceSession ->
            if (id in _completing.value) return@launchForCurrentWorkspace
            _completing.value = _completing.value + id
            runCatching {
                repository.completeAction(
                    workspaceSession.authenticationIdentity,
                    action,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) installMutationOutcome(outcome)
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) reportOperationFailure(
                        error.message ?: "Complete failed",
                    )
                }
            if (isCurrentWorkspace(workspaceSession)) {
                _completing.value = _completing.value - id
            }
        }
    }

    fun openCreateSheet(type: CreatorItemType = CreatorItemType.Action) {
        presentCreator(ItemCreatorState.initial().selectType(type))
    }

    private fun presentCreator(initial: ItemCreatorState) {
        _creatorSession.value = CreatorSession(initial)
        _showCreateSheet.value = true
    }

    fun openInlineCreator(
        source: InlineCreatorSource,
        title: String,
        date: LocalDate,
        zone: ZoneId = ZoneId.systemDefault()
    ) {
        if (_saving.value) return
        _creatorSession.value = CreatorSession.fromInline(source, title, date, zone)
        _showCreateSheet.value = true
    }

    fun saveActionTemplate(
        templateId: String,
        title: String,
        content: String?,
        naiveTime: String?,
        duration: String?,
        onSuccess: () -> Unit,
    ) {
        val current = actionTemplate(templateId) ?: return
        val updated = current.copy(
            title = title,
            content = content,
            naiveTime = naiveTime,
            duration = duration,
        )
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateActionTemplate(
                    workspaceSession.authenticationIdentity,
                    updated,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Save failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun deleteActionTemplate(templateId: String, onSuccess: () -> Unit) {
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.deleteActionTemplate(
                    workspaceSession.authenticationIdentity,
                    templateId,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Delete failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun saveEventTemplate(
        templateId: String,
        title: String,
        content: String?,
        duration: String,
        onSuccess: () -> Unit,
    ) {
        val current = eventTemplate(templateId) ?: return
        val updated = current.copy(title = title, content = content, duration = duration)
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateEventTemplate(
                    workspaceSession.authenticationIdentity,
                    updated,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Save failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun deleteEventTemplate(templateId: String, onSuccess: () -> Unit) {
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.deleteEventTemplate(
                    workspaceSession.authenticationIdentity,
                    templateId,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Delete failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun actionTemplate(templateId: String): ActionTemplate? =
        _uiState.value.content?.actionTemplates?.firstOrNull { it.id == templateId }

    fun eventTemplate(templateId: String): EventTemplate? =
        _uiState.value.content?.eventTemplates?.firstOrNull { it.id == templateId }

    fun useActionTemplate(templateId: String) {
        val template = _uiState.value.content?.actionTemplates?.firstOrNull { it.id == templateId }
            ?: return
        val preferredTime = template.naiveTime?.let { value ->
            runCatching { LocalTime.parse(value) }.getOrNull()
        }
        val duration = parseIsoDurationSeconds(template.duration)
            ?.takeIf { it > 0 }
            ?.let(Duration::ofSeconds)
        presentCreator(
            ItemCreatorState.initial().copy(
                selectedType = CreatorItemType.Action,
                title = template.title,
                notes = template.content.orEmpty(),
                schedule = CreatorSchedule(time = preferredTime),
                duration = duration,
            )
        )
    }

    fun useEventTemplate(templateId: String) {
        val template = _uiState.value.content?.eventTemplates?.firstOrNull { it.id == templateId }
            ?: return
        val initial = ItemCreatorState.initial().selectType(CreatorItemType.Event)
        val duration = parseIsoDurationSeconds(template.duration)
            ?.takeIf { it > 0 }
            ?.let(Duration::ofSeconds)
        presentCreator(
            initial.copy(
                title = template.title,
                notes = template.content.orEmpty(),
                duration = duration ?: initial.duration,
            )
        )
    }

    fun openCreateSheetOnDate(
        date: LocalDate,
        zoneId: ZoneId = ZoneId.systemDefault(),
    ) {
        presentCreator(
            ItemCreatorState.initial(zoneId).copy(
                schedule = CreatorSchedule(date = date, zoneId = zoneId),
                actionQueued = true,
            )
        )
    }

    fun openCreateSheetAt(
        date: LocalDate,
        time: LocalTime,
        zoneId: ZoneId = ZoneId.systemDefault(),
    ) {
        presentCreator(
            ItemCreatorState.initial(zoneId).copy(
                schedule = CreatorSchedule(date = date, time = time, zoneId = zoneId),
                actionQueued = true,
                actionPinned = true,
            )
        )
    }

    fun closeCreateSheet() {
        _showCreateSheet.value = false
    }

    fun createItem(
        request: CreateItemRequest,
        onSuccess: (() -> Unit)? = null,
        creationId: String = UuidV7.newString(),
    ) {
        if (_saving.value) return
        val submittingSession = _creatorSession.value.takeIf {
            _showCreateSheet.value && it.creationId == creationId
        }?.id
        launchForCurrentWorkspace { workspaceSession ->
            if (_saving.value) return@launchForCurrentWorkspace
            _saving.value = true
            _creatorSubmission.value = submittingSession
            _uiState.value = _uiState.value.copy(operationError = null)
            try {
                val outcome = repository.createItem(
                    workspaceSession.authenticationIdentity,
                    request,
                    attemptSync = false,
                    creationId = creationId,
                )
                if (isCurrentWorkspace(workspaceSession)) {
                    installMutationOutcome(outcome)
                    if (onSuccess == null && _creatorSession.value.id == submittingSession) _showCreateSheet.value =
                        false
                    onSuccess?.invoke()
                }
            } catch (_: CancellationException) {
                currentCoroutineContext().ensureActive()
            } catch (error: Exception) {
                if (isCurrentWorkspace(workspaceSession)) reportOperationFailure(error.message ?: "Create failed")
            } finally {
                if (_creatorSubmission.value == submittingSession) _creatorSubmission.value = null
                if (isCurrentWorkspace(workspaceSession)) _saving.value = false
            }
        }
    }

    fun applyTimelineEdit(edit: TimelineEdit, onFinished: (Boolean) -> Unit) {
        val session = currentWorkspaceSession()
        val workspace = _workspaceDescriptor.value
        val datasetId = _uiState.value.content?.datasetId
        if (session == null || workspace == null || datasetId == null || _saving.value) {
            reportOperationFailure(if (_saving.value) "Another save is in progress." else "The workspace is not ready for editing.")
            onFinished(false)
            return
        }
        val zoneId = ZoneId.systemDefault()
        fun isCurrent() = isCurrentWorkspace(session) && _workspaceDescriptor.value == workspace &&
                repository.activeWorkspaceDescriptor(session.authenticationIdentity) == workspace &&
                _uiState.value.content?.datasetId == datasetId && ZoneId.systemDefault() == zoneId

        _saving.value = true
        _uiState.value = _uiState.value.copy(operationError = null)
        viewModelScope.launch(start = CoroutineStart.UNDISPATCHED) {
            var success = false
            try {
                currentCoroutineContext().ensureActive()
                check(isCurrent()) { "The workspace changed. Please try the edit again." }
                val items = if (session.authenticationIdentity == null) repository.loadLocal()
                else repository.loadCached(session.authenticationIdentity)
                    ?: error("No saved workspace is available. Connect before editing.")
                check(isCurrent() && items.datasetId == datasetId) { "The workspace changed. Please try the edit again." }
                val outcome = persistTimelineEdit(edit, session, items, zoneId)
                check(isCurrent()) { "The workspace changed before the edit could be displayed." }
                installMutationOutcome(outcome)
                success = true
            } catch (cancelled: CancellationException) {
                if (isCurrentWorkspace(session)) reportOperationFailure("Timeline edit was interrupted. Please try again.")
                throw cancelled
            } catch (error: Exception) {
                if (isCurrentWorkspace(session)) reportOperationFailure(
                    error.message ?: "Could not save timeline edit."
                )
            } finally {
                if (isCurrentWorkspace(session)) _saving.value = false
                onFinished(success)
            }
        }
    }

    private suspend fun persistTimelineEdit(
        edit: TimelineEdit,
        session: WorkspaceSession,
        items: AllItems,
        zoneId: ZoneId,
    ): MutationOutcome = when (edit) {
        is TimelineEdit.Interval -> when (edit.original.kind) {
            TimelineItemKind.Action -> {
                val current =
                    items.actions.firstOrNull { it.id == edit.original.id } ?: throw WorkspaceEditConflictException()
                val updated = timelineEditedAction(current, edit, zoneId)
                if (!edit.changed) MutationOutcome(items) else repository.updateAction(
                    session.authenticationIdentity, updated, attemptSync = false, expectedAction = current,
                )
            }

            TimelineItemKind.Event -> {
                val current =
                    items.events.firstOrNull { it.id == edit.original.id } ?: throw WorkspaceEditConflictException()
                val updated = timelineEditedEvent(current, edit, zoneId)
                if (!edit.changed) MutationOutcome(items) else repository.updateEvent(
                    session.authenticationIdentity, updated, attemptSync = false, expectedEvent = current,
                )
            }

            TimelineItemKind.Marker -> {
                val current =
                    items.markers.firstOrNull { it.id == edit.original.id } ?: throw WorkspaceEditConflictException()
                val updated = timelineEditedMarker(current, edit, zoneId)
                if (!edit.changed) MutationOutcome(items) else repository.updateMarker(
                    session.authenticationIdentity, updated, attemptSync = false, expectedMarker = current,
                )
            }
        }

        is TimelineEdit.Signal -> {
            val current =
                items.signals.firstOrNull { it.id == edit.original.id } ?: throw WorkspaceEditConflictException()
            val updated = timelineEditedSignal(current, edit)
            if (!edit.changed) MutationOutcome(items) else repository.updateSignal(
                session.authenticationIdentity, updated, attemptSync = false, expectedSignal = current,
            )
        }
    }

    fun saveAction(
        actionId: String,
        title: String,
        content: String?,
        start: String?,
        duration: String?,
        queued: Boolean,
        pinned: Boolean,
        onSuccess: () -> Unit,
    ) {
        val current = resolveAction(actionId) ?: return
        val updated = current.copy(
            title = title,
            content = content,
            start = start,
            duration = duration,
            queued = queued,
            pinned = pinned && queued && start?.contains('T') == true,
        )
        saveActionObject(updated, expectedAction = current, onSuccess = onSuccess)
    }

    private fun saveActionObject(action: Action, expectedAction: Action?, onSuccess: () -> Unit) {
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateAction(
                    workspaceSession.authenticationIdentity,
                    action,
                    attemptSync = workspaceSession.canAttemptSync,
                    expectedAction = expectedAction,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Save failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun queueActionsAnyTime(actionIds: Set<String>, onSuccess: () -> Unit) {
        val originals = actionIds.mapNotNull(::resolveAction)
        val actions = originals.map { action ->
            action.copy(queued = true, pinned = false, start = null)
        }
        if (actions.isEmpty()) return
        mutateActionBatch(actions, originals, onSuccess, "Could not queue selected actions")
    }

    fun duplicateActions(actionIds: Set<String>, onSuccess: () -> Unit) {
        val actions = actionIds.mapNotNull(::resolveAction).map { source ->
            val id = UuidV7.newString()
            source.copy(
                id = id,
                recurrenceId = id,
                completion = null,
                sourceProvider = null,
                sourceExternalId = null,
            )
        }
        if (actions.isEmpty()) return
        mutateActionBatch(actions, emptyList(), onSuccess, "Could not duplicate selected actions")
    }

    private fun mutateActionBatch(
        actions: List<Action>,
        expectedActions: List<Action>,
        onSuccess: () -> Unit,
        failureMessage: String,
    ) {
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateActions(
                    workspaceSession.authenticationIdentity,
                    actions,
                    attemptSync = workspaceSession.canAttemptSync,
                    expectedActions = expectedActions,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: failureMessage)
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun deleteActions(actionIds: Set<String>, onSuccess: () -> Unit) {
        if (actionIds.isEmpty()) return
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.deleteActions(
                    workspaceSession.authenticationIdentity,
                    actionIds.toList(),
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Could not delete selected actions")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun duplicateAction(actionId: String, onSuccess: () -> Unit) {
        val source = resolveAction(actionId) ?: return
        val id = UuidV7.newString()
        val duplicate = source.copy(
            id = id,
            recurrenceId = id,
            completion = null,
            sourceProvider = null,
            sourceExternalId = null,
        )
        saveActionObject(duplicate, expectedAction = null, onSuccess = onSuccess)
    }

    fun restoreAction(actionId: String, onSuccess: () -> Unit) {
        val source = resolveAction(actionId) ?: return
        if (source.completion == null) return
        saveActionObject(
            source.copy(
                completion = null,
                recurrence = null,
                queued = false,
                pinned = false,
            ),
            expectedAction = source,
            onSuccess = onSuccess,
        )
    }

    fun saveActionForReuse(actionId: String) {
        val source = resolveAction(actionId) ?: return
        val preferredTime = source.start
            ?.takeIf { 'T' in it }
            ?.let(::parseSchedulePoint)
            ?.atZone(ZoneId.systemDefault())
            ?.toLocalTime()
            ?.format(DateTimeFormatter.ISO_LOCAL_TIME)
        val template = ActionTemplate(
            id = UuidV7.newString(),
            title = source.title,
            content = source.content,
            naiveTime = preferredTime,
            duration = source.duration,
            recurrence = source.recurrence,
        )
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateActionTemplate(
                    workspaceSession.authenticationIdentity,
                    template,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) installMutationOutcome(outcome)
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Could not save for reuse")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun deleteAction(actionId: String, onSuccess: () -> Unit) {
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.deleteAction(
                    workspaceSession.authenticationIdentity,
                    actionId,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Delete failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun saveEvent(
        eventId: String,
        title: String,
        content: String?,
        start: String,
        duration: String,
        onSuccess: () -> Unit,
    ) {
        val current = resolveEvent(eventId) ?: return
        val updated = current.copy(
            title = title,
            content = content,
            start = start,
            duration = duration,
        )
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateEvent(
                    workspaceSession.authenticationIdentity,
                    updated,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Save failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun duplicateEvent(eventId: String, onSuccess: () -> Unit) {
        val source = resolveEvent(eventId) ?: return
        val id = UuidV7.newString()
        val duplicate = source.copy(
            id = id,
            lineageId = id,
            sourceProvider = null,
            sourceExternalId = null,
        )
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateEvent(
                    workspaceSession.authenticationIdentity,
                    duplicate,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Duplicate failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun saveEventForReuse(eventId: String) {
        val source = resolveEvent(eventId) ?: return
        val template = source.asTemplate(UuidV7.newString())
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateEventTemplate(
                    workspaceSession.authenticationIdentity,
                    template,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) installMutationOutcome(outcome)
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Could not save for reuse")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun deleteEvent(eventId: String, onSuccess: () -> Unit) {
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.deleteEvent(
                    workspaceSession.authenticationIdentity,
                    eventId,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Delete failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun saveRoutine(
        routineId: String,
        title: String,
        content: String?,
        target: String?,
        steps: List<com.example.subroutine_simple.data.models.RoutineStep>,
        onSuccess: () -> Unit,
    ) {
        val current = resolveRoutine(routineId) ?: return
        val updated = current.copy(
            title = title,
            content = content,
            target = target,
            steps = steps,
        )
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateRoutine(
                    workspaceSession.authenticationIdentity,
                    updated,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Save failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun duplicateRoutine(routineId: String, onSuccess: () -> Unit) {
        val source = resolveRoutine(routineId) ?: return
        val duplicate = source.copy(id = UuidV7.newString())
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateRoutine(
                    workspaceSession.authenticationIdentity,
                    duplicate,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Duplicate failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun deleteRoutine(routineId: String, onSuccess: () -> Unit) {
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.deleteRoutine(
                    workspaceSession.authenticationIdentity,
                    routineId,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Delete failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun instantiateRoutine(routineId: String) {
        launchForCurrentSession { sessionGeneration ->
            _saving.value = true
            runCatching { repository.instantiateRoutine(routineId, startTime = null) }
                .onSuccess {
                    if (isCurrentSession(sessionGeneration)) loadActions()
                }
                .onFailure { error ->
                    if (isCurrentSession(sessionGeneration)) {
                        reportOperationFailure(error.message ?: "Could not start routine")
                    }
                }
            if (isCurrentSession(sessionGeneration)) _saving.value = false
        }
    }

    fun saveMarker(
        markerId: String,
        title: String,
        content: String?,
        date: String,
        endDate: String?,
        onSuccess: () -> Unit,
    ) {
        val current = resolveMarker(markerId) ?: return
        val updated = current.copy(
            title = title,
            content = content,
            date = date,
            endDate = endDate,
        )
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateMarker(
                    workspaceSession.authenticationIdentity,
                    updated,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Save failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun duplicateMarker(markerId: String, onSuccess: () -> Unit) {
        val source = resolveMarker(markerId) ?: return
        val id = UuidV7.newString()
        val duplicate = source.copy(
            id = id,
            lineageId = id,
            sourceProvider = null,
            sourceExternalId = null,
        )
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateMarker(
                    workspaceSession.authenticationIdentity,
                    duplicate,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Duplicate failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun deleteMarker(markerId: String, onSuccess: () -> Unit) {
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.deleteMarker(
                    workspaceSession.authenticationIdentity,
                    markerId,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Delete failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun saveSignal(
        signalId: String,
        title: String,
        content: String?,
        datetime: String,
        onSuccess: () -> Unit,
    ) {
        val current = resolveSignal(signalId) ?: return
        val updated = current.copy(title = title, content = content, datetime = datetime)
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateSignal(
                    workspaceSession.authenticationIdentity,
                    updated,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Save failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun duplicateSignal(signalId: String, onSuccess: () -> Unit) {
        val source = resolveSignal(signalId) ?: return
        val id = UuidV7.newString()
        val duplicate = source.copy(id = id, lineageId = id)
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.updateSignal(
                    workspaceSession.authenticationIdentity,
                    duplicate,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Duplicate failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun deleteSignal(signalId: String, onSuccess: () -> Unit) {
        launchForCurrentWorkspace { workspaceSession ->
            _saving.value = true
            runCatching {
                repository.deleteSignal(
                    workspaceSession.authenticationIdentity,
                    signalId,
                    attemptSync = workspaceSession.canAttemptSync,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        installMutationOutcome(outcome)
                        onSuccess()
                    }
                }
                .onFailure { error ->
                    if (isCurrentWorkspace(workspaceSession)) {
                        reportOperationFailure(error.message ?: "Delete failed")
                    }
                }
            if (isCurrentWorkspace(workspaceSession)) _saving.value = false
        }
    }

    fun routine(routineId: String): Routine? = resolveRoutine(routineId)

    fun marker(markerId: String): Marker? = resolveMarker(markerId)

    fun signal(signalId: String): Signal? = resolveSignal(signalId)

    fun useRemoteConflict() {
        val authenticationIdentity = currentAuthenticationIdentity() ?: return
        val mutationId = _uiState.value.content?.blockedMutationId ?: return
        val epoch = uiSessionEpoch
        launchForCurrentSession { sessionGeneration ->
            fun isCurrent(): Boolean = epoch == uiSessionEpoch && isCurrentSession(sessionGeneration) &&
                    authenticationIdentity == currentAuthenticationIdentity()
            if (!isCurrent()) return@launchForCurrentSession
            _saving.value = true
            runCatching { repository.useRemoteConflict(authenticationIdentity, mutationId) }
                .onSuccess { outcome ->
                    if (isCurrent()) {
                        installMutationOutcome(outcome)
                        refreshReminderCommandStatuses(::isCurrent)
                    }
                }
                .onFailure { error ->
                    if (isCurrent()) {
                        reportOperationFailure(error.message ?: "Could not use the server version")
                    }
                }
            if (isCurrent()) _saving.value = false
        }
    }

    fun keepLocalConflict() {
        val authenticationIdentity = currentAuthenticationIdentity() ?: return
        val mutationId = _uiState.value.content?.blockedMutationId ?: return
        val epoch = uiSessionEpoch
        launchForCurrentSession { sessionGeneration ->
            fun isCurrent(): Boolean = epoch == uiSessionEpoch && isCurrentSession(sessionGeneration) &&
                    authenticationIdentity == currentAuthenticationIdentity()
            if (!isCurrent()) return@launchForCurrentSession
            _saving.value = true
            runCatching {
                repository.keepLocalConflict(
                    authenticationIdentity,
                    mutationId,
                    attemptSync = authentication.state.value is AuthenticationState.SignedIn,
                )
            }
                .onSuccess { outcome ->
                    if (isCurrent()) {
                        installMutationOutcome(outcome)
                        refreshReminderCommandStatuses(::isCurrent)
                    }
                }
                .onFailure { error ->
                    if (isCurrent()) {
                        reportOperationFailure(error.message ?: "Could not retry the local change")
                    }
                }
            if (isCurrent()) _saving.value = false
        }
    }

    fun action(actionId: String): Action? = resolveAction(actionId)

    fun event(eventId: String): Event? = resolveEvent(eventId)

    private fun installMutationOutcome(outcome: MutationOutcome) {
        if (sessionBoundaryInProgress.value) return
        val pending = !outcome.items.isLocalOnly && outcome.items.pendingCount > 0
        val presentation = outcome.mutationPresentation(authentication.state.value)
        installItems(
            all = outcome.items,
            syncStatus = presentation.syncStatus,
            message = presentation.message,
            isFromCache = false,
        )
        if (pending && outcome.items.blockedCount == 0) BackgroundSyncWorker.requestImmediate(getApplication())
        AppServices.notifyWorkspaceChanged()
    }

    private suspend fun refreshReminderCommandStatuses(isCurrent: () -> Boolean) {
        try {
            reminderLifecycle.withLock {
                currentCoroutineContext().ensureActive()
                if (isCurrent()) LocalReminderRuntime.refreshCommandStatuses(getApplication())
            }
        } catch (_: CancellationException) {
            currentCoroutineContext().ensureActive()
        } catch (error: Exception) {
            if (isCurrent()) reportOperationFailure(error.message ?: "Could not refresh reminder command feedback")
        }
    }

    private fun reportOperationFailure(message: String) {
        _uiState.value = _uiState.value.copy(message = message, operationError = message)
    }

    private data class WorkspaceSession(
        val authenticationIdentity: String?,
        val sessionGeneration: Long?,
        val canAttemptSync: Boolean,
        val uiEpoch: Long,
    )

    private fun launchForCurrentWorkspace(block: suspend (WorkspaceSession) -> Unit) {
        val session = currentWorkspaceSession() ?: return
        viewModelScope.launch { if (isCurrentWorkspace(session)) block(session) }
    }

    private fun currentWorkspaceSession(): WorkspaceSession? {
        if (sessionBoundaryInProgress.value ||
            (retiredSessionGeneration != null && retiredSessionGeneration == currentSessionGeneration())
        ) return null
        return when (val state = authentication.state.value) {
            AuthenticationState.SignedOut -> WorkspaceSession(
                authenticationIdentity = null,
                sessionGeneration = null,
                canAttemptSync = false,
                uiEpoch = uiSessionEpoch,
            )

            is AuthenticationState.SignedIn -> WorkspaceSession(
                authenticationIdentity = state.accountId,
                sessionGeneration = state.sessionGeneration,
                canAttemptSync = true,
                uiEpoch = uiSessionEpoch,
            )

            is AuthenticationState.InteractionRequired -> WorkspaceSession(
                authenticationIdentity = state.accountId,
                sessionGeneration = state.sessionGeneration,
                canAttemptSync = false,
                uiEpoch = uiSessionEpoch,
            )

            is AuthenticationState.Error -> {
                val accountId = state.accountId ?: return null
                WorkspaceSession(
                    authenticationIdentity = accountId,
                    sessionGeneration = state.sessionGeneration,
                    canAttemptSync = false,
                    uiEpoch = uiSessionEpoch,
                )
            }

            else -> null
        }
    }

    private fun isCurrentWorkspace(session: WorkspaceSession): Boolean =
        !sessionBoundaryInProgress.value && session.uiEpoch == uiSessionEpoch && when (
            val state = authentication.state.value
        ) {
            AuthenticationState.SignedOut -> session.authenticationIdentity == null
            is AuthenticationState.SignedIn ->
                state.accountId == session.authenticationIdentity &&
                        state.sessionGeneration == session.sessionGeneration

            is AuthenticationState.InteractionRequired ->
                state.accountId == session.authenticationIdentity &&
                        state.sessionGeneration == session.sessionGeneration

            is AuthenticationState.Error ->
                state.accountId != null && state.accountId == session.authenticationIdentity &&
                        state.sessionGeneration == session.sessionGeneration

            else -> false
        }

    private fun launchForCurrentSession(block: suspend (Long) -> Unit) {
        val sessionGeneration = currentSessionGeneration() ?: return
        viewModelScope.launch { if (isCurrentSession(sessionGeneration)) block(sessionGeneration) }
    }

    private fun currentAuthenticationIdentity(): String? = when (val state = authentication.state.value) {
        is AuthenticationState.SignedIn -> state.accountId
        is AuthenticationState.InteractionRequired -> state.accountId
        is AuthenticationState.Error -> state.accountId
        else -> null
    }

    private fun currentSessionGeneration(): Long? = when (val state = authentication.state.value) {
        is AuthenticationState.SignedIn -> state.sessionGeneration
        is AuthenticationState.InteractionRequired -> state.sessionGeneration
        is AuthenticationState.Error -> state.sessionGeneration
        else -> null
    }

    private fun isCurrentSession(sessionGeneration: Long): Boolean =
        !sessionBoundaryInProgress.value && sessionGeneration != retiredSessionGeneration &&
                currentSessionGeneration() == sessionGeneration

    private fun resolveEvent(eventId: String?): Event? {
        if (eventId == null) return null
        return _uiState.value.content?.events?.firstOrNull { it.id == eventId }
            ?: eventCache[eventId]
    }

    private fun resolveAction(actionId: String?): Action? {
        if (actionId == null) return null
        return _uiState.value.content?.actions?.firstOrNull { it.id == actionId }
            ?: actionCache[actionId]
    }

    private fun resolveRoutine(routineId: String?): Routine? =
        routineId?.let { id -> _uiState.value.content?.routines?.firstOrNull { it.id == id } }

    private fun resolveMarker(markerId: String?): Marker? =
        markerId?.let { id -> _uiState.value.content?.markers?.firstOrNull { it.id == id } }

    private fun resolveSignal(signalId: String?): Signal? =
        signalId?.let { id -> _uiState.value.content?.signals?.firstOrNull { it.id == id } }
}
