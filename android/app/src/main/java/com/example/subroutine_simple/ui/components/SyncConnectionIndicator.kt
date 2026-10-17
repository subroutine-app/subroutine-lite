package com.example.subroutine_simple.ui.components

import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CloudDone
import androidx.compose.material.icons.filled.CloudOff
import androidx.compose.material.icons.filled.SyncProblem
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.ui.SyncConnectionStatus
import kotlinx.coroutines.launch

@Composable
fun SyncConnectionIndicator(
    status: SyncConnectionStatus,
    modifier: Modifier = Modifier,
) {
    val label = when (status) {
        SyncConnectionStatus.Connecting -> "Connecting"
        SyncConnectionStatus.Connected -> "Connected"
        SyncConnectionStatus.Offline -> "Offline"
        SyncConnectionStatus.LocalOnly -> return
        SyncConnectionStatus.SignInRequired -> "Sign in to sync"
    }
    val tooltip = rememberTooltipState()
    val scope = rememberCoroutineScope()
    TooltipBox(
        modifier = modifier,
        positionProvider = TooltipDefaults.rememberTooltipPositionProvider(),
        tooltip = { PlainTooltip { Text(label) } },
        state = tooltip,
    ) {
        IconButton(
            onClick = { scope.launch { tooltip.show() } },
            modifier = Modifier
                .testTag("sync:connection")
                .semantics { contentDescription = "Sync: $label" },
        ) {
            if (status == SyncConnectionStatus.Connecting) {
                CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
            } else {
                Icon(
                    imageVector = when (status) {
                        SyncConnectionStatus.Connected -> Icons.Default.CloudDone
                        SyncConnectionStatus.SignInRequired -> Icons.Default.SyncProblem
                        else -> Icons.Default.CloudOff
                    },
                    contentDescription = null,
                    tint = if (status == SyncConnectionStatus.SignInRequired) {
                        MaterialTheme.colorScheme.error
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    },
                    modifier = Modifier.size(18.dp),
                )
            }
        }
    }
}
