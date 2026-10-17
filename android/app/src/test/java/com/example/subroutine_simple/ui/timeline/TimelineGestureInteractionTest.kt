package com.example.subroutine_simple.ui.timeline

import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.Looper
import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import androidx.activity.ComponentActivity
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.node.RootForTest
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.createLifecycleAwareWindowRecomposer
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getAllSemanticsNodes
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.Density
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.ui.theme.SubroutineTypography
import java.time.Duration
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset
import kotlinx.coroutines.CoroutineExceptionHandler
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.android.controller.ActivityController
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode
import org.robolectric.shadows.ShadowChoreographer

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [28], qualifiers = "w400dp-h1000dp-mdpi")
@LooperMode(LooperMode.Mode.PAUSED)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class TimelineGestureInteractionTest {
    private data class Submission(val edit: TimelineEdit, val complete: (Boolean) -> Unit)

    private val date = LocalDate.of(2000, 2, 3)
    private val eventStart = Instant.parse("2000-02-03T11:30:00Z")
    private val actionStart = Instant.parse("2000-02-03T12:30:00Z")
    private val creator = TimelineCreatorState("gesture-test", saved = null, persist = {})
    private val effects = mutableListOf<HapticFeedbackType>()
    private val submissions = mutableListOf<Submission>()
    private val haptic = object : HapticFeedback {
        override fun performHapticFeedback(hapticFeedbackType: HapticFeedbackType) {
            effects += hapticFeedbackType
        }
    }
    private var controller: ActivityController<ComponentActivity>? = null
    private lateinit var view: ComposeView
    private lateinit var bitmap: Bitmap
    private var snapSeconds = 0L
    private var snapDeltaPx = 0f
    private val root: RootForTest get() = view.getChildAt(0) as RootForTest
    private val frameInterval = Duration.ofMillis(16)
    private var compositionFailure: Throwable? = null
    private var downTime = 0L

    @Before
    fun controlFrames() {
        ShadowChoreographer.setPaused(true)
        ShadowChoreographer.setFrameDelay(frameInterval)
    }

    @After
    fun dispose() {
        if (::view.isInitialized) view.disposeComposition()
        controller?.pause()?.stop()?.destroy()
        shadowOf(Looper.getMainLooper()).idle()
        if (::bitmap.isInitialized) bitmap.recycle()
        assertCompositionHealthy()
    }

    @Test
    fun bothSidesOfEventStartResizeTickOnlyWhenTheSnappedPreviewChanges() {
        render()
        for (right in listOf(false, true)) {
            resize("event:event", TimelineEditMode.Start, right, saveSucceeds = true)
        }
    }

    @Test
    fun bothSidesOfActionEndHideTheGripOnReleaseAndRejectAFailedSave() {
        render()
        for (right in listOf(false, true)) {
            resize("action:action", TimelineEditMode.End, right, saveSucceeds = false)
        }
    }

    @Test
    fun androidCancellationRejectsAChangedResizeWithoutSaving() {
        render()
        val contact = edgeContact("event:event", TimelineEditMode.Start, right = false)
        touch(MotionEvent.ACTION_DOWN, contact)
        assertGrip("event:event", TimelineEditMode.Start)
        val moved = moveThroughOneSnap(contact, -snapDeltaPx, HapticFeedbackType.GestureThresholdActivate)
        touch(MotionEvent.ACTION_CANCEL, moved)
        assertIdle()
        assertTrue(submissions.isEmpty())
        assertEquals(
            listOf(
                HapticFeedbackType.GestureThresholdActivate,
                HapticFeedbackType.SegmentTick,
                HapticFeedbackType.Reject
            ),
            effects,
        )
    }

    @Test
    fun aSecondPointerRejectsTheDragAndTheRemainingReleaseCannotSave() {
        render()
        val contact = edgeContact("event:event", TimelineEditMode.Start, right = false)
        touch(MotionEvent.ACTION_DOWN, contact)
        val moved = moveThroughOneSnap(contact, -snapDeltaPx, HapticFeedbackType.GestureThresholdActivate)
        val second = moved + Offset(32f, 0f)
        touch(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), moved, second)
        assertIdle()
        assertTrue(submissions.isEmpty())
        touch(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), moved, second)
        touch(MotionEvent.ACTION_UP, moved)
        assertIdle()
        assertTrue(submissions.isEmpty())
        assertEquals(
            listOf(
                HapticFeedbackType.GestureThresholdActivate,
                HapticFeedbackType.SegmentTick,
                HapticFeedbackType.Reject
            ),
            effects,
        )
    }

    @Test
    fun actionBodyLongPressMovesWithoutAResizeGripAndPreservesDuration() {
        render()
        val contact = node(TimelineTestTags.interval("action:action")).boundsInRoot.center
        touch(MotionEvent.ACTION_DOWN, contact)
        assertTrue(effects.isEmpty())
        // The coroutine delay and Android frames use separate clocks under Robolectric.
        val deadline = System.nanoTime() + Duration.ofMillis(ViewConfiguration.getLongPressTimeout() + 1500L).toNanos()
        while (effects.isEmpty() && System.nanoTime() < deadline) {
            frames(2)
            Thread.sleep(10)
        }
        frames(2)
        assertEquals(listOf(HapticFeedbackType.LongPress), effects)
        assertNoGrip()
        assertNull(creator.anchor)
        val moved = moveThroughOneSnap(contact, snapDeltaPx, HapticFeedbackType.LongPress)
        assertNoGrip()
        touch(MotionEvent.ACTION_UP, moved)
        val pending = submissions.single()
        val edit = pending.edit as TimelineEdit.Interval
        assertEquals("action:action", edit.key)
        assertEquals(TimelineEditMode.Move, edit.mode)
        assertEquals(actionStart.plusSeconds(snapSeconds), edit.start)
        assertEquals(actionStart.plusSeconds(1800 + snapSeconds), edit.endExclusive)
        assertSaving()
        pending.complete(true)
        frames(4)
        assertIdle()
        assertEquals(
            listOf(
                HapticFeedbackType.LongPress,
                HapticFeedbackType.SegmentTick,
                HapticFeedbackType.GestureEnd
            ), effects
        )
    }

    private fun resize(key: String, mode: TimelineEditMode, right: Boolean, saveSucceeds: Boolean) {
        effects.clear()
        submissions.clear()
        assertIdle()
        val contact = edgeContact(key, mode, right)
        touch(MotionEvent.ACTION_DOWN, contact)
        assertEquals(listOf(HapticFeedbackType.GestureThresholdActivate), effects)
        assertGrip(key, mode)
        val moved = moveThroughOneSnap(
            contact,
            if (mode == TimelineEditMode.Start) -snapDeltaPx else snapDeltaPx,
            HapticFeedbackType.GestureThresholdActivate,
        )
        assertGrip(key, mode)
        assertTrue(submissions.isEmpty())
        touch(MotionEvent.ACTION_UP, moved)
        val pending = submissions.single()
        val edit = pending.edit as TimelineEdit.Interval
        assertEquals(key, edit.key)
        assertEquals(mode, edit.mode)
        assertTrue(edit.changed)
        if (mode == TimelineEditMode.Start) {
            assertEquals(eventStart.minusSeconds(snapSeconds), edit.start)
            assertEquals(eventStart.plusSeconds(1800), edit.endExclusive)
        } else {
            assertEquals(actionStart, edit.start)
            assertEquals(actionStart.plusSeconds(1800 + snapSeconds), edit.endExclusive)
        }
        assertSaving()
        val released = listOf(
            HapticFeedbackType.GestureThresholdActivate, HapticFeedbackType.SegmentTick, HapticFeedbackType.GestureEnd,
        )
        assertEquals(released, effects)
        pending.complete(saveSucceeds)
        frames(4)
        assertIdle()
        assertEquals(if (saveSucceeds) released else released + HapticFeedbackType.Reject, effects)
    }

    private fun moveThroughOneSnap(contact: Offset, deltaY: Float, activation: HapticFeedbackType): Offset {
        val initialLabel = dragLabel()
        for (delta in listOf(0.25f, 0.5f, 0.75f)) {
            touch(MotionEvent.ACTION_MOVE, contact + Offset(0f, delta))
            assertEquals(initialLabel, dragLabel())
            assertEquals(listOf(activation), effects)
        }
        val moved = contact + Offset(0f, deltaY)
        touch(MotionEvent.ACTION_MOVE, moved)
        val snappedLabel = dragLabel()
        assertNotEquals(initialLabel, snappedLabel)
        for (delta in listOf(0f, 0.25f, 0.5f, 0.25f, 0f)) {
            touch(MotionEvent.ACTION_MOVE, moved + Offset(0f, delta))
            assertEquals(snappedLabel, dragLabel())
            assertEquals(listOf(activation, HapticFeedbackType.SegmentTick), effects)
        }
        return moved
    }

    private fun edgeContact(key: String, mode: TimelineEditMode, right: Boolean): Offset {
        val card = node(TimelineTestTags.interval(key)).boundsInRoot
        val target = node("timeline:handle:${mode.name.lowercase()}:$key").boundsInRoot
        assertEquals(card.left, target.left, 1f)
        assertEquals(card.right, target.right, 1f)
        assertEquals(48f, target.height, 1f)
        return Offset(
            if (right) card.right - 2f else card.left + 2f,
            if (mode == TimelineEditMode.Start) card.top + 2f else card.bottom - 2f,
        )
    }

    private fun assertGrip(key: String, mode: TimelineEditMode) {
        val tag = "timeline:active-handle:${mode.name.lowercase()}:$key"
        assertEquals(listOf(tag), tags().filter { it.startsWith("timeline:active-handle:") })
        val grip = node(tag).boundsInRoot
        val card = node(TimelineTestTags.interval(key)).boundsInRoot
        assertEquals(card.center.x, grip.center.x, 1f)
        assertEquals(if (mode == TimelineEditMode.Start) card.top else card.bottom, grip.center.y, 1f)
    }

    private fun assertNoGrip() = assertFalse(tags().any { it.startsWith("timeline:active-handle:") })

    private fun assertIdle() {
        assertNoGrip()
        assertFalse(tags().contains("timeline:drag-time"))
        assertNull(creator.anchor)
    }

    private fun assertSaving() {
        assertNoGrip()
        assertTrue(dragLabel().startsWith("Saving…"))
    }

    private fun dragLabel() = node("timeline:drag-time").config[SemanticsProperties.Text].joinToString { it.text }

    private fun render() {
        val activity = Robolectric.buildActivity(ComponentActivity::class.java).also { controller = it }.setup().get()
        view = ComposeView(activity).apply {
            setParentCompositionContext(
                createLifecycleAwareWindowRecomposer(
                    coroutineContext = CoroutineExceptionHandler { _, failure -> compositionFailure = failure },
                    lifecycle = activity.lifecycle,
                ),
            )
            setContent {
                CompositionLocalProvider(LocalDensity provides Density(1f, 1f), LocalHapticFeedback provides haptic) {
                    MaterialTheme(typography = SubroutineTypography) {
                        TimelineScreen(
                            initialDate = date,
                            zoneId = ZoneOffset.UTC,
                            actions = listOf(
                                Action(
                                    id = "action", recurrenceId = "action", title = "Action", queued = true,
                                    start = actionStart.toString(), duration = "PT30M",
                                )
                            ),
                            events = listOf(
                                Event(
                                    id = "event", lineageId = "event", title = "Event",
                                    start = eventStart.toString(), duration = "PT30M",
                                )
                            ),
                            markers = emptyList(),
                            signals = emptyList(),
                            onCenteredDateChange = {},
                            onActionClick = { error("A drag must not open the action editor") },
                            onEventClick = { error("A drag must not open the event editor") },
                            onMarkerClick = { error("No marker in this fixture") },
                            onSignalClick = { error("No signal in this fixture") },
                            creator = creator,
                            onCreate = { _, _, _ -> error("A drag must not create an item") },
                            onApplyEdit = { edit, complete -> submissions += Submission(edit, complete) },
                        )
                    }
                }
            }
        }
        bitmap = Bitmap.createBitmap(400, 1000, Bitmap.Config.ARGB_8888)
        activity.setContentView(view)
        controller?.visible()
        frames(16)
        controller?.windowFocusChanged(true)
        frames(4)
        assertTrue(requireNotNull(node(TimelineTestTags.ZoomOut).config[SemanticsActions.OnClick].action).invoke())
        frames(16)
        val viewportBounds = node(TimelineTestTags.HourRuler).boundsInRoot
        assertTrue("Viewport did not lay out: $viewportBounds", viewportBounds.height >= 800f)
        for (key in listOf("event:event", "action:action")) {
            for (mode in listOf(TimelineEditMode.Start, TimelineEditMode.End)) {
                val edge = node("timeline:handle:${mode.name.lowercase()}:$key").boundsInRoot
                assertTrue(
                    "Edge outside viewport: $edge in $viewportBounds",
                    edge.top >= viewportBounds.top && edge.bottom <= viewportBounds.bottom
                )
            }
        }
        val eventTop = node(TimelineTestTags.interval("event:event")).boundsInRoot.top
        val actionTop = node(TimelineTestTags.interval("action:action")).boundsInRoot.top
        val secondsPerPixel = Duration.between(eventStart, actionStart).seconds.toDouble() / (actionTop - eventTop)
        snapSeconds = requireNotNull(divisionFor(secondsPerPixel, 1f).fixedSeconds)
        snapDeltaPx = (snapSeconds / secondsPerPixel * 0.6).toFloat()
        assertIdle()
        assertTrue(effects.isEmpty())
    }

    private fun touch(action: Int, position: Offset, second: Offset? = null) {
        val positions = listOfNotNull(position, second)
        if (action == MotionEvent.ACTION_DOWN) downTime = SystemClock.uptimeMillis()
        val event = MotionEvent.obtain(
            downTime, SystemClock.uptimeMillis(), action, positions.size,
            Array(positions.size) { index ->
                MotionEvent.PointerProperties().apply { id = index; toolType = MotionEvent.TOOL_TYPE_FINGER }
            },
            Array(positions.size) { index ->
                MotionEvent.PointerCoords().apply {
                    x = positions[index].x
                    y = positions[index].y
                    pressure = 1f
                    size = 1f
                }
            },
            0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0,
        )
        try {
            val handled = view.dispatchTouchEvent(event)
            // AndroidComposeView can discard identical MOVE samples before pointer dispatch.
            if (action != MotionEvent.ACTION_CANCEL && action != MotionEvent.ACTION_MOVE) {
                assertTrue("Touch $action at $positions was rejected; tags: ${tags()}", handled)
            }
        } finally {
            event.recycle()
        }
        frames(2)
    }

    private fun frames(count: Int) {
        repeat(count) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idleFor(frameInterval)
            assertCompositionHealthy()
            view.measure(
                View.MeasureSpec.makeMeasureSpec(400, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(1000, View.MeasureSpec.EXACTLY),
            )
            view.layout(0, 0, view.measuredWidth, view.measuredHeight)
            root.measureAndLayoutForTest()
            view.draw(Canvas(bitmap))
            assertCompositionHealthy()
        }
    }

    private fun assertCompositionHealthy() {
        compositionFailure?.let { throw AssertionError("Timeline gesture composition failed", it) }
    }

    private fun nodes() = root.semanticsOwner.getAllSemanticsNodes(mergingEnabled = false)
    private fun tags() = nodes().mapNotNull { it.config.getOrNull(SemanticsProperties.TestTag) }
    private fun node(tag: String): SemanticsNode {
        assertCompositionHealthy()
        val matches = nodes().filter { it.config.getOrNull(SemanticsProperties.TestTag) == tag }
        assertEquals(
            "Expected one node tagged '$tag'; nodes: ${nodes().map { it.config.getOrNull(SemanticsProperties.TestTag) to it.boundsInRoot }}",
            1,
            matches.size
        )
        return matches.single()
    }
}
