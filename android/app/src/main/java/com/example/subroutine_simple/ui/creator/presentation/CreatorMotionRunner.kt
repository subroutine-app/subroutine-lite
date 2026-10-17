package com.example.subroutine_simple.ui.creator.presentation

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

private sealed interface CreatorMotionEvent {
    data class Insets(val ime: CreatorIme) : CreatorMotionEvent
    data class Finished(val transition: CreatorTransition) : CreatorMotionEvent
}

internal suspend fun runCreatorMotion(
    opening: Boolean,
    isVisible: () -> Boolean,
    initialIme: CreatorIme,
    insets: Flow<CreatorIme>,
    now: () -> Long,
    progress: () -> Float,
    install: suspend (CreatorMotion, CreatorIme) -> CreatorTransition,
    animate: suspend () -> Unit,
    onHidden: () -> Unit,
) {
    suspend fun drive() = coroutineScope {
        val events = Channel<CreatorMotionEvent>(Channel.UNLIMITED)
        val observer = launch { insets.collect { events.send(CreatorMotionEvent.Insets(it)) } }
        var animation: Job? = null
        var sample = initialIme
        var transition: CreatorTransition? = null
        var motion: CreatorMotion = CreatorMotion.Waiting
        val started = now()
        var changed = started

        suspend fun move(next: CreatorMotion) {
            animation?.cancelAndJoin()
            animation = null
            val leg = install(next, sample)
            transition = leg
            motion = next
            changed = now()
            if (next is CreatorMotion.Timed) {
                animation = launch {
                    try {
                        animate()
                    } catch (cancelled: CancellationException) {
                        // Animatable interruption is not necessarily cancellation of this owner.
                        currentCoroutineContext().ensureActive()
                    }
                    events.send(CreatorMotionEvent.Finished(leg))
                }
            }
        }

        try {
            move(motion)
            while (motion != CreatorMotion.Hidden) {
                val time = now()
                val timeout = motion.waitMillis(opening, time - started, time - changed)
                val event = if (timeout == null) events.receive() else {
                    withTimeoutOrNull(timeout) { events.receive() }
                }
                val next = when (event) {
                    null -> motion.onTimeout(opening, sample, progress())
                    is CreatorMotionEvent.Insets -> {
                        if (sample != event.ime) changed = now()
                        sample = event.ime
                        motion.onInsets(sample, opening)
                    }

                    is CreatorMotionEvent.Finished -> {
                        if (event.transition !== transition) continue
                        motion.onTimedFinished(opening)
                    }
                }
                if (next != motion) move(next)
            }
        } finally {
            observer.cancel()
            animation?.cancel()
            events.close()
        }
    }

    try {
        if (opening) drive() else {
            // The exit owns a bounded lifetime, not a promise from the IME or frame clock.
            withTimeoutOrNull(CreatorKeyboardTimeoutMillis) { drive() }
        }
    } finally {
        if (!opening && !isVisible()) onHidden()
    }
}
