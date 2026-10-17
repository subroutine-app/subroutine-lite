package com.example.subroutine_simple.ui.creator.presentation

import android.os.SystemClock
import android.util.Log
import com.example.subroutine_simple.BuildConfig
import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi

import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.absoluteOffset
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.imeAnimationSource
import androidx.compose.foundation.layout.imeAnimationTarget
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBars

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier

import androidx.compose.ui.draw.drawWithContent

import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.ui.theme.SubroutineTheme

import kotlin.math.roundToInt


@OptIn(ExperimentalLayoutApi::class)
@Composable
fun CreatorPresentation(
    visible: Boolean,
    origin: Rect?,
    onDismissRequest: () -> Unit,
    onHidden: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable (FocusRequester) -> Unit,
) {
    val density = LocalDensity.current
    val direction = LocalLayoutDirection.current
    val keyboard = LocalSoftwareKeyboardController.current
    val focus = LocalFocusManager.current
    val requester = remember { FocusRequester() }
    val ime = WindowInsets.ime
    val imeSource = WindowInsets.imeAnimationSource
    val imeTarget = WindowInsets.imeAnimationTarget
    val bars = WindowInsets.systemBars
    val safe = WindowInsets.safeDrawing
    val imeVisible = WindowInsets.isImeVisible
    val currentImeVisible by rememberUpdatedState(imeVisible)
    val currentVisible by rememberUpdatedState(visible)
    val hidden by rememberUpdatedState(onHidden)
    val dismiss by rememberUpdatedState(onDismissRequest)
    val frozenOrigin = remember { origin?.takeIf { it.width > 0 && it.height > 0 } }
    val clock = remember { Animatable(0f) }
    val geometry = remember { CreatorGeometry() }
    var transition by remember {
        mutableStateOf(CreatorTransition(CreatorMotion.Waiting, visible, null, 0, 0))
    }

    fun insets() = CreatorIme(
        imeSource.getBottom(density), ime.getBottom(density), imeTarget.getBottom(density), currentImeVisible,
    )

    fun progress(): Float = transition.progress(ime.getBottom(density), clock.value)

    LaunchedEffect(visible, density) {
        if (BuildConfig.DEBUG) Log.d("Creator", "presentation visible=$visible ime=${insets()}")
        if (visible) {
            val focused = requester.requestFocus()
            if (BuildConfig.DEBUG) Log.d("Creator", "focus accepted=$focused")
            keyboard?.show()
        } else {
            focus.clearFocus(force = true)
            keyboard?.hide()
        }
        runCreatorMotion(
            opening = visible,
            isVisible = { currentVisible },
            initialIme = insets(),
            insets = snapshotFlow { insets() },
            now = SystemClock::uptimeMillis,
            progress = { geometry.frame?.progress ?: 0f },
            install = { next, sample ->
                if (BuildConfig.DEBUG) Log.d("Creator", "motion=$next ime=$sample")
                val start = geometry.frame
                val fromInset = geometry.inset ?: sample.current
                val previousInset = geometry.measurementInset ?: sample.current
                val measurementInset = if (!visible) previousInset else when (next) {
                    is CreatorMotion.Keyboard -> next.target
                    is CreatorMotion.Timed -> next.inset
                    is CreatorMotion.Ready -> next.inset
                    else -> previousInset
                }
                clock.snapTo(0f)
                CreatorTransition(next, visible, start, fromInset, measurementInset).also { transition = it }
            },
            animate = { clock.animateTo(1f, tween(220)); Unit },
            onHidden = { hidden() },
        )
    }

    BackHandler(enabled = visible) { dismiss() }
    val p = progress()
    val colors = SubroutineTheme.colors
    Box(
        modifier.fillMaxSize().then(
            if (visible) Modifier.testTag("creator:overlay").semantics {
                paneTitle = "New item"
                isTraversalGroup = true
            } else Modifier.clearAndSetSemantics { },
        )
    ) {
        Box(
            Modifier.fillMaxSize()
                .graphicsLayer { alpha = progress() }
                .background(MaterialTheme.colorScheme.scrim.copy(alpha = 0.28f))
                .then(
                    if (visible) Modifier.clickable(
                        interactionSource = remember { MutableInteractionSource() },
                        indication = null,
                        onClick = { dismiss() },
                    ) else Modifier
                ).clearAndSetSemantics { },
        )
        Layout(
            modifier = Modifier.fillMaxSize(),
            content = {
                Box(
                    Modifier.background(lerp(colors.primaryFill, MaterialTheme.colorScheme.surfaceContainerLow, p))
                        .then(if (visible) Modifier.testTag("creator:surface") else Modifier),
                ) {
                    CreatorContent(
                        visible = visible,
                        modifier = Modifier.graphicsLayer {
                            alpha = ((progress() - 0.2f) / 0.8f).coerceIn(0f, 1f)
                        },
                    ) {
                        Box(Modifier.padding(16.dp)) { content(requester) }
                    }
                }
            },
        ) { children, constraints ->
            val leg = transition
            val margin = 12.dp.roundToPx()
            val leftInset = safe.getLeft(this, direction)
            val rightInset = safe.getRight(this, direction)
            val topInset = safe.getTop(this)
            val bottomInset = maxOf(leg.measurementInset, bars.getBottom(this))
            val width = (constraints.maxWidth - leftInset - rightInset - margin * 2)
                .coerceIn(1, 640.dp.roundToPx())
            val height = (constraints.maxHeight - topInset - bottomInset - margin * 2).coerceAtLeast(1)
            val child = children.single().measure(Constraints(minWidth = width, maxWidth = width, maxHeight = height))
            val left = leftInset + (constraints.maxWidth - leftInset - rightInset - width) / 2f
            val bottom = (constraints.maxHeight - bottomInset - margin).toFloat()
            val destination = Rect(left, bottom - child.height, left + width, bottom)
            val start = frozenOrigin ?: Rect(
                destination.right - 56.dp.toPx(), constraints.maxHeight - bars.getBottom(this) - margin - 56.dp.toPx(),
                destination.right, (constraints.maxHeight - bars.getBottom(this) - margin).toFloat(),
            )
            layout(constraints.maxWidth, constraints.maxHeight) {
                val bounds = geometry.place(leg, start, destination, ime.getBottom(density), clock.value).bounds
                child.placeWithLayer(0, 0) {
                    transformOrigin = TransformOrigin(0f, 0f)
                    translationX = bounds.left
                    translationY = bounds.top
                    scaleX = bounds.width / child.width.coerceAtLeast(1)
                    scaleY = bounds.height / child.height.coerceAtLeast(1)
                    shape = CreatorLayerShape(with(density) { CreatorAppearance.cornerRadius.toPx() }, scaleX, scaleY)
                    clip = true
                    shadowElevation = with(density) { 6.dp.toPx() } * progress()
                }
            }
        }
        if (frozenOrigin != null && p < 0.3f) {
            Box(
                modifier = Modifier.absoluteOffset {
                    IntOffset(
                        frozenOrigin.left.roundToInt(),
                        frozenOrigin.top.roundToInt()
                    )
                }
                    .size(with(density) { frozenOrigin.width.toDp() }, with(density) { frozenOrigin.height.toDp() })
                    .graphicsLayer { alpha = (1f - progress() / 0.3f).coerceIn(0f, 1f) }
                    .clearAndSetSemantics { },
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.Default.Add, null, tint = colors.onPrimaryFill)
            }
        }
    }
}

@Composable
private fun CreatorContent(visible: Boolean, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val layer = rememberGraphicsLayer()
    Layout(
        modifier = modifier.drawWithContent {
            if (visible) drawContent() else drawLayer(layer)
        },
        content = {
            Box(Modifier.drawWithContent {
                layer.record { this@drawWithContent.drawContent() }
                drawLayer(layer)
            }) { content() }
        },
    ) { children, constraints ->
        val child = children.single().measure(constraints)
        layout(child.width, child.height) {
            // Retain only painting on exit; unplaced content has no hit tests or semantics.
            if (visible) child.place(0, 0)
        }
    }
}
