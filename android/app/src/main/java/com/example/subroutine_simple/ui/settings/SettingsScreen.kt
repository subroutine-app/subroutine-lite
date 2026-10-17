package com.example.subroutine_simple.ui.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.example.subroutine_simple.settings.AppSettings
import com.example.subroutine_simple.settings.AppearanceMode
import com.example.subroutine_simple.settings.NotificationLeadTime
import com.example.subroutine_simple.settings.NotificationPreview
import com.example.subroutine_simple.settings.SettingsStore
import com.example.subroutine_simple.ui.theme.subroutineTonalButtonColors

object SettingsTestTags {
    const val Root = "settings"
    const val Back = "settings:back"
    const val Appearance = "settings:appearance"
    const val Notifications = "settings:notifications"
    const val NotificationsToggle = "settings:notifications:toggle"
    const val NotificationStatus = "settings:notifications:status"
    const val ServerRemindersStatus = "settings:notifications:server-reminders-status"
    const val LeadTime = "settings:notifications:lead-time"
    const val Preview = "settings:notifications:preview"
    const val TestNotification = "settings:notifications:test"
    const val SystemNotificationSettings = "settings:notifications:system-settings"
    const val Account = "settings:account"
    const val SignIn = "settings:account:sign-in"
    const val SignOut = "settings:account:sign-out"
    const val Data = "settings:data"
    const val RemoveOfflineData = "settings:data:remove-offline"
    const val RemoveOfflineDataDialog = "settings:data:remove-offline:dialog"
    const val ConfirmRemoveOfflineData = "settings:data:remove-offline:confirm"

    fun appearance(mode: AppearanceMode): String = "settings:appearance:${mode.name.lowercase()}"
    fun leadTime(leadTime: NotificationLeadTime): String =
        "settings:notifications:lead-time:${leadTime.seconds}"

    fun preview(preview: NotificationPreview): String =
        "settings:notifications:preview:${preview.name.lowercase()}"
}

@Composable
fun SettingsScreen(
    settingsStore: SettingsStore,
    notificationPermissionStatus: NotificationPermissionStatus,
    accountState: SettingsAccountState,
    dataState: SettingsDataState,
    onBack: () -> Unit,
    onNotificationsEnabledChange: (Boolean) -> Unit,
    onTestNotification: () -> Unit,
    onOpenSystemNotificationSettings: () -> Unit,
    onSignIn: (() -> Unit)?,
    onSignOut: () -> Unit,
    onRemoveOfflineAccountData: () -> Unit,
    onSync: (() -> Unit)? = null,
    syncing: Boolean = false,
    modifier: Modifier = Modifier,
) {
    val settings by settingsStore.settings.collectAsStateWithLifecycle()
    SettingsScreen(
        settings = settings,
        notificationPermissionStatus = notificationPermissionStatus,
        accountState = accountState,
        dataState = dataState,
        onBack = onBack,
        onAppearanceChange = settingsStore::setAppearance,
        onNotificationsEnabledChange = onNotificationsEnabledChange,
        onNotificationLeadTimeChange = settingsStore::setNotificationLeadTime,
        onNotificationPreviewChange = settingsStore::setNotificationPreview,
        onTestNotification = onTestNotification,
        onOpenSystemNotificationSettings = onOpenSystemNotificationSettings,
        onSignIn = onSignIn,
        onSignOut = onSignOut,
        onRemoveOfflineAccountData = onRemoveOfflineAccountData,
        onSync = onSync,
        syncing = syncing,
        modifier = modifier,
    )
}

@Composable
fun SettingsScreen(
    settings: AppSettings,
    notificationPermissionStatus: NotificationPermissionStatus,
    accountState: SettingsAccountState,
    dataState: SettingsDataState,
    onBack: () -> Unit,
    onAppearanceChange: (AppearanceMode) -> Unit,
    onNotificationsEnabledChange: (Boolean) -> Unit,
    onNotificationLeadTimeChange: (NotificationLeadTime) -> Unit,
    onNotificationPreviewChange: (NotificationPreview) -> Unit,
    onTestNotification: () -> Unit,
    onOpenSystemNotificationSettings: () -> Unit,
    onSignIn: (() -> Unit)?,
    onSignOut: () -> Unit,
    onRemoveOfflineAccountData: () -> Unit,
    onSync: (() -> Unit)? = null,
    syncing: Boolean = false,
    modifier: Modifier = Modifier,
) {
    var showRemoveDataConfirmation by rememberSaveable { mutableStateOf(false) }
    var showSignOutConfirmation by rememberSaveable { mutableStateOf(false) }
    val mayRemoveOfflineAccountData =
        accountState !is SettingsAccountState.SignedOutLocalOnly && dataState.isRemoteAccountData

    Scaffold(
        modifier = modifier
            .fillMaxSize()
            .testTag(SettingsTestTags.Root),
        topBar = {
            TopAppBar(
                title = { Text("Settings") },
                navigationIcon = {
                    IconButton(
                        onClick = onBack,
                        modifier = Modifier.testTag(SettingsTestTags.Back),
                    ) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { innerPadding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
            contentAlignment = Alignment.TopCenter,
        ) {
            androidx.compose.foundation.lazy.LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .widthIn(max = 720.dp),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {

                if (onSignIn != null || accountState !is SettingsAccountState.SignedOutLocalOnly) {
                    item(key = "account") {
                        AccountSection(
                            state = accountState,
                            onSignIn = onSignIn,
                            onSignOut = { showSignOutConfirmation = true },
                        )
                        if (onSync != null) {
                            TextButton(onClick = onSync, enabled = !syncing) {
                                Text(if (syncing) "Synchronizing…" else "Sync now")
                            }
                        }
                    }
                }
                item(key = "appearance") {
                    AppearanceSection(
                        appearance = settings.appearance,
                        onAppearanceChange = onAppearanceChange,
                    )
                }
                item(key = "notifications") {
                    NotificationsSection(
                        settings = settings,
                        permissionStatus = notificationPermissionStatus,
                        onEnabledChange = onNotificationsEnabledChange,
                        onLeadTimeChange = onNotificationLeadTimeChange,
                        onPreviewChange = onNotificationPreviewChange,
                        onTestNotification = onTestNotification,
                        onOpenSystemNotificationSettings = onOpenSystemNotificationSettings,
                    )
                }
                item(key = "data") {
                    DataSection(
                        state = dataState,
                        showRemoveAction = mayRemoveOfflineAccountData,
                        onRemoveOfflineData = { showRemoveDataConfirmation = true },
                    )
                }
            }
        }
    }

    if (showSignOutConfirmation) {
        AlertDialog(
            onDismissRequest = { showSignOutConfirmation = false },
            title = { Text("Sign out?") },
            text = {
                Text(
                    if (dataState.pendingCount > 0) {
                        "${dataState.pendingCount} unsynced changes will stay on this device. Sign in to the same account to synchronize them."
                    } else {
                        "Account data stays on this device. Your local workspace will open."
                    }
                )
            },
            confirmButton = {
                TextButton(onClick = { showSignOutConfirmation = false; onSignOut() }) { Text("Sign out") }
            },
            dismissButton = {
                TextButton(onClick = { showSignOutConfirmation = false }) { Text("Cancel") }
            },
        )
    }

    if (showRemoveDataConfirmation && mayRemoveOfflineAccountData) {
        AlertDialog(
            onDismissRequest = { showRemoveDataConfirmation = false },
            title = { Text("Remove offline account data?") },
            text = {
                Text(
                    "This removes the account data downloaded to this device. Any pending local " +
                            "changes that have not synced will be lost. Data stored on the server is not deleted.",
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        showRemoveDataConfirmation = false
                        onRemoveOfflineAccountData()
                    },
                    colors = ButtonDefaults.textButtonColors(
                        contentColor = MaterialTheme.colorScheme.error,
                    ),
                    modifier = Modifier.testTag(SettingsTestTags.ConfirmRemoveOfflineData),
                ) {
                    Text("Remove from this device")
                }
            },
            dismissButton = {
                TextButton(onClick = { showRemoveDataConfirmation = false }) {
                    Text("Cancel")
                }
            },
            modifier = Modifier.testTag(SettingsTestTags.RemoveOfflineDataDialog),
        )
    }
}

@Composable
private fun AppearanceSection(
    appearance: AppearanceMode,
    onAppearanceChange: (AppearanceMode) -> Unit,
) {
    SettingsSection(
        title = "Appearance",
        icon = Icons.Default.Palette,
        modifier = Modifier.testTag(SettingsTestTags.Appearance),
    ) {
        SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
            AppearanceMode.entries.forEachIndexed { index, mode ->
                SegmentedButton(
                    selected = appearance == mode,
                    onClick = { onAppearanceChange(mode) },
                    colors = SegmentedButtonDefaults.colors(
                        activeContainerColor = MaterialTheme.colorScheme.primaryContainer,
                        activeContentColor = MaterialTheme.colorScheme.onSurface,
                    ),
                    shape = SegmentedButtonDefaults.itemShape(
                        index = index,
                        count = AppearanceMode.entries.size,
                    ),
                    modifier = Modifier
                        .weight(1f)
                        .testTag(SettingsTestTags.appearance(mode)),
                ) {
                    Text(mode.label)
                }
            }
        }
    }
}

@Composable
private fun NotificationsSection(
    settings: AppSettings,
    permissionStatus: NotificationPermissionStatus,
    onEnabledChange: (Boolean) -> Unit,
    onLeadTimeChange: (NotificationLeadTime) -> Unit,
    onPreviewChange: (NotificationPreview) -> Unit,
    onTestNotification: () -> Unit,
    onOpenSystemNotificationSettings: () -> Unit,
) {
    val remindersEnabled = settings.notificationsEnabled && permissionStatus.canPostNotifications
    SettingsSection(
        title = "Notifications",
        icon = Icons.Default.Notifications,
        modifier = Modifier.testTag(SettingsTestTags.Notifications),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .toggleable(
                    value = remindersEnabled,
                    role = Role.Switch,
                    onValueChange = onEnabledChange,
                )
                .padding(vertical = 4.dp)
                .testTag(SettingsTestTags.NotificationsToggle),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                "Allow reminders",
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.weight(1f),
            )
            Switch(
                checked = remindersEnabled,
                onCheckedChange = null,
            )
        }

        if (!permissionStatus.canPostNotifications) {
            if (permissionStatus == NotificationPermissionStatus.CHECKING) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp).semantics {
                        contentDescription = permissionStatus.description
                    },
                    strokeWidth = 2.dp,
                )
            } else {
                Text(
                    text = permissionStatus.description,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.testTag(SettingsTestTags.NotificationStatus),
                )
            }
        }
        Text(
            text = "Server reminders unavailable.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.testTag(SettingsTestTags.ServerRemindersStatus),
        )

        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)

        SettingLabel(title = "Lead time")
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .testTag(SettingsTestTags.LeadTime),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            NotificationLeadTime.entries.forEach { leadTime ->
                FilterChip(
                    selected = settings.notificationLeadTime == leadTime,
                    onClick = { onLeadTimeChange(leadTime) },
                    label = { Text(leadTime.label) },
                    colors = FilterChipDefaults.filterChipColors(
                        selectedContainerColor = MaterialTheme.colorScheme.primaryContainer,
                        selectedLabelColor = MaterialTheme.colorScheme.onSurface,
                        selectedLeadingIconColor = MaterialTheme.colorScheme.onSurface,
                        selectedTrailingIconColor = MaterialTheme.colorScheme.onSurface,
                    ),
                    modifier = Modifier.testTag(SettingsTestTags.leadTime(leadTime)),
                )
            }
        }

        SettingLabel(
            title = "Preview",
            supportingText = "Action titles may be visible on the lock screen.",
        )
        SingleChoiceSegmentedButtonRow(
            modifier = Modifier
                .fillMaxWidth()
                .testTag(SettingsTestTags.Preview),
        ) {
            NotificationPreview.entries.forEachIndexed { index, preview ->
                SegmentedButton(
                    selected = settings.notificationPreview == preview,
                    onClick = { onPreviewChange(preview) },
                    colors = SegmentedButtonDefaults.colors(
                        activeContainerColor = MaterialTheme.colorScheme.primaryContainer,
                        activeContentColor = MaterialTheme.colorScheme.onSurface,
                    ),
                    shape = SegmentedButtonDefaults.itemShape(
                        index = index,
                        count = NotificationPreview.entries.size,
                    ),
                    modifier = Modifier
                        .weight(1f)
                        .testTag(SettingsTestTags.preview(preview)),
                ) {
                    Text(preview.label)
                }
            }
        }


        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FilledTonalButton(
                onClick = onTestNotification,
                enabled = remindersEnabled,
                colors = subroutineTonalButtonColors(),
                modifier = Modifier
                    .weight(1f)
                    .testTag(SettingsTestTags.TestNotification),
            ) {
                Text("Test notification")
            }
            OutlinedButton(
                onClick = onOpenSystemNotificationSettings,
                modifier = Modifier
                    .weight(1f)
                    .testTag(SettingsTestTags.SystemNotificationSettings),
            ) {
                Text("Android settings")
            }
        }

    }
}

@Composable
private fun AccountSection(
    state: SettingsAccountState,
    onSignIn: (() -> Unit)?,
    onSignOut: () -> Unit,
) {
    SettingsSection(
        title = "Account",
        icon = Icons.Default.AccountCircle,
        modifier = Modifier.testTag(SettingsTestTags.Account),
    ) {
        when (state) {
            SettingsAccountState.SignedOutLocalOnly -> {
                SettingLabel(
                    title = "Signed out",
                    supportingText = "Data is stored only on this device.",
                )
                if (onSignIn != null) {
                    FilledTonalButton(
                        onClick = onSignIn,
                        colors = subroutineTonalButtonColors(),
                        modifier = Modifier.testTag(SettingsTestTags.SignIn),
                    ) {
                        Text("Sign in")
                    }
                }
            }

            is SettingsAccountState.NeedsAttention -> {
                SettingLabel(
                    title = state.username.displayUsernameOr("Account needs attention"),
                    supportingText = "Account sync is paused.",
                    supportingColor = MaterialTheme.colorScheme.error,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (onSignIn != null) {
                        FilledTonalButton(
                            onClick = onSignIn,
                            colors = subroutineTonalButtonColors(),
                            modifier = Modifier.testTag(SettingsTestTags.SignIn),
                        ) {
                            Text("Sign in again")
                        }
                    }
                    TextButton(
                        onClick = onSignOut,
                        modifier = Modifier.testTag(SettingsTestTags.SignOut),
                    ) {
                        Text("Sign out")
                    }
                }
            }

            is SettingsAccountState.SignOutFailed -> {
                SettingLabel(
                    title = "Sign-out incomplete",
                    supportingText = state.message,
                    supportingColor = MaterialTheme.colorScheme.error,
                )
                TextButton(
                    onClick = onSignOut,
                    modifier = Modifier.testTag(SettingsTestTags.SignOut),
                ) {
                    Text("Retry sign out")
                }
            }

            is SettingsAccountState.SignedIn -> {
                SettingLabel(
                    title = state.username.displayUsernameOr("Signed in"),
                )
                TextButton(
                    onClick = onSignOut,
                    modifier = Modifier.testTag(SettingsTestTags.SignOut),
                ) {
                    Text("Sign out")
                }
            }
        }
    }
}

@Composable
private fun DataSection(
    state: SettingsDataState,
    showRemoveAction: Boolean,
    onRemoveOfflineData: () -> Unit,
) {
    SettingsSection(
        title = "Data",
        icon = Icons.Default.Storage,
        modifier = Modifier.testTag(SettingsTestTags.Data),
    ) {
        SettingLabel(
            title = if (state.isRemoteAccountData) "Offline account data" else "Local-only data",
            supportingText = if (state.isRemoteAccountData) {
                "Downloaded to this device."
            } else {
                "This data is stored only on this device."
            },
        )
        if (state.pendingCount > 0) {
            Text(
                text = pendingStatusText(state.pendingCount, state.blockedCount),
                style = MaterialTheme.typography.bodyMedium,
                color = if (state.blockedCount > 0) {
                    MaterialTheme.colorScheme.error
                } else {
                    MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
        }
        if (showRemoveAction) {
            TextButton(
                onClick = onRemoveOfflineData,
                colors = ButtonDefaults.textButtonColors(
                    contentColor = MaterialTheme.colorScheme.error,
                ),
                modifier = Modifier.testTag(SettingsTestTags.RemoveOfflineData),
            ) {
                Icon(Icons.Default.Delete, contentDescription = null)
                Text(
                    text = "Remove offline account data",
                    modifier = Modifier.padding(start = 8.dp),
                )
            }
        }
    }
}

@Composable
private fun SettingsSection(
    title: String,
    icon: ImageVector,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.extraLarge,
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(title, style = MaterialTheme.typography.titleLarge)
            }
            content()
        }
    }
}

@Composable
private fun SettingLabel(
    title: String,
    supportingText: String? = null,
    supportingColor: androidx.compose.ui.graphics.Color = MaterialTheme.colorScheme.onSurfaceVariant,
) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(title, style = MaterialTheme.typography.bodyLarge)
        supportingText?.let { text ->
            Text(
                text,
                style = MaterialTheme.typography.bodySmall,
                color = supportingColor,
            )
        }
    }
}

private fun pendingStatusText(pendingCount: Int, blockedCount: Int): String = when {
    pendingCount == 0 -> "No pending changes."
    blockedCount == 0 -> "$pendingCount pending ${"change".pluralized(pendingCount)}."
    else -> "$pendingCount pending ${"change".pluralized(pendingCount)}, including " +
            "$blockedCount blocked."
}

private fun String.pluralized(count: Int): String = if (count == 1) this else "${this}s"

private fun String?.displayUsernameOr(fallback: String): String =
    this?.trim()?.takeIf(String::isNotEmpty) ?: fallback
