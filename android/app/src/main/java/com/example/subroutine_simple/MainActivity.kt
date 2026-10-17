package com.example.subroutine_simple

import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.util.Log
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.viewModels

import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.SystemBarStyle
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height

import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.FormatListBulleted
import androidx.compose.material.icons.automirrored.filled.Login
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.automirrored.outlined.FormatListBulleted
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CalendarMonth
import androidx.compose.material.icons.filled.CenterFocusStrong
import androidx.compose.material.icons.filled.Explore
import androidx.compose.material.icons.filled.Inbox
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.ViewTimeline
import androidx.compose.material.icons.outlined.Explore
import androidx.compose.material.icons.outlined.Inbox
import androidx.compose.material.icons.outlined.ViewTimeline
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button

import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton

import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.FloatingActionButton
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold

import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.semantics.clearAndSetSemantics
import com.example.subroutine_simple.ui.creator.ParsedCreatorDraft
import com.example.subroutine_simple.ui.creator.ParsedCreatorDraftSaver
import com.example.subroutine_simple.ui.creator.ParsedItemCreator
import com.example.subroutine_simple.ui.creator.presentation.CreatorPresentation
import com.example.subroutine_simple.ui.creator.presentation.CreatorAppearance
import com.example.subroutine_simple.ui.creator.InlineCreatorSource
import com.example.subroutine_simple.ui.creator.CreatorSession
import com.example.subroutine_simple.ui.theme.SubroutineTheme
import com.example.subroutine_simple.ui.components.InlineActionDraftsViewModel
import com.example.subroutine_simple.ui.components.LocalInlineActionDrafts
import com.example.subroutine_simple.data.models.UuidV7
import androidx.compose.runtime.getValue

import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.key
import androidx.compose.ui.platform.testTag
import com.example.subroutine_simple.ui.components.SyncStatusBanner

import com.example.subroutine_simple.ui.components.inlineActionRequest
import com.example.subroutine_simple.ui.components.rememberInlineActionCreatorState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer

import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription

import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation3.runtime.entryProvider
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.ui.NavDisplay
import com.example.subroutine_simple.auth.AuthenticationState
import com.example.subroutine_simple.auth.AuthenticationRecovery

import com.example.subroutine_simple.reminders.ACTION_OPEN_REMINDER
import com.example.subroutine_simple.reminders.EXTRA_ACTION_ID
import com.example.subroutine_simple.reminders.EXTRA_WORKSPACE_ID
import com.example.subroutine_simple.reminders.POST_NOTIFICATIONS_PERMISSION
import com.example.subroutine_simple.reminders.LocalReminderRuntime
import com.example.subroutine_simple.reminders.ReminderNotifications
import com.example.subroutine_simple.reminders.ReminderNotificationStatus
import com.example.subroutine_simple.settings.AppearanceMode
import com.example.subroutine_simple.ui.MainViewModel
import com.example.subroutine_simple.ui.shouldShowWorkspaceLoading
import com.example.subroutine_simple.ui.ReminderOpenResult
import com.example.subroutine_simple.ui.SyncStatus
import com.example.subroutine_simple.ui.navigation.MainNavigationViewModel
import com.example.subroutine_simple.ui.navigation.MainTab
import com.example.subroutine_simple.ui.home.HomeScreen
import com.example.subroutine_simple.ui.calendar.CalendarScreen
import com.example.subroutine_simple.ui.calendar.projectCalendar
import com.example.subroutine_simple.ui.components.SubroutineLogotype
import com.example.subroutine_simple.ui.components.SyncConnectionIndicator

import com.example.subroutine_simple.ui.focus.FocusMode
import com.example.subroutine_simple.ui.focus.FocusScreen
import com.example.subroutine_simple.ui.focus.projectFocus
import com.example.subroutine_simple.ui.screens.ActionTemplateEditor
import com.example.subroutine_simple.ui.screens.BacklogScreen
import com.example.subroutine_simple.ui.screens.BrowseScreen
import com.example.subroutine_simple.ui.screens.EditActionScreen
import com.example.subroutine_simple.ui.screens.EditEventScreen
import com.example.subroutine_simple.ui.screens.EditMarkerScreen
import com.example.subroutine_simple.ui.screens.EditRoutineScreen
import com.example.subroutine_simple.ui.screens.EditSignalScreen
import com.example.subroutine_simple.ui.screens.EventTemplateEditor
import com.example.subroutine_simple.ui.screens.UpcomingScreen
import com.example.subroutine_simple.ui.screens.RoutinesScreen
import com.example.subroutine_simple.ui.screens.TemplatesScreen
import com.example.subroutine_simple.ui.search.SearchCorpus
import com.example.subroutine_simple.ui.search.SearchRequest
import com.example.subroutine_simple.ui.search.SearchResource
import com.example.subroutine_simple.ui.search.SearchScreen
import com.example.subroutine_simple.ui.search.projectSearch
import com.example.subroutine_simple.sync.BackgroundSyncWorker
import com.example.subroutine_simple.ui.settings.rememberNotificationStatus
import com.example.subroutine_simple.ui.settings.SettingsAccountState
import com.example.subroutine_simple.ui.settings.SettingsDataState
import com.example.subroutine_simple.ui.settings.SettingsScreen
import com.example.subroutine_simple.ui.theme.SubroutineSimpleTheme
import com.example.subroutine_simple.ui.theme.subroutineButtonColors
import com.example.subroutine_simple.ui.timeline.TimelineScreen
import com.example.subroutine_simple.ui.timeline.rememberTimelineCreatorState
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.imePadding
import java.time.Instant
import java.time.LocalDate
import java.time.YearMonth
import java.time.ZoneId
import kotlinx.coroutines.delay
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private data class ReminderOpenRequest(
    val actionId: String,
    val workspaceId: String,
    val validatedIntent: Intent,
    val uiEpoch: Long? = null,
)

private enum class NotificationPermissionRequest { None, Startup, EnableReminders }

class MainActivity : ComponentActivity() {
    private var reminderOpenRequest by mutableStateOf<ReminderOpenRequest?>(null)
    private var notificationStatusRevision by mutableStateOf(0)
    private var reminderIntentJob: Job? = null
    private val mainViewModel: MainViewModel by viewModels()
    private var notificationPermissionRequest = NotificationPermissionRequest.None
    private val notificationSettingsLauncher = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) {
        mainViewModel.settingsStore.setNotificationsEnabled(true)
        refreshNotificationStatus()
    }
    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        val request = notificationPermissionRequest
        notificationPermissionRequest = NotificationPermissionRequest.None
        mainViewModel.settingsStore.setNotificationsEnabled(granted)
        refreshNotificationStatus()
        if (!granted && request == NotificationPermissionRequest.EnableReminders &&
            !shouldShowRequestPermissionRationale(POST_NOTIFICATIONS_PERMISSION)
        ) {
            notificationSettingsLauncher.launch(ReminderNotifications.systemSettingsIntent(this))
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        notificationPermissionRequest = savedInstanceState?.getString("notificationPermissionRequest")
            ?.let(NotificationPermissionRequest::valueOf) ?: NotificationPermissionRequest.None
        handleReminderIntent(intent)
        BackgroundSyncWorker.schedule(applicationContext)
        ReminderNotifications.createChannel(applicationContext)
        enableEdgeToEdge()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            window.isNavigationBarContrastEnforced = false
        }
        setContent {
            val viewModel = mainViewModel
            val navigationViewModel: MainNavigationViewModel = viewModel()
            val settings by viewModel.settingsStore.settings.collectAsStateWithLifecycle()
            val useDarkTheme = when (settings.appearance) {
                AppearanceMode.SYSTEM -> isSystemInDarkTheme()
                AppearanceMode.LIGHT -> false
                AppearanceMode.DARK -> true
            }
            LaunchedEffect(useDarkTheme) {
                val style =
                    SystemBarStyle.auto(android.graphics.Color.TRANSPARENT, android.graphics.Color.TRANSPARENT) {
                        useDarkTheme
                    }
                enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    window.isNavigationBarContrastEnforced = false
                }
            }
            SubroutineSimpleTheme(darkTheme = useDarkTheme) {
                AppNavigation(
                    activity = this@MainActivity,
                    viewModel = viewModel,
                    navigationViewModel = navigationViewModel,
                    reminderOpenRequest = reminderOpenRequest,
                    notificationStatusRevision = notificationStatusRevision,
                    onReminderOpenConsumed = ::consumeReminderOpenRequest,
                )
            }
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        outState.putString("notificationPermissionRequest", notificationPermissionRequest.name)
        super.onSaveInstanceState(outState)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleReminderIntent(intent)
    }

    override fun onResume() {
        super.onResume()
        refreshNotificationStatus()
        mainViewModel.onForeground()
        if (!mainViewModel.settingsStore.notificationPermissionChecked) {
            mainViewModel.settingsStore.markNotificationPermissionChecked()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                !ReminderNotifications.hasPostNotificationsPermission(this)
            ) {
                requestNotificationPermission(NotificationPermissionRequest.Startup)
            }
        }
    }

    override fun onPause() {
        mainViewModel.onBackground()
        super.onPause()
    }

    private fun refreshNotificationStatus() {
        mainViewModel.settingsStore.refreshNotificationPermission()
        notificationStatusRevision += 1
        mainViewModel.reconcileReminders()
        lifecycleScope.launch(Dispatchers.IO) { LocalReminderRuntime.onForeground(applicationContext) }
    }

    fun setRemindersEnabled(enabled: Boolean) {
        val status = ReminderNotifications.permissionStatus(this)
        if (!enabled || status == ReminderNotificationStatus.Enabled) {
            mainViewModel.settingsStore.setNotificationsEnabled(enabled)
        } else {
            mainViewModel.settingsStore.setNotificationsEnabled(false)
            if (status == ReminderNotificationStatus.PermissionRequired) {
                requestNotificationPermission(NotificationPermissionRequest.EnableReminders)
            } else {
                notificationSettingsLauncher.launch(ReminderNotifications.systemSettingsIntent(this))
            }
        }
    }

    private fun requestNotificationPermission(request: NotificationPermissionRequest) {
        if (notificationPermissionRequest != NotificationPermissionRequest.None) return

        notificationPermissionRequest = request
        notificationPermissionLauncher.launch(POST_NOTIFICATIONS_PERMISSION)
    }

    private fun handleReminderIntent(intent: Intent?) {
        reminderIntentJob?.cancel()
        if (intent?.action != ACTION_OPEN_REMINDER) {
            setIntent(intent)
            return
        }
        reminderOpenRequest = null
        setIntent(Intent(this, MainActivity::class.java))
        val actionId = intent.getStringExtra(EXTRA_ACTION_ID)?.takeIf { it.isNotBlank() } ?: return
        val workspaceId = intent.getStringExtra(EXTRA_WORKSPACE_ID)?.takeIf { it.isNotBlank() } ?: return
        val candidate = Intent(intent)
        reminderIntentJob = lifecycleScope.launch {
            val current = withContext(Dispatchers.IO) {
                runCatching { LocalReminderRuntime.isCurrentOpenIntent(applicationContext, candidate) }.getOrDefault(
                    false
                )
            }
            if (current) {
                setIntent(candidate)
                reminderOpenRequest = ReminderOpenRequest(actionId, workspaceId, candidate)
            } else {
                Toast.makeText(this@MainActivity, "This reminder is no longer current.", Toast.LENGTH_LONG).show()
            }
        }
    }

    private fun consumeReminderOpenRequest() {
        reminderOpenRequest = null
        setIntent(Intent(this, MainActivity::class.java))
    }
}

@Composable
private fun AppNavigation(
    activity: MainActivity,
    viewModel: MainViewModel,
    navigationViewModel: MainNavigationViewModel,
    reminderOpenRequest: ReminderOpenRequest?,
    notificationStatusRevision: Int,
    onReminderOpenConsumed: () -> Unit,
) {
    val authentication by viewModel.authenticationState.collectAsStateWithLifecycle()
    val dataState by viewModel.uiState.collectAsStateWithLifecycle()
    val initializing by viewModel.initializing.collectAsStateWithLifecycle()
    val mergePrompt by viewModel.workspaceMergePrompt.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val signIn: (() -> Unit)? = if (viewModel.supportsSignIn) ({ viewModel.signIn(activity) }) else null
    var readyReminder by remember(reminderOpenRequest) { mutableStateOf<ReminderOpenRequest?>(null) }
    var reminderFeedback by remember { mutableStateOf<String?>(null) }
    var reminderNeedsSignIn by remember { mutableStateOf(false) }

    LaunchedEffect(reminderOpenRequest) {
        val request = reminderOpenRequest ?: return@LaunchedEffect
        reminderFeedback = null
        reminderNeedsSignIn = false
        when (val result = viewModel.prepareReminderOpen(request.workspaceId, request.actionId)) {
            is ReminderOpenResult.Ready -> readyReminder = request.copy(uiEpoch = result.uiEpoch)
            ReminderOpenResult.SignInRequired -> {
                reminderNeedsSignIn = signIn != null
                reminderFeedback = if (reminderNeedsSignIn) {
                    "Sign in, then open the action from your workspace. This reminder has expired."
                } else {
                    "This action belongs to an account workspace that is not available on this device."
                }
                onReminderOpenConsumed()
            }

            ReminderOpenResult.Unavailable -> {
                reminderFeedback = "The action may have been deleted or not downloaded. Open it from your workspace."
                onReminderOpenConsumed()
            }
        }
    }
    LaunchedEffect(readyReminder, authentication, dataState.content) {
        val request = readyReminder ?: return@LaunchedEffect
        val epoch = request.uiEpoch ?: return@LaunchedEffect
        if (!viewModel.canOpenReminder(request.workspaceId, request.actionId, epoch)) {
            reminderFeedback = "This reminder is no longer available in the current session."
            onReminderOpenConsumed()
        }
    }
    if (reminderOpenRequest != null && readyReminder == null) {
        AlertDialog(
            onDismissRequest = onReminderOpenConsumed,
            title = { Text("Opening reminder") },
            text = { LoadingIndicator() },
            confirmButton = { TextButton(onClick = onReminderOpenConsumed) { Text("Cancel") } },
        )
    }
    reminderFeedback?.let { message ->
        AlertDialog(
            onDismissRequest = { reminderFeedback = null },
            title = { Text(if (reminderNeedsSignIn) "Sign in required" else "Action unavailable") },
            text = { Text(message) },
            confirmButton = {
                TextButton(onClick = {
                    reminderFeedback = null
                    if (reminderNeedsSignIn) signIn?.invoke()
                }) { Text(if (reminderNeedsSignIn) "Sign in" else "OK") }
            },
            dismissButton = {
                if (reminderNeedsSignIn) {
                    TextButton(onClick = { reminderFeedback = null }) { Text("Not now") }
                }
            },
        )
    }
    val accountKey = when (val state = authentication) {
        is AuthenticationState.SignedIn -> state.accountId
        is AuthenticationState.InteractionRequired -> state.accountId
        is AuthenticationState.Error -> state.accountId
        else -> null
    }
    when {
        shouldShowWorkspaceLoading(authentication, dataState.content != null, dataState.syncStatus, initializing) -> {
            Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    LoadingIndicator(modifier = Modifier.size(48.dp).testTag("workspace:loading").semantics {
                        contentDescription = "Loading account and items"
                    })
                }
            }
        }

        dataState.content != null -> {
            // Expiry keeps this workspace's navigation and drafts; switching accounts does not.
            key(accountKey, dataState.content?.datasetId) {
                AuthenticatedNavigation(
                    activity = activity,
                    viewModel = viewModel,
                    navigationViewModel = navigationViewModel,
                    onSignIn = signIn.takeUnless { authentication is AuthenticationState.SignedIn },
                    reminderOpenRequest = readyReminder,
                    notificationStatusRevision = notificationStatusRevision,
                    onReminderOpenConsumed = onReminderOpenConsumed,
                )
            }
        }

        else -> {
            val error = authentication as? AuthenticationState.Error
            val retrySignOut = error?.recovery == AuthenticationRecovery.SignOut
            val retryConnection = error?.retryable == true || authentication == AuthenticationState.SignedOut
            AuthenticationRequired(
                message = error?.message ?: dataState.operationError ?: dataState.message
                ?: "Sign in to open your account.",
                onSignIn = when {
                    retrySignOut -> viewModel::signOut
                    retryConnection -> viewModel::loadActions
                    else -> signIn
                },
                actionLabel = when {
                    retrySignOut -> "Retry sign out"
                    retryConnection -> "Retry"
                    else -> "Sign in"
                },
            )
        }
    }

    mergePrompt?.let { prompt ->
        AlertDialog(
            onDismissRequest = {},
            title = { Text("Bring local items into this account?") },
            text = {
                Text(
                    "This device has ${prompt.localItemCount} local item${if (prompt.localItemCount == 1) "" else "s"}. " +
                            "The account currently has ${prompt.remoteItemCount}. You can import the local items or keep the workspaces separate.",
                )
            },
            confirmButton = {
                Button(
                    onClick = viewModel::mergeLocalData,
                    enabled = !saving,
                    colors = subroutineButtonColors(),
                ) {
                    Text("Import local items")
                }
            },
            dismissButton = {
                TextButton(onClick = viewModel::keepLocalDataSeparate, enabled = !saving) {
                    Text("Keep separate")
                }
            },
        )
    }
}

@Composable
private fun AuthenticationRequired(
    message: String,
    onSignIn: (() -> Unit)?,
    actionLabel: String = "Sign in",
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        contentAlignment = Alignment.Center,
    ) {
        Surface(
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            shape = MaterialTheme.shapes.extraLarge,
            modifier = Modifier.widthIn(max = 480.dp),
        ) {
            Column(
                modifier = Modifier.padding(horizontal = 28.dp, vertical = 32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                SubroutineLogotype()
                Spacer(Modifier.height(24.dp))
                Text(
                    text = message,
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
                if (onSignIn != null) {
                    Spacer(Modifier.height(24.dp))
                    Button(onClick = onSignIn, colors = subroutineButtonColors()) {
                        Text(actionLabel)
                    }
                }
            }
        }
    }
}

@Composable
private fun AuthenticatedNavigation(
    activity: MainActivity,
    viewModel: MainViewModel,
    navigationViewModel: MainNavigationViewModel,
    onSignIn: (() -> Unit)?,
    reminderOpenRequest: ReminderOpenRequest?,
    notificationStatusRevision: Int,
    onReminderOpenConsumed: () -> Unit,
) {
    val backStack = rememberNavBackStack(MainRoute)
    val navigationState by navigationViewModel.uiState.collectAsStateWithLifecycle()
    val authentication by viewModel.authenticationState.collectAsStateWithLifecycle()

    LaunchedEffect(notificationStatusRevision) {
        viewModel.reconcileReminders()
    }

    var reminderUnavailable by remember { mutableStateOf(false) }
    LaunchedEffect(reminderOpenRequest) {
        val request = reminderOpenRequest ?: return@LaunchedEffect
        val epoch = request.uiEpoch ?: return@LaunchedEffect
        val current = withContext(Dispatchers.IO) {
            runCatching { LocalReminderRuntime.isCurrentOpenIntent(activity, request.validatedIntent) }.getOrDefault(
                false
            )
        }
        if (current && viewModel.canOpenReminder(request.workspaceId, request.actionId, epoch)) {
            if (backStack.lastOrNull() != EditActionRoute(request.actionId)) {
                backStack.add(EditActionRoute(request.actionId))
            }
        } else {
            reminderUnavailable = true
        }
        onReminderOpenConsumed()
    }
    if (reminderUnavailable) {
        AlertDialog(
            onDismissRequest = { reminderUnavailable = false },
            title = { Text("Action unavailable") },
            text = { Text("This reminder is no longer current. Open the action from your workspace instead.") },
            confirmButton = { TextButton(onClick = { reminderUnavailable = false }) { Text("OK") } },
        )
    }

    val inlineDrafts: InlineActionDraftsViewModel = viewModel(viewModelStoreOwner = activity)
    var creatorOrigin by remember { mutableStateOf<Rect?>(null) }
    val creatorPresented by viewModel.showCreateSheet.collectAsStateWithLifecycle()
    val creatorSession by viewModel.creatorSession.collectAsStateWithLifecycle()
    CompositionLocalProvider(LocalInlineActionDrafts provides inlineDrafts) {
        Box(Modifier.fillMaxSize()) {
            NavDisplay(
                modifier = if (creatorPresented) Modifier.clearAndSetSemantics { } else Modifier,
                backStack = backStack,
                onBack = { backStack.removeLastOrNull() },
                transitionSpec = {
                    slideInHorizontally(initialOffsetX = { it }) togetherWith
                            slideOutHorizontally(targetOffsetX = { -it })
                },
                popTransitionSpec = {
                    slideInHorizontally(initialOffsetX = { -it }) togetherWith
                            slideOutHorizontally(targetOffsetX = { it })
                },
                predictivePopTransitionSpec = {
                    slideInHorizontally(initialOffsetX = { -it }) togetherWith
                            slideOutHorizontally(targetOffsetX = { it })
                },
                entryProvider = entryProvider {
                    entry<MainRoute> {
                        MainScreen(
                            viewModel = viewModel,
                            selectedTopLevelTab = navigationState.selectedTopLevelTab,
                            timelineDate = navigationState.timelineDate,
                            onSelectTopLevelTab = navigationViewModel::selectTopLevelTab,
                            onTimelineDateChange = navigationViewModel::setTimelineDate,
                            onEditAction = { actionId -> backStack.add(EditActionRoute(actionId)) },
                            onEditEvent = { eventId -> backStack.add(EditEventRoute(eventId)) },
                            onEditMarker = { markerId -> backStack.add(EditMarkerRoute(markerId)) },
                            onEditSignal = { signalId -> backStack.add(EditSignalRoute(signalId)) },
                            onEditRoutine = { id -> backStack.add(EditRoutineRoute(id)) },
                            onEditActionTemplate = { id -> backStack.add(EditActionTemplateRoute(id)) },
                            onEditEventTemplate = { id -> backStack.add(EditEventTemplateRoute(id)) },
                            onOpenSearch = { backStack.add(SearchRoute) },
                            onOpenRoutines = { backStack.add(RoutinesRoute) },
                            onOpenTemplates = { backStack.add(TemplatesRoute) },
                            onOpenFocus = { mode -> backStack.add(FocusRoute(mode)) },
                            onOpenBacklog = { backStack.add(BacklogRoute) },
                            onOpenCalendar = { date -> backStack.add(CalendarRoute(date.toString())) },
                            onOpenSettings = { backStack.add(SettingsRoute) },
                            onCreatorBounds = { creatorOrigin = it },
                            creatorPresented = creatorPresented,
                        )
                    }
                    entry<BacklogRoute> {
                        BrowseDetailScaffold(title = "Backlog", onBack = { backStack.removeLastOrNull() }) { padding ->
                            BacklogScreen(
                                viewModel = viewModel,
                                contentPadding = padding,
                                onEditAction = { id -> backStack.add(EditActionRoute(id)) },
                            )
                        }
                    }
                    entry<SearchRoute> {
                        BrowseDetailScaffold(
                            title = "Search",
                            onBack = { backStack.removeLastOrNull() },
                        ) { contentPadding ->
                            SearchDestination(
                                viewModel = viewModel,
                                contentPadding = contentPadding,
                                onEditAction = { id -> backStack.add(EditActionRoute(id)) },
                                onEditEvent = { id -> backStack.add(EditEventRoute(id)) },
                                onEditRoutine = { id -> backStack.add(EditRoutineRoute(id)) },
                                onEditMarker = { id -> backStack.add(EditMarkerRoute(id)) },
                                onEditSignal = { id -> backStack.add(EditSignalRoute(id)) },
                            )
                        }
                    }
                    entry<RoutinesRoute> {
                        BrowseDetailScaffold(
                            title = "Routines",
                            onBack = { backStack.removeLastOrNull() },
                        ) { contentPadding ->
                            RoutinesDestination(
                                viewModel = viewModel,
                                contentPadding = contentPadding,
                                onEditRoutine = { id -> backStack.add(EditRoutineRoute(id)) },
                                canStartRoutine = authentication is AuthenticationState.SignedIn,
                            )
                        }
                    }
                    entry<TemplatesRoute> {
                        BrowseDetailScaffold(
                            title = "Templates",
                            onBack = { backStack.removeLastOrNull() },
                        ) { contentPadding ->
                            TemplatesDestination(
                                viewModel = viewModel,
                                contentPadding = contentPadding,
                                onEditActionTemplate = { id -> backStack.add(EditActionTemplateRoute(id)) },
                                onEditEventTemplate = { id -> backStack.add(EditEventTemplateRoute(id)) },
                            )
                        }
                    }
                    entry<SettingsRoute> {
                        val authentication by viewModel.authenticationState.collectAsStateWithLifecycle()
                        val content by viewModel.uiState.collectAsStateWithLifecycle()
                        val settings by viewModel.settingsStore.settings.collectAsStateWithLifecycle()


                        val notificationStatus by rememberNotificationStatus(
                            activity, settings, notificationStatusRevision, backStack.lastOrNull() == SettingsRoute,
                        )
                        SettingsScreen(
                            settingsStore = viewModel.settingsStore,
                            notificationPermissionStatus = notificationStatus.permission,
                            accountState = authentication.toSettingsAccountState(),
                            dataState = SettingsDataState(
                                pendingCount = content.content?.pendingCount ?: 0,
                                blockedCount = content.content?.blockedCount ?: 0,
                                isRemoteAccountData = content.content?.isLocalOnly == false,
                            ),
                            onBack = { backStack.removeLastOrNull() },
                            onTestNotification = {
                                ReminderNotifications.postTestNotification(activity, settings)
                            },
                            onNotificationsEnabledChange = activity::setRemindersEnabled,
                            onOpenSystemNotificationSettings = {
                                ReminderNotifications.openSystemSettings(activity)
                            },
                            onSignIn = onSignIn,
                            onSignOut = viewModel::signOut,
                            onRemoveOfflineAccountData = viewModel::removeOfflineAccountData,
                            onSync = if (authentication is AuthenticationState.SignedIn) viewModel::recoverConnection else null,
                            syncing = content.isRefreshing,
                        )
                    }
                    entry<CalendarRoute> { route ->
                        CalendarDestination(
                            initialDate = route.initialDate
                                ?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
                                ?: navigationState.timelineDate,
                            viewModel = viewModel,
                            onBack = { backStack.removeLastOrNull() },
                            onEditAction = { id -> backStack.add(EditActionRoute(id)) },
                            onEditEvent = { id -> backStack.add(EditEventRoute(id)) },
                            onEditMarker = { id -> backStack.add(EditMarkerRoute(id)) },
                            onEditSignal = { id -> backStack.add(EditSignalRoute(id)) },
                            onViewInTimeline = { date ->
                                navigationViewModel.showTimeline(date)
                                backStack.removeLastOrNull()
                            },
                        )
                    }
                    entry<FocusRoute> { route ->
                        FocusDestination(
                            viewModel = viewModel,
                            initialMode = route.mode,
                            onBack = { backStack.removeLastOrNull() },
                            onEditAction = { id -> backStack.add(EditActionRoute(id)) },
                            onEditEvent = { id -> backStack.add(EditEventRoute(id)) },
                            onOpenSignal = { id -> backStack.add(EditSignalRoute(id)) },
                        )
                    }
                    entry<EditActionRoute> { route ->
                        EditActionScreen(
                            actionId = route.actionId,
                            viewModel = viewModel,
                            onBack = { backStack.popIfCurrent(route) },
                        )
                    }
                    entry<EditEventRoute> { route ->
                        EditEventScreen(
                            eventId = route.eventId,
                            viewModel = viewModel,
                            onBack = { backStack.popIfCurrent(route) },
                        )
                    }
                    entry<EditMarkerRoute> { route ->
                        EditMarkerScreen(
                            markerId = route.markerId,
                            viewModel = viewModel,
                            onBack = { backStack.popIfCurrent(route) },
                        )
                    }
                    entry<EditSignalRoute> { route ->
                        EditSignalScreen(
                            signalId = route.signalId,
                            viewModel = viewModel,
                            onBack = { backStack.popIfCurrent(route) },
                        )
                    }
                    entry<EditActionTemplateRoute> { route ->
                        val uiState by viewModel.uiState.collectAsStateWithLifecycle()
                        val saving by viewModel.saving.collectAsStateWithLifecycle()
                        val template = viewModel.actionTemplate(route.templateId)
                        val onBack: () -> Unit = { backStack.popIfCurrent(route) }
                        if (template != null) {
                            ActionTemplateEditor(
                                template = template,
                                saving = saving,
                                onBack = onBack,
                                onSave = { title, content, naiveTime, duration ->
                                    viewModel.saveActionTemplate(
                                        templateId = route.templateId,
                                        title = title,
                                        content = content,
                                        naiveTime = naiveTime,
                                        duration = duration,
                                        onSuccess = onBack,
                                    )
                                },
                                onDelete = {
                                    viewModel.deleteActionTemplate(route.templateId, onBack)
                                },
                            )
                        } else if (uiState.content != null && !saving) {
                            onBack()
                        }
                    }
                    entry<EditEventTemplateRoute> { route ->
                        val uiState by viewModel.uiState.collectAsStateWithLifecycle()
                        val saving by viewModel.saving.collectAsStateWithLifecycle()
                        val template = viewModel.eventTemplate(route.templateId)
                        val onBack: () -> Unit = { backStack.popIfCurrent(route) }
                        if (template != null) {
                            EventTemplateEditor(
                                template = template,
                                saving = saving,
                                onBack = onBack,
                                onSave = { title, content, duration ->
                                    viewModel.saveEventTemplate(
                                        templateId = route.templateId,
                                        title = title,
                                        content = content,
                                        duration = duration,
                                        onSuccess = onBack,
                                    )
                                },
                                onDelete = {
                                    viewModel.deleteEventTemplate(route.templateId, onBack)
                                },
                            )
                        } else if (uiState.content != null && !saving) {
                            onBack()
                        }
                    }
                    entry<EditRoutineRoute> { route ->
                        val uiState by viewModel.uiState.collectAsStateWithLifecycle()
                        val saving by viewModel.saving.collectAsStateWithLifecycle()
                        val routine = viewModel.routine(route.routineId)
                        val onBack: () -> Unit = { backStack.popIfCurrent(route) }
                        if (routine != null) {
                            EditRoutineScreen(
                                routine = routine,
                                saving = saving,
                                onBack = onBack,
                                onSave = { title, content, target, steps ->
                                    viewModel.saveRoutine(
                                        routineId = route.routineId,
                                        title = title,
                                        content = content,
                                        target = target,
                                        steps = steps,
                                        onSuccess = onBack,
                                    )
                                },
                                onDuplicate = {
                                    viewModel.duplicateRoutine(route.routineId, onBack)
                                },
                                onDelete = {
                                    viewModel.deleteRoutine(route.routineId, onBack)
                                },
                            )
                        } else if (uiState.content != null && !saving) {
                            onBack()
                        }
                    }
                },
            )
            ItemCreatorOverlay(viewModel, creatorOrigin, creatorPresented, creatorSession)
        }
    }
}

@Composable
private fun MainScreen(
    viewModel: MainViewModel,
    selectedTopLevelTab: MainTab,
    timelineDate: LocalDate,
    onSelectTopLevelTab: (MainTab) -> Unit,
    onTimelineDateChange: (LocalDate) -> Unit,
    onEditAction: (actionId: String) -> Unit,
    onEditEvent: (eventId: String) -> Unit,
    onEditMarker: (markerId: String) -> Unit,
    onEditSignal: (signalId: String) -> Unit,
    onEditRoutine: (String) -> Unit,
    onEditActionTemplate: (String) -> Unit,
    onEditEventTemplate: (String) -> Unit,
    onOpenSearch: () -> Unit,
    onOpenRoutines: () -> Unit,
    onOpenTemplates: () -> Unit,
    onOpenFocus: (FocusMode) -> Unit,
    onOpenBacklog: () -> Unit,
    onOpenCalendar: (LocalDate) -> Unit,
    onOpenSettings: () -> Unit,
    onCreatorBounds: (Rect) -> Unit,
    creatorPresented: Boolean,
) {

    val selectedTab = selectedTopLevelTab
    val connectionStatus by viewModel.connectionStatus.collectAsStateWithLifecycle()

    var navigationMenuExpanded by remember { mutableStateOf(false) }
    val tabStates = rememberSaveableStateHolder()

    Scaffold(
        modifier = Modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(selectedTab.name, maxLines = 1)
                        SyncConnectionIndicator(connectionStatus)
                    }
                },
                actions = {
                    IconButton(onClick = onOpenSearch) {
                        Icon(Icons.Default.Search, contentDescription = "Search all items")
                    }
                    IconButton(onClick = { onOpenCalendar(timelineDate) }) {
                        Icon(Icons.Default.CalendarMonth, contentDescription = "Open Calendar")
                    }
                    Box {
                        IconButton(onClick = { navigationMenuExpanded = true }) {
                            Icon(Icons.Default.MoreVert, contentDescription = "More destinations")
                        }
                        DropdownMenu(
                            expanded = navigationMenuExpanded,
                            onDismissRequest = { navigationMenuExpanded = false },
                        ) {
                            listOf(
                                "Backlog" to onOpenBacklog,
                                "Focus" to { onOpenFocus(FocusMode.Actions) },
                                "Routines" to onOpenRoutines,
                                "Templates" to onOpenTemplates,
                                "Settings & account" to onOpenSettings,
                            ).forEach { (label, open) ->
                                DropdownMenuItem(
                                    text = { Text(label) },
                                    onClick = { navigationMenuExpanded = false; open() },
                                )
                            }
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.surface,
                    scrolledContainerColor = MaterialTheme.colorScheme.surfaceContainer,
                ),
            )
        },
        bottomBar = {
            NavigationBar(
                containerColor = MaterialTheme.colorScheme.surfaceContainer,
                tonalElevation = 0.dp,
            ) {
                MainTab.entries.forEach { tab ->
                    val selected = selectedTab == tab
                    NavigationBarItem(
                        selected = selected,
                        onClick = { onSelectTopLevelTab(tab) },
                        icon = {
                            Icon(
                                imageVector = when (tab) {
                                    MainTab.Home -> if (selected) Icons.Filled.Home else Icons.Outlined.Home
                                    MainTab.Timeline -> if (selected) Icons.Filled.ViewTimeline else Icons.Outlined.ViewTimeline
                                    MainTab.Upcoming -> if (selected) Icons.AutoMirrored.Filled.FormatListBulleted
                                    else Icons.AutoMirrored.Outlined.FormatListBulleted

                                    MainTab.Browse -> if (selected) Icons.Filled.Explore else Icons.Outlined.Explore
                                },
                                contentDescription = null,
                            )
                        },
                        label = { Text(tab.name) },
                        colors = subroutineNavigationItemColors(),
                        modifier = Modifier.testTag("navigation:${tab.name.lowercase()}"),
                    )
                }
            }
        },
        floatingActionButton = {
            FloatingActionButton(
                onClick = { if (!viewModel.showCreateSheet.value) viewModel.openCreateSheet() },
                shape = RoundedCornerShape(CreatorAppearance.cornerRadius),
                containerColor = SubroutineTheme.colors.primaryFill,
                contentColor = SubroutineTheme.colors.onPrimaryFill,
                modifier = Modifier
                    .onGloballyPositioned { onCreatorBounds(it.boundsInRoot()) }
                    .graphicsLayer { alpha = if (creatorPresented) 0f else 1f }
                    .testTag("creator:fab")
                    .then(if (creatorPresented) Modifier.clearAndSetSemantics { } else Modifier),
            ) {
                Icon(Icons.Filled.Add, contentDescription = "New item")
            }
        },
    ) { innerPadding ->
        tabStates.SaveableStateProvider(selectedTab.name) {
            when (selectedTab) {
                MainTab.Upcoming -> UpcomingScreen(
                    viewModel = viewModel,
                    contentPadding = innerPadding,
                    onEditAction = onEditAction,
                    onEditEvent = onEditEvent,
                    onEditMarker = onEditMarker,
                    onEditSignal = onEditSignal,
                    onCreateOnDate = viewModel::openCreateSheetOnDate,
                    modifier = Modifier.fillMaxSize(),
                )

                MainTab.Timeline -> TimelineDestination(
                    viewModel = viewModel,
                    selectedDate = timelineDate,
                    onSelectedDateChange = onTimelineDateChange,
                    contentPadding = innerPadding,
                    onEditAction = onEditAction,
                    onEditEvent = onEditEvent,
                    onEditMarker = onEditMarker,
                    onEditSignal = onEditSignal,
                )

                MainTab.Browse -> BrowseScreen(
                    viewModel = viewModel,
                    contentPadding = innerPadding,
                    onEditItem = { resource ->
                        when (resource) {
                            is SearchResource.LiveAction -> onEditAction(resource.id)
                            is SearchResource.LiveEvent -> onEditEvent(resource.id)
                            is SearchResource.LiveRoutine -> onEditRoutine(resource.id)
                            is SearchResource.LiveMarker -> onEditMarker(resource.id)
                            is SearchResource.LiveSignal -> onEditSignal(resource.id)
                            is SearchResource.SavedAction -> onEditActionTemplate(resource.id)
                            is SearchResource.SavedEvent -> onEditEventTemplate(resource.id)
                        }
                    },
                )

                MainTab.Home -> HomeScreen(
                    viewModel = viewModel,
                    contentPadding = innerPadding,
                    onEditAction = onEditAction,
                    onEditEvent = onEditEvent,
                    onOpenFocus = onOpenFocus,
                )
            }
        }
    }
}

@Composable
private fun ItemCreatorOverlay(viewModel: MainViewModel, origin: Rect?, visible: Boolean, session: CreatorSession) {
    val inlineDrafts = LocalInlineActionDrafts.current
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    var retained by remember { mutableStateOf(visible) }
    LaunchedEffect(visible) { if (visible) retained = true }

    if (visible || retained) {
        val sharedSaving by viewModel.saving.collectAsStateWithLifecycle()
        val submission by viewModel.creatorSubmission.collectAsStateWithLifecycle()
        val saving = submission == session.id
        LaunchedEffect(visible, saving, sharedSaving) {
            if (BuildConfig.DEBUG) Log.d("Creator", "visible=$visible submitting=$saving sharedSaving=$sharedSaving")
        }
        var draft by rememberSaveable(session.id, stateSaver = ParsedCreatorDraftSaver) {
            mutableStateOf(ParsedCreatorDraft.initial(session.initial))
        }
        var confirmDiscard by rememberSaveable(session.id) { mutableStateOf(false) }
        val dismiss = {
            if (BuildConfig.DEBUG) Log.d("Creator", "dismiss requested; submitting=$saving")
            if (!saving) {
                if (draft.isDirty) confirmDiscard = true else viewModel.closeCreateSheet()
            }
        }
        CreatorPresentation(
            visible = visible,
            origin = origin,
            onDismissRequest = dismiss,
            onHidden = { if (!viewModel.showCreateSheet.value) retained = false },
        ) { focusRequester ->
            key(session.id) {
                ParsedItemCreator(
                    draft = draft,
                    onDraftChange = { draft = it },
                    focusRequester = focusRequester,
                    onCreate = { request ->
                        val submittedSession = session
                        viewModel.createItem(request, creationId = submittedSession.creationId, onSuccess = {
                            submittedSession.source?.let { source ->
                                inlineDrafts.draft(source.key).complete(source.creationId)
                            }
                            if (viewModel.creatorSession.value.id == submittedSession.id) viewModel.closeCreateSheet()
                        })
                    },
                    initiallyExpandedDetails = session.expandDetails,
                    onCancel = dismiss,
                    isSubmitting = saving || !visible,
                    creationEnabled = !sharedSaving,
                    errorMessage = uiState.operationError,
                )
            }
        }
        if (confirmDiscard && visible) {
            AlertDialog(
                onDismissRequest = { confirmDiscard = false },
                title = { Text("Discard this item?") },
                text = { Text("The item you started will not be created.") },
                confirmButton = {
                    TextButton(onClick = { confirmDiscard = false; viewModel.closeCreateSheet() }, enabled = !saving) {
                        Text("Discard")
                    }
                },
                dismissButton = {
                    TextButton(onClick = { confirmDiscard = false }) { Text("Keep editing") }
                },
            )
        }
    }
}

@Composable
private fun AuthenticationState.toSettingsAccountState(): SettingsAccountState = when (this) {
    AuthenticationState.SignedOut,
    AuthenticationState.Initializing,
        -> SettingsAccountState.SignedOutLocalOnly

    is AuthenticationState.SignedIn -> SettingsAccountState.SignedIn(username)
    is AuthenticationState.InteractionRequired -> SettingsAccountState.NeedsAttention(username)
    is AuthenticationState.Error -> when (recovery) {
        AuthenticationRecovery.SignOut -> SettingsAccountState.SignOutFailed(message)
        AuthenticationRecovery.Reconnect -> SettingsAccountState.NeedsAttention(null)
    }
}

@Composable
private fun subroutineNavigationItemColors() = NavigationBarItemDefaults.colors(
    selectedIconColor = MaterialTheme.colorScheme.onPrimaryContainer,
    selectedTextColor = MaterialTheme.colorScheme.primary,
    indicatorColor = MaterialTheme.colorScheme.primaryContainer,
    unselectedIconColor = MaterialTheme.colorScheme.onSurfaceVariant,
    unselectedTextColor = MaterialTheme.colorScheme.onSurfaceVariant,
)

@Composable
private fun CalendarDestination(
    initialDate: LocalDate,
    viewModel: MainViewModel,
    onBack: () -> Unit,
    onEditAction: (String) -> Unit,
    onEditEvent: (String) -> Unit,
    onEditMarker: (String) -> Unit,
    onEditSignal: (String) -> Unit,
    onViewInTimeline: (LocalDate) -> Unit,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val completing by viewModel.completing.collectAsStateWithLifecycle()
    val workspace by viewModel.workspaceDescriptor.collectAsStateWithLifecycle()
    var dateValue by rememberSaveable(initialDate) { mutableStateOf(initialDate.toString()) }
    var monthValue by rememberSaveable(initialDate) { mutableStateOf(YearMonth.from(initialDate).toString()) }
    val selectedDate = LocalDate.parse(dateValue)
    val visibleMonth = YearMonth.parse(monthValue)
    val creatorState =
        rememberInlineActionCreatorState("calendar:${workspace?.workspaceId}:${uiState.content?.datasetId}:$selectedDate")
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Calendar") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { innerPadding ->
        val content = uiState.content
        if (content == null) {
            UnavailableBrowseContent(uiState, innerPadding)
        } else {
            CalendarScreen(
                projection = projectCalendar(
                    visibleMonth = visibleMonth,
                    selectedDate = selectedDate,
                    actions = content.actions,
                    events = content.events,
                    markers = content.markers,
                    signals = content.signals,
                ),
                onVisibleMonthChange = { monthValue = it.toString() },
                onSelectedDateChange = { dateValue = it.toString() },
                onCreateOnDay = viewModel::openCreateSheetOnDate,
                onViewInTimeline = onViewInTimeline,
                onEditAction = { onEditAction(it.id) },
                onEditEvent = { onEditEvent(it.id) },
                onEditMarker = { onEditMarker(it.id) },
                onEditSignal = { onEditSignal(it.id) },
                onCompleteAction = viewModel::completeAction,
                completing = completing,
                actionsEnabled = !saving,
                creatorState = creatorState,
                onMoreOptions = {
                    viewModel.openInlineCreator(
                        InlineCreatorSource(creatorState.key, creatorState.creationId),
                        creatorState.title,
                        selectedDate,
                    )
                },
                onCreateAction = { title, onSuccess ->
                    inlineActionRequest(title, selectedDate)?.let {
                        viewModel.createItem(it, onSuccess = onSuccess, creationId = creatorState.creationId)
                    }
                },
                errorMessage = uiState.operationError,
                status = { WorkspaceStatus(uiState, viewModel) },
                contentPadding = innerPadding,
                modifier = Modifier.widthIn(max = 720.dp),
            )
        }
    }
}

@Composable
private fun FocusDestination(
    viewModel: MainViewModel,
    initialMode: FocusMode = FocusMode.Actions,
    onBack: () -> Unit,
    onEditAction: (String) -> Unit,
    onEditEvent: (String) -> Unit,
    onOpenSignal: (String) -> Unit,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    var mode by remember(initialMode) { mutableStateOf(initialMode) }
    var selectedActionKey by remember { mutableStateOf<String?>(null) }
    var dismissedSignalKeys by remember { mutableStateOf(emptySet<String>()) }
    var now by remember { mutableStateOf(Instant.now()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(1_000L)
            now = Instant.now()
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Focus") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { innerPadding ->
        val content = uiState.content
        if (content == null) {
            UnavailableBrowseContent(uiState, innerPadding)
        } else {
            val projection = projectFocus(
                actions = content.actions,
                events = content.events,
                signals = content.signals,
                now = now,
            )
            FocusScreen(
                projection = projection,
                mode = mode,
                selectedActionKey = selectedActionKey,
                onModeChange = { mode = it },
                onSelectedActionChange = { selectedActionKey = it },
                onCompleteAction = { viewModel.completeAction(it.id) },
                onEditAction = { onEditAction(it.id) },
                onEditEvent = { onEditEvent(it.id) },
                onDismissSignal = { dismissedSignalKeys = dismissedSignalKeys + it },
                onOpenSignal = { onOpenSignal(it.id) },
                contentPadding = innerPadding,
            )
        }
    }
}

@Composable
private fun BrowseDetailScaffold(
    title: String,
    onBack: () -> Unit,
    content: @Composable (androidx.compose.foundation.layout.PaddingValues) -> Unit,
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
        content = content,
    )
}


@Composable
private fun SearchDestination(
    viewModel: MainViewModel,
    contentPadding: androidx.compose.foundation.layout.PaddingValues,
    onEditAction: (String) -> Unit,
    onEditEvent: (String) -> Unit,
    onEditRoutine: (String) -> Unit,
    onEditMarker: (String) -> Unit,
    onEditSignal: (String) -> Unit,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    var request by remember { mutableStateOf(SearchRequest()) }
    val content = uiState.content
    if (content == null) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(contentPadding),
            contentAlignment = Alignment.Center,
        ) {
            if (uiState.syncStatus == SyncStatus.InitialLoading) {
                CircularProgressIndicator()
            } else {
                Text(uiState.operationError ?: uiState.message ?: "Could not load items.")
            }
        }
        return
    }

    SearchScreen(
        request = request,
        projection = projectSearch(
            corpus = SearchCorpus(
                actions = content.actions,
                events = content.events,
                routines = content.routines,
                markers = content.markers,
                signals = content.signals,
                actionTemplates = content.actionTemplates,
                eventTemplates = content.eventTemplates,
            ),
            request = request,
        ),
        onRequestChange = { request = it },
        onEditLiveItem = { resource ->
            when (resource) {
                is SearchResource.LiveAction -> onEditAction(resource.id)
                is SearchResource.LiveEvent -> onEditEvent(resource.id)
                is SearchResource.LiveRoutine -> onEditRoutine(resource.id)
                is SearchResource.LiveMarker -> onEditMarker(resource.id)
                is SearchResource.LiveSignal -> onEditSignal(resource.id)
            }
        },
        onUseSavedTemplate = { resource ->
            when (resource) {
                is SearchResource.SavedAction -> viewModel.useActionTemplate(resource.id)
                is SearchResource.SavedEvent -> viewModel.useEventTemplate(resource.id)
            }
        },
        contentPadding = contentPadding,
        modifier = Modifier.fillMaxSize(),
    )
}

@Composable
private fun RoutinesDestination(
    viewModel: MainViewModel,
    contentPadding: androidx.compose.foundation.layout.PaddingValues,
    onEditRoutine: (String) -> Unit,
    canStartRoutine: Boolean,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val content = uiState.content
    if (content == null) {
        UnavailableBrowseContent(uiState, contentPadding)
        return
    }
    RoutinesScreen(
        routines = content.routines,
        onEditRoutine = onEditRoutine,
        onStartRoutine = if (canStartRoutine) viewModel::instantiateRoutine else null,
        startActionsEnabled = !saving,
        contentPadding = contentPadding,
        modifier = Modifier.fillMaxSize(),
    )
}

@Composable
private fun TemplatesDestination(
    viewModel: MainViewModel,
    contentPadding: androidx.compose.foundation.layout.PaddingValues,
    onEditActionTemplate: (String) -> Unit,
    onEditEventTemplate: (String) -> Unit,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val content = uiState.content
    if (content == null) {
        UnavailableBrowseContent(uiState, contentPadding)
        return
    }
    TemplatesScreen(
        actionTemplates = content.actionTemplates,
        eventTemplates = content.eventTemplates,
        onUseActionTemplate = viewModel::useActionTemplate,
        onUseEventTemplate = viewModel::useEventTemplate,
        onEditActionTemplate = onEditActionTemplate,
        onEditEventTemplate = onEditEventTemplate,
        contentPadding = contentPadding,
        modifier = Modifier.fillMaxSize(),
    )
}

@Composable
internal fun UnavailableBrowseContent(
    uiState: com.example.subroutine_simple.ui.ActionsUiState,
    contentPadding: androidx.compose.foundation.layout.PaddingValues,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(contentPadding),
        contentAlignment = Alignment.Center,
    ) {
        if (uiState.syncStatus == SyncStatus.InitialLoading) {
            CircularProgressIndicator()
        } else {
            Text(uiState.operationError ?: uiState.message ?: "Could not load items.")
        }
    }
}

@Composable
private fun WorkspaceStatus(state: com.example.subroutine_simple.ui.ActionsUiState, viewModel: MainViewModel) {
    state.attentionMessage?.let { message ->
        val authentication by viewModel.authenticationState.collectAsStateWithLifecycle()
        val saving by viewModel.saving.collectAsStateWithLifecycle()
        SyncStatusBanner(
            message = message,
            conflict = (state.content?.blockedCount ?: 0) > 0,
            onUseRemote = viewModel::useRemoteConflict,
            onKeepLocal = viewModel::keepLocalConflict,
            actionsEnabled = !saving && authentication is AuthenticationState.SignedIn,
        )
    }
}

@Composable
private fun TimelineDestination(
    viewModel: MainViewModel,
    selectedDate: LocalDate,
    onSelectedDateChange: (LocalDate) -> Unit,
    contentPadding: androidx.compose.foundation.layout.PaddingValues,
    onEditAction: (String) -> Unit,
    onEditEvent: (String) -> Unit,
    onEditMarker: (String) -> Unit,
    onEditSignal: (String) -> Unit,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val content = uiState.content
    if (content == null) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(contentPadding),
            contentAlignment = Alignment.Center,
        ) {
            if (uiState.syncStatus == SyncStatus.InitialLoading) {
                CircularProgressIndicator()
            } else {
                Text(uiState.operationError ?: uiState.message ?: "Could not load items.")
            }
        }
        return
    }

    val saving by viewModel.saving.collectAsStateWithLifecycle()
    val completing by viewModel.completing.collectAsStateWithLifecycle()
    val workspace by viewModel.workspaceDescriptor.collectAsStateWithLifecycle()
    val creator = rememberTimelineCreatorState("${workspace?.workspaceId}:${content.datasetId}")
    val zoneId = ZoneId.systemDefault()
    key(creator.scopeKey) {
        TimelineScreen(
            initialDate = selectedDate,
            zoneId = zoneId,
            actions = content.actions,
            events = content.events,
            markers = content.markers,
            signals = content.signals,
            onCenteredDateChange = onSelectedDateChange,
            onActionClick = onEditAction,
            onEventClick = onEditEvent,
            onMarkerClick = onEditMarker,
            onSignalClick = onEditSignal,
            onCompleteAction = viewModel::completeAction,
            actionsEnabled = !saving,
            completingActionIds = completing,
            status = { WorkspaceStatus(uiState, viewModel) },
            creator = creator,
            onCreate = { request, creationId, complete ->
                viewModel.createItem(request, creationId = creationId, onSuccess = complete)
            },
            onApplyEdit = viewModel::applyTimelineEdit,
            creationError = uiState.operationError,
            modifier = Modifier
                .fillMaxSize()
                .padding(contentPadding)
                .consumeWindowInsets(contentPadding)
                .imePadding(),
        )
    }
}
