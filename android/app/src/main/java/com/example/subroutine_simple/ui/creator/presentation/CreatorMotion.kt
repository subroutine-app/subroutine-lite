package com.example.subroutine_simple.ui.creator.presentation

import androidx.compose.ui.geometry.Rect

internal const val CreatorKeyboardTimeoutMillis = 800L

internal data class CreatorIme(val source: Int, val current: Int, val target: Int, val visible: Boolean) {
    val moving: Boolean get() = source != target || current != target
}

internal sealed interface CreatorMotion {
    data object Waiting : CreatorMotion
    data class Keyboard(val target: Int) : CreatorMotion
    data class Timed(val inset: Int) : CreatorMotion
    data class Ready(val inset: Int) : CreatorMotion

    data object Hidden : CreatorMotion
}

internal fun CreatorMotion.onInsets(ime: CreatorIme, opening: Boolean): CreatorMotion {
    if (this == CreatorMotion.Hidden) return this
    if (!opening && ime.target != 0) return CreatorMotion.Waiting

    fun follow(): CreatorMotion = when {
        ime.current != ime.target -> CreatorMotion.Keyboard(ime.target)
        opening -> CreatorMotion.Ready(ime.target)
        else -> CreatorMotion.Hidden
    }

    return when (this) {
        CreatorMotion.Waiting -> when {
            !opening && ime.current == 0 -> CreatorMotion.Timed(0)
            !opening -> follow()
            ime.target > 0 -> if (ime.moving) follow() else CreatorMotion.Timed(ime.target)
            ime.visible && !ime.moving && ime.current == 0 -> CreatorMotion.Timed(0)
            else -> this
        }

        is CreatorMotion.Keyboard -> follow()
        is CreatorMotion.Timed -> if (ime.target != inset || ime.current != ime.target) follow() else this
        is CreatorMotion.Ready -> if (ime.target != inset || ime.current != inset) follow() else this
        CreatorMotion.Hidden -> this
    }
}

internal fun CreatorMotion.onTimeout(opening: Boolean, ime: CreatorIme, progress: Float): CreatorMotion = when {
    !opening -> CreatorMotion.Hidden
    progress >= 1f -> CreatorMotion.Ready(ime.current)
    this == CreatorMotion.Waiting -> CreatorMotion.Timed(0)
    else -> CreatorMotion.Timed(ime.current)
}

internal fun CreatorMotion.onTimedFinished(opening: Boolean): CreatorMotion = when (this) {
    is CreatorMotion.Timed -> if (opening) CreatorMotion.Ready(inset) else CreatorMotion.Hidden

    else -> this
}

internal fun CreatorMotion.waitMillis(opening: Boolean, elapsed: Long, idle: Long): Long? = when {
    this == CreatorMotion.Hidden -> null
    !opening -> (CreatorKeyboardTimeoutMillis - elapsed).coerceAtLeast(0L)
    this == CreatorMotion.Waiting || this is CreatorMotion.Keyboard ->
        (CreatorKeyboardTimeoutMillis - idle).coerceAtLeast(0L)

    else -> null
}

internal data class CreatorKeyboardMotion(val fromInset: Int, val toInset: Int) {
    fun fraction(inset: Int): Float = if (fromInset == toInset) 1f else {
        ((inset - fromInset).toFloat() / (toInset - fromInset)).coerceIn(0f, 1f)
    }
}

internal data class CreatorFrame(val bounds: Rect, val progress: Float)

internal data class CreatorSegment(val start: CreatorFrame, val end: CreatorFrame) {
    fun frame(fraction: Float): CreatorFrame {
        val p = fraction.coerceIn(0f, 1f)
        return CreatorFrame(
            creatorBounds(start.bounds, end.bounds, p),
            start.progress + (end.progress - start.progress) * p,
        )
    }
}

internal class CreatorTransition(
    val motion: CreatorMotion,
    val opening: Boolean,
    val start: CreatorFrame?,
    val fromInset: Int,
    val measurementInset: Int,
) {
    fun fraction(inset: Int, clock: Float): Float = when (val phase = motion) {
        CreatorMotion.Waiting -> 0f
        is CreatorMotion.Keyboard -> CreatorKeyboardMotion(fromInset, phase.target).fraction(inset)
        is CreatorMotion.Timed -> clock
        is CreatorMotion.Ready, CreatorMotion.Hidden -> 1f
    }

    fun progress(inset: Int, clock: Float): Float {
        val from = start?.progress ?: 0f
        val goal = if (opening) 1f else 0f
        return from + (goal - from) * fraction(inset, clock)
    }
}

// Main-thread layout receipt, not observable state: effects can rebase without a layout/composition loop.
internal class CreatorGeometry {
    var frame: CreatorFrame? = null
        private set
    var inset: Int? = null
        private set
    var measurementInset: Int? = null
        private set
    private var transition: CreatorTransition? = null
    private var segment: CreatorSegment? = null

    fun place(
        next: CreatorTransition,
        origin: Rect,
        destination: Rect,
        currentInset: Int,
        clock: Float,
    ): CreatorFrame {
        if (transition !== next || next.motion is CreatorMotion.Ready) {
            segment = CreatorSegment(
                next.start ?: frame ?: CreatorFrame(origin, 0f),
                CreatorFrame(if (next.opening) destination else origin, if (next.opening) 1f else 0f),
            )
            transition = next
        }
        val placed = checkNotNull(segment).frame(next.fraction(currentInset, clock))
        frame = placed
        inset = currentInset
        measurementInset = next.measurementInset
        return placed
    }
}

internal fun creatorBounds(origin: Rect, destination: Rect, progress: Float): Rect {
    val p = progress.coerceIn(0f, 1f)
    fun mix(a: Float, b: Float) = a + (b - a) * p
    return Rect(
        mix(origin.left, destination.left), mix(origin.top, destination.top),
        mix(origin.right, destination.right), mix(origin.bottom, destination.bottom),
    )
}
