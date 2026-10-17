package com.example.subroutine_simple.ui.creator

import androidx.compose.ui.geometry.Rect
import com.example.subroutine_simple.ui.creator.presentation.CreatorFrame
import com.example.subroutine_simple.ui.creator.presentation.CreatorGeometry
import com.example.subroutine_simple.ui.creator.presentation.CreatorIme
import com.example.subroutine_simple.ui.creator.presentation.CreatorMotion
import com.example.subroutine_simple.ui.creator.presentation.CreatorTransition
import com.example.subroutine_simple.ui.creator.presentation.onInsets
import com.example.subroutine_simple.ui.creator.presentation.onTimedFinished
import com.example.subroutine_simple.ui.creator.presentation.onTimeout
import com.example.subroutine_simple.ui.creator.presentation.waitMillis
import com.example.subroutine_simple.ui.creator.presentation.runCreatorMotion
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertFalse
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CreatorMotionTest {
    private val fab = Rect(324f, 700f, 380f, 756f)
    private val editor = Rect(12f, 300f, 388f, 488f)
    private val withoutKeyboard = Rect(12f, 600f, 388f, 788f)

    @Test
    fun openingUsesNativeInsetsAndFreezesTheMeasuredDestination() {
        val geometry = CreatorGeometry()
        val leg = CreatorTransition(CreatorMotion.Keyboard(300), true, CreatorFrame(fab, 0f), 0, 300)
        assertEquals(fab, geometry.place(leg, fab, editor, 0, 0f).bounds)
        for (inset in 0..300) {
            // A content remeasurement must not move an active segment's endpoint.
            val resized = Rect(12f, 240f, 388f, 488f)
            val frame = geometry.place(leg, fab, resized, inset, 0f)
            assertTrue("Editor must stay above the moving keyboard", frame.bounds.bottom <= 800 - inset)
        }
        assertEquals(CreatorFrame(editor, 1f), geometry.frame)
        assertEquals(editor, geometry.place(leg, fab, withoutKeyboard, 320, 0f).bounds)
    }

    @Test
    fun interruptedOpeningAndReopeningPreserveActualRectangles() {
        val geometry = CreatorGeometry()
        val opening = CreatorTransition(CreatorMotion.Keyboard(300), true, CreatorFrame(fab, 0f), 0, 300)
        val interrupted = geometry.place(opening, fab, editor, 120, 0f)
        val waiting = CreatorTransition(CreatorMotion.Waiting, false, interrupted, 120, 300)
        assertEquals(interrupted, geometry.place(waiting, fab, withoutKeyboard, 120, 0f))
        val closing = CreatorTransition(CreatorMotion.Keyboard(0), false, interrupted, 120, 300)
        assertEquals(interrupted, geometry.place(closing, fab, withoutKeyboard, 120, 0f))
        val halfway = geometry.place(closing, fab, withoutKeyboard, 60, 0f)
        val reopened = CreatorTransition(CreatorMotion.Keyboard(300), true, halfway, 60, 300)
        assertEquals(halfway, geometry.place(reopened, fab, editor, 60, 0f))
        assertEquals(CreatorFrame(editor, 1f), geometry.place(reopened, fab, editor, 300, 0f))
        assertEquals(CreatorFrame(fab, 0f), geometry.place(closing, fab, withoutKeyboard, 0, 0f))
    }

    @Test
    fun alreadyMovingImeUsesItsRemainingNativeInterval() {
        val sample = CreatorIme(0, 120, 300, true)
        val phase = CreatorMotion.Waiting.onInsets(sample, opening = true)
        assertEquals(CreatorMotion.Keyboard(300), phase)
        val geometry = CreatorGeometry()
        val leg = CreatorTransition(phase, true, CreatorFrame(fab, 0f), sample.current, sample.target)
        assertEquals(fab, geometry.place(leg, fab, editor, 120, 0f).bounds)
        assertEquals(editor, geometry.place(leg, fab, editor, 300, 0f).bounds)
        assertEquals(CreatorMotion.Ready(300), phase.onInsets(sample.copy(current = 300), opening = true))
        assertEquals(
            CreatorMotion.Timed(300),
            CreatorMotion.Waiting.onInsets(CreatorIme(300, 300, 300, true), opening = true),
        )
    }

    @Test
    fun lateKeyboardCoalescesWithFallbackAndNeverStartsAnotherTimedMove() {
        val absent = CreatorIme(0, 0, 0, false)
        val fallback = CreatorMotion.Waiting.onTimeout(opening = true, ime = absent, progress = 0f)
        assertEquals(CreatorMotion.Timed(0), fallback)
        val geometry = CreatorGeometry()
        val timed = CreatorTransition(fallback, true, CreatorFrame(fab, 0f), 0, 0)
        val halfway = geometry.place(timed, fab, withoutKeyboard, 0, 0.5f)
        val arriving = CreatorIme(0, 0, 300, true)
        val phase = fallback.onInsets(arriving, opening = true)
        assertEquals(CreatorMotion.Keyboard(300), phase)
        val native = CreatorTransition(phase, true, halfway, 0, 300)
        assertEquals(halfway, geometry.place(native, fab, editor, 0, 0f))
        assertEquals(editor, geometry.place(native, fab, editor, 300, 0f).bounds)

        val ready = fallback.onTimedFinished(opening = true)
        assertEquals(CreatorMotion.Keyboard(300), ready.onInsets(arriving, opening = true))
        assertNull(ready.waitMillis(opening = true, elapsed = 2000, idle = 2000))
        assertEquals(CreatorMotion.Timed(0), CreatorMotion.Waiting.onInsets(absent.copy(visible = true), true))
    }

    @Test
    fun retargetAndCancellationDoNotWaitForAnObsoleteEndpoint() {
        val geometry = CreatorGeometry()
        val opening = CreatorTransition(CreatorMotion.Keyboard(300), true, CreatorFrame(fab, 0f), 0, 300)
        val interrupted = geometry.place(opening, fab, editor, 120, 0f)
        val taller = CreatorIme(120, 120, 450, true)
        val phase = opening.motion.onInsets(taller, opening = true)
        assertEquals(CreatorMotion.Keyboard(450), phase)
        val higherEditor = Rect(12f, 150f, 388f, 338f)
        val retarget = CreatorTransition(phase, true, interrupted, 120, 450)
        assertEquals(interrupted, geometry.place(retarget, fab, higherEditor, 120, 0f))
        assertEquals(higherEditor, geometry.place(retarget, fab, higherEditor, 450, 0f).bounds)

        val cancelled = phase.onInsets(CreatorIme(450, 200, 0, false), opening = true)
        assertEquals(CreatorMotion.Keyboard(0), cancelled)
        assertEquals(CreatorMotion.Ready(0), cancelled.onInsets(CreatorIme(0, 0, 0, false), opening = true))
        assertEquals(0L, phase.waitMillis(opening = true, elapsed = 4000, idle = 800))
        assertEquals(
            CreatorMotion.Ready(200),
            phase.onTimeout(opening = true, ime = taller.copy(current = 200), progress = 1f),
        )
    }

    @Test
    fun closeDeadlineSurvivesRetargetsAndIgnoresFurtherImeEvents() {
        val showing = CreatorIme(0, 200, 300, true)
        val closing = CreatorMotion.Keyboard(0).onInsets(showing, opening = false)
        assertEquals(CreatorMotion.Waiting, closing)
        assertEquals(1L, closing.waitMillis(opening = false, elapsed = 799, idle = 0))
        assertEquals(0L, closing.waitMillis(opening = false, elapsed = 800, idle = 0))
        val hidden = closing.onTimeout(opening = false, ime = showing, progress = 1f)
        assertEquals(CreatorMotion.Hidden, hidden)
        assertEquals(hidden, hidden.onInsets(showing.copy(target = 500), opening = false))
        val geometry = CreatorGeometry()
        val start = CreatorFrame(editor, 1f)
        val leg = CreatorTransition(hidden, false, start, showing.current, 300)
        assertEquals(CreatorFrame(fab, 0f), geometry.place(leg, fab, withoutKeyboard, 500, 0f))
        assertNull(hidden.waitMillis(opening = false, elapsed = 800, idle = 0))
    }

    @Test
    fun repeatedOpenCloseRunsHaveIndependentOwners() = motionTest {
        val geometry = CreatorGeometry()
        repeat(10) {
            val opening = MotionRun(this, true, CreatorIme(300, 300, 300, true), geometry)
            opening.await(CreatorMotion.Ready(300))
            opening.job.cancelAndJoin()
            assertEquals(0, opening.hidden)
            val closing = MotionRun(this, false, CreatorIme(300, 300, 300, true), geometry)
            closing.await(CreatorMotion.Waiting)
            closing.ime.value = CreatorIme(300, 150, 0, false)
            closing.await(CreatorMotion.Keyboard(0))
            closing.ime.value = CreatorIme(300, 0, 0, false)
            closing.job.join()
            assertEquals(1, closing.hidden)
            assertEquals(CreatorFrame(fab, 0f), geometry.frame)
        }
    }

    @Test
    fun reversedImeWhileClosingStillReleasesTheOwner() = motionTest {
        val closing = MotionRun(this, false, CreatorIme(300, 200, 0, false))
        closing.await(CreatorMotion.Keyboard(0))
        closing.ime.value = CreatorIme(200, 200, 300, true)
        closing.await(CreatorMotion.Waiting)
        closing.ime.value = CreatorIme(200, 100, 0, false)
        closing.await(CreatorMotion.Keyboard(0))
        closing.ime.value = CreatorIme(200, 0, 0, false)
        closing.job.join()
        assertEquals(1, closing.hidden)
    }

    @Test
    fun hardwareAndFloatingKeyboardsOpenAndCloseWithoutNativeMotion() = motionTest {
        for (visible in listOf(false, true)) {
            val absent = CreatorIme(0, 0, 0, visible)
            val opening = MotionRun(this, true, absent)
            opening.await(CreatorMotion.Ready(0))
            opening.job.cancelAndJoin()
            val closing = MotionRun(this, false, absent)
            closing.job.join()
            assertEquals(1, closing.hidden)
        }
    }

    @Test
    fun canceledAnimationDoesNotLoseItsCompletion() = motionTest {
        for (opening in listOf(true, false)) {
            val run = MotionRun(this, opening, CreatorIme(0, 0, 0, true), animate = {
                throw CancellationException("Animation interrupted without canceling its owner")
            })
            if (opening) {
                run.await(CreatorMotion.Ready(0))
                assertTrue(run.job.isActive)
                run.job.cancelAndJoin()
                assertEquals(0, run.hidden)
            } else {
                run.job.join()
                assertEquals(1, run.hidden)
            }
        }
    }

    @Test
    fun retargetCancelsAnObsoleteTimedAnimationWithoutCompletingTheNewLeg() = motionTest {
        val animating = CompletableDeferred<Unit>()
        val canceled = CompletableDeferred<Unit>()
        val opening = MotionRun(this, true, CreatorIme(0, 0, 0, true), animate = {
            animating.complete(Unit)
            try {
                awaitCancellation()
            } finally {
                canceled.complete(Unit)
            }
        })
        animating.await()
        opening.ime.value = CreatorIme(0, 100, 300, true)
        opening.await(CreatorMotion.Keyboard(300))
        canceled.await()
        assertTrue(opening.job.isActive)
        opening.ime.value = CreatorIme(0, 300, 300, true)
        opening.await(CreatorMotion.Ready(300))
        opening.job.cancelAndJoin()
        assertEquals(0, opening.hidden)
    }

    @Test
    fun disposalReleasesExitButDoesNotReportAnOpenCreatorHidden() = motionTest {
        for (opening in listOf(true, false)) {
            val run = MotionRun(this, opening, CreatorIme(0, 120, 300, true))
            run.await(CreatorMotion.Waiting)
            run.job.cancelAndJoin()
            assertEquals(if (opening) 0 else 1, run.hidden)
        }
    }

    @Test
    fun reopeningBeforeExitCompletionRejectsTheOldHiddenCallback() = motionTest {
        val closing = MotionRun(this, false, CreatorIme(300, 200, 0, false))
        closing.await(CreatorMotion.Keyboard(0))
        closing.visible = true
        closing.job.cancelAndJoin()
        assertEquals(0, closing.hidden)
        val opening = MotionRun(this, true, CreatorIme(200, 200, 300, true), closing.geometry)
        opening.await(CreatorMotion.Keyboard(300))
        opening.ime.value = CreatorIme(200, 300, 300, true)
        opening.await(CreatorMotion.Ready(300))
        opening.job.cancelAndJoin()
    }

    @Test
    fun foregroundFramesResumeTheSameOpeningOwner() = motionTest {
        val frames = CompletableDeferred<Unit>()
        val opening = MotionRun(this, true, CreatorIme(0, 0, 0, true), animate = { frames.await() })
        opening.await(CreatorMotion.Timed(0))
        assertTrue(opening.job.isActive)
        assertEquals(0, opening.hidden)
        frames.complete(Unit)
        opening.await(CreatorMotion.Ready(0))
        opening.job.cancelAndJoin()
    }

    @Test
    fun disposingDuringTimedExitReleasesWithoutAnAnimationCallback() = motionTest {
        val animating = CompletableDeferred<Unit>()
        val closing = MotionRun(this, false, CreatorIme(0, 0, 0, false), animate = {
            animating.complete(Unit)
            awaitCancellation()
        })
        animating.await()
        closing.job.cancelAndJoin()
        assertEquals(1, closing.hidden)
    }

    @Test
    fun stoppedFrameClockAndMissingImeCallbacksCannotRetainExitAcrossForegroundRestart() = motionTest {
        for (ime in listOf(CreatorIme(300, 300, 300, true), CreatorIme(0, 0, 0, false))) {
            val closing = MotionRun(this, false, ime, animate = { awaitCancellation() })
            closing.job.join()
            assertEquals(1, closing.hidden)
            assertFalse(closing.job.isActive)
            val resumed = MotionRun(this, true, CreatorIme(300, 300, 300, true))
            resumed.await(CreatorMotion.Ready(300))
            resumed.job.cancelAndJoin()
            assertEquals(0, resumed.hidden)
        }
    }

    private inner class MotionRun(
        scope: CoroutineScope,
        opening: Boolean,
        initialIme: CreatorIme,
        val geometry: CreatorGeometry = CreatorGeometry(),
        animate: suspend () -> Unit = {},
    ) {
        var visible = opening
        var hidden = 0
        val ime = MutableStateFlow(initialIme)
        private val phases = Channel<CreatorMotion>(Channel.UNLIMITED)
        val job: Job = scope.launch {
            runCreatorMotion(
                opening = opening,
                isVisible = { visible },
                initialIme = initialIme,
                insets = ime,
                now = { System.nanoTime() / 1_000_000 },
                progress = { geometry.frame?.progress ?: 0f },
                install = { motion, sample ->
                    val inset = when (motion) {
                        is CreatorMotion.Keyboard -> motion.target
                        is CreatorMotion.Timed -> motion.inset
                        is CreatorMotion.Ready -> motion.inset
                        else -> sample.current
                    }
                    val leg = CreatorTransition(motion, opening, geometry.frame, sample.current, inset)
                    geometry.place(leg, fab, if (inset > 0) editor else withoutKeyboard, sample.current, 0f)
                    phases.send(motion)
                    leg
                },
                animate = animate,
                onHidden = { hidden++ },
            )
        }

        suspend fun await(motion: CreatorMotion) {
            while (phases.receive() != motion) Unit
        }
    }

    private fun motionTest(block: suspend CoroutineScope.() -> Unit) = runBlocking {
        withTimeout(5_000, block)
    }
}
