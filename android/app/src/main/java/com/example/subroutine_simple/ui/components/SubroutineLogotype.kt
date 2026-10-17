package com.example.subroutine_simple.ui.components

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.R
import com.example.subroutine_simple.ui.theme.SubroutineTheme

@Composable
internal fun SubroutineLogotype(modifier: Modifier = Modifier) {

    val appName = stringResource(R.string.app_name)
    Column(
        modifier = modifier
            .widthIn(max = 280.dp)
            .fillMaxWidth()
            .testTag("subroutine:logotype")
            .clearAndSetSemantics { contentDescription = appName },
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Image(
            painter = painterResource(R.drawable.subroutine_logotype_brick),
            contentDescription = null,
            colorFilter = ColorFilter.tint(SubroutineTheme.colors.brick),
            modifier = Modifier.fillMaxWidth().aspectRatio(3328f / 1024f),
        )
        Text(
            text = stringResource(R.string.app_edition),
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onSurface,
        )
    }
}
