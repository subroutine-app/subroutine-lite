package com.example.subroutine_simple.ui.components

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp

@Composable
fun SyncStatusBanner(
    message: String,
    conflict: Boolean,
    onUseRemote: () -> Unit,
    onKeepLocal: () -> Unit,
    modifier: Modifier = Modifier,
    actionsEnabled: Boolean = true,
) {
    Surface(
        color = if (conflict) {
            MaterialTheme.colorScheme.errorContainer
        } else {
            MaterialTheme.colorScheme.surfaceContainerLow
        },
        contentColor = if (conflict) {
            MaterialTheme.colorScheme.onErrorContainer
        } else {
            MaterialTheme.colorScheme.onSurface
        },
        shape = MaterialTheme.shapes.large,
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 6.dp)
            .testTag("sync:status"),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 18.dp, vertical = 14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(text = message, style = MaterialTheme.typography.bodyMedium)
            if (conflict) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
                ) {
                    TextButton(
                        onClick = onUseRemote,
                        enabled = actionsEnabled,
                        modifier = Modifier.testTag("sync:use-remote"),
                    ) { Text("Use server version") }
                    TextButton(
                        onClick = onKeepLocal,
                        enabled = actionsEnabled,
                        modifier = Modifier.testTag("sync:keep-local"),
                    ) { Text("Keep my changes") }
                }
            }
        }
    }
}
