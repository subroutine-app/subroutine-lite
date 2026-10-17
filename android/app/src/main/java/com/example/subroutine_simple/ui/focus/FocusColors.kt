package com.example.subroutine_simple.ui.focus

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.ui.theme.SubroutineTheme

internal enum class FocusCue {
    None,
    Action,
    Attention,
    Context,
}

internal fun focusCue(
    event: FocusEventMoment? = null,
    signal: FocusSignalNotice? = null,
): FocusCue = when {
    event is FocusEventMoment.InProgress -> FocusCue.Action
    event is FocusEventMoment.Upcoming && event.withinNoticeThreshold -> FocusCue.Attention
    signal is FocusSignalNotice.Recent -> FocusCue.Context
    signal is FocusSignalNotice.Upcoming -> FocusCue.Attention
    else -> FocusCue.None
}

@Composable
internal fun focusCueColor(cue: FocusCue): Color = when (cue) {
    FocusCue.None -> MaterialTheme.colorScheme.onSurfaceVariant
    FocusCue.Action -> SubroutineTheme.colors.action
    FocusCue.Attention -> SubroutineTheme.colors.attention
    FocusCue.Context -> SubroutineTheme.colors.context
}

@Composable
internal fun Modifier.focusNoticeCue(cue: FocusCue): Modifier {
    if (cue == FocusCue.Action || cue == FocusCue.None) return this
    val color = focusCueColor(cue)
    return drawBehind {
        val x = if (layoutDirection == LayoutDirection.Ltr) -8.dp.toPx() else size.width + 8.dp.toPx()
        drawCircle(color = color, radius = 3.dp.toPx(), center = Offset(x, size.height / 2f))
    }
}
