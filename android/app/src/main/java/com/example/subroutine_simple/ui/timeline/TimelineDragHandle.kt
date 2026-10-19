package com.example.subroutine_simple.ui.timeline

import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.awaitLongPressOrCancellation
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

@Composable
internal fun TimelineDragHandle(
    key: String,
    mode: TimelineEditMode,
    centerYPx: Float,
    leftPx: Float,
    widthPx: Float,
    viewportHeightPx: Float,
    onStart: (localPosition: Offset) -> Unit,
    onEdit: () -> Unit,
    onStep: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (centerYPx !in 0f..viewportHeightPx) return

    val halfTouchPx = with(LocalDensity.current) { 24.dp.toPx() }
    val currentStart by rememberUpdatedState(onStart)
    val currentEdit by rememberUpdatedState(onEdit)
    val currentStep by rememberUpdatedState(onStep)
    val interactionSource = remember(key, mode) { MutableInteractionSource() }
    val focused by interactionSource.collectIsFocusedAsState()
    val label = when (mode) {
        TimelineEditMode.Move -> "Move item"
        TimelineEditMode.Start -> "Start time"
        TimelineEditMode.End -> "End time"
    }
    Box(
        modifier = modifier
            .offset { IntOffset(leftPx.roundToInt(), (centerYPx - halfTouchPx).roundToInt()) }
            .width(with(LocalDensity.current) { widthPx.toDp() })
            .height(48.dp)
            .testTag("timeline:handle:${mode.name.lowercase()}:$key")
            .semantics {
                contentDescription = label
                role = Role.Button
                onClick(label = "Edit item") {
                    currentEdit()
                    true
                }
                customActions = listOf(
                    CustomAccessibilityAction("Earlier") { currentStep(-1); true },
                    CustomAccessibilityAction("Later") { currentStep(1); true },
                )
            }
            .onKeyEvent { event ->
                when (event.key) {
                    Key.Enter, Key.NumPadEnter, Key.DirectionCenter, Key.Spacebar -> {
                        if (event.type == KeyEventType.KeyUp) currentEdit()
                        true
                    }

                    Key.DirectionUp -> {
                        if (event.type == KeyEventType.KeyDown) currentStep(-1)
                        true
                    }

                    Key.DirectionDown -> {
                        if (event.type == KeyEventType.KeyDown) currentStep(1)
                        true
                    }

                    else -> false
                }
            }
            .focusable(interactionSource = interactionSource)
            .pointerInput(key, mode) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = true)
                    down.consume()
                    val held = awaitLongPressOrCancellation(down.id) ?: return@awaitEachGesture
                    currentStart(held.position)
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        if (focused) TimelineResizeGrip()
    }
}

@Composable
internal fun TimelineResizeGrip(modifier: Modifier = Modifier) {
    Box(
        modifier.size(width = 28.dp, height = 4.dp).background(
            color = MaterialTheme.colorScheme.primary,
            shape = MaterialTheme.shapes.extraSmall,
        ),
    )
}

@Composable
internal fun Modifier.timelineBodyDrag(
    key: String,
    onMoveStart: ((Offset) -> Unit)?,
    moveEarlier: (() -> Unit)?,
    moveLater: (() -> Unit)?,
): Modifier {
    val currentStart by rememberUpdatedState(onMoveStart)
    val currentEarlier by rememberUpdatedState(moveEarlier)
    val currentLater by rememberUpdatedState(moveLater)
    val drag = if (onMoveStart == null) Modifier else Modifier.pointerInput(key) {
        // The root owns the drag lifetime, even if this source is moved or culled.
        detectDragGesturesAfterLongPress(
            onDragStart = { currentStart?.invoke(it) },
            onDrag = { change, _ -> change.consume() },
        )
    }
    val actions = if (moveEarlier == null && moveLater == null) Modifier else Modifier.semantics {
        customActions = buildList {
            if (moveEarlier != null) add(CustomAccessibilityAction("Move earlier") {
                currentEarlier?.let { it(); true } ?: false
            })
            if (moveLater != null) add(CustomAccessibilityAction("Move later") {
                currentLater?.let { it(); true } ?: false
            })
        }
    }
    return then(drag).then(actions)
}
