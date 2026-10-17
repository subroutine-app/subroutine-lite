package com.example.subroutine_simple.ui.creator

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Insets
import android.os.Looper
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.WindowInsets as AndroidWindowInsets
import androidx.activity.ComponentActivity
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.node.RootForTest
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.createLifecycleAwareWindowRecomposer
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.getAllSemanticsNodes
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.ui.NavDisplay
import com.example.subroutine_simple.ui.creator.presentation.CreatorPresentation
import com.example.subroutine_simple.ui.theme.SubroutineSimpleTheme
import java.time.Duration
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
class CreatorPresentationIntegrationTest {
    private enum class Content { Basic, Parsed }

    private class Fixture(val content: Content, busy: Boolean = false) {
        val visible = MutableStateFlow(false)
        val saving = MutableStateFlow(busy)
        val submittingSession = MutableStateFlow<Int?>(null)
        val session = MutableStateFlow(0)
        val backStack = mutableStateListOf("home")
        val parserCalls = AtomicInteger()
        val parser = ItemParser { request ->
            parserCalls.incrementAndGet()
            ItemParseResponse(version = 1, title = request.text, warnings = emptyList(), highlights = emptyList())
        }
        var navigationVisible = false
        var overlayVisible = false
        var mounted = false
        var hiddenCalls = 0
        var settingsVisits = 0
        var imeBottom = 0
        var imeVisible = false
        val dismissAttempts = mutableListOf<Int>()
        val closes = mutableListOf<Int>()
    }

    private var controller: ActivityController<ComponentActivity>? = null
    private lateinit var view: ComposeView
    private lateinit var bitmap: Bitmap
    private lateinit var fixture: Fixture
    private val root: RootForTest get() = view.getChildAt(0) as RootForTest
    private val frameInterval = Duration.ofMillis(16)
    private var compositionFailure: Throwable? = null
    private var draws = 0

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
    fun basicFormOpensCancelsAndReopensWithoutImeOrUnrelatedNavigation() =
        exerciseCycles(Content.Basic, simulatedIme = false)

    @Test
    fun parsedFormOpensCancelsAndReopensWithoutImeOrUnrelatedNavigation() =
        exerciseCycles(Content.Parsed, simulatedIme = false)

    @Test
    @Config(sdk = [33])
    fun basicFormOpensCancelsAndReopensWithDispatchedImeInsets() =
        exerciseCycles(Content.Basic, simulatedIme = true)

    @Test
    @Config(sdk = [33])
    fun parsedFormOpensCancelsAndReopensWithDispatchedImeInsets() =
        exerciseCycles(Content.Parsed, simulatedIme = true)

    @Test
    fun unrelatedSavingCannotDisableTheInputOrTrapTheCreator() {
        render(Fixture(Content.Parsed, busy = true))
        repeat(2) { cycle ->
            tap(node("creator:fab"))
            frames(80)
            assertPresented()
            assertFalse(cancel().config.contains(SemanticsProperties.Disabled))
            assertFalse(input().config.contains(SemanticsProperties.Disabled))
            assertTrue("The unrelated operation must still be busy", fixture.saving.value)
            tap(cancel())
            assertClosed(cycle + 1)
        }
        assertEquals(listOf(1, 2), fixture.closes)
        assertEquals(0, fixture.settingsVisits)
    }

    @Test
    fun onlyTheActiveSubmissionDisablesControlsNotAnotherCreatorsPendingWrite() {
        render(Fixture(Content.Parsed))
        tap(node("creator:fab"))
        frames(80)
        assertPresented()
        val originalSession = fixture.session.value
        fixture.submittingSession.value = originalSession
        fixture.saving.value = true
        frames(8)
        assertTrue(cancel().config.contains(SemanticsProperties.Disabled))
        tapScrim()
        frames(4)
        assertTrue(fixture.visible.value)
        fixture.submittingSession.value = null
        frames(8)
        tap(cancel())
        assertClosed(1)
        fixture.submittingSession.value = originalSession
        tap(node("creator:fab"))
        frames(80)
        assertPresented()
        assertFalse(input().config.contains(SemanticsProperties.Disabled))
        assertFalse(cancel().config.contains(SemanticsProperties.Disabled))
        tapScrim()
        assertClosed(2)
        assertEquals(listOf(1, 1, 2), fixture.dismissAttempts)
        assertEquals(listOf(1, 2), fixture.closes)
        assertEquals(0, fixture.settingsVisits)
    }

    @Test
    fun reopeningDuringRetainedExitDoesNotLoseFocusOrRunTheOldHiddenCallback() {
        render(Fixture(Content.Parsed))
        tap(node("creator:fab"))
        frames(80)
        assertPresented()
        tap(cancel())
        frames(2)
        assertFalse(fixture.visible.value)
        assertTrue("This reproduction must reopen during the retained exit", fixture.mounted)
        assertEquals(0, fixture.hiddenCalls)
        tap(node("creator:fab"))
        frames(80)
        report("reopened during exit")
        assertPresented()
        assertEquals("The cancelled exit invoked onHidden for a new session", 0, fixture.hiddenCalls)
        tap(cancel())
        assertClosed(1)
        assertEquals(listOf(1, 2), fixture.closes)
        assertEquals(listOf(1, 2), fixture.dismissAttempts)
        assertEquals(0, fixture.settingsVisits)
    }

    private fun exerciseCycles(content: Content, simulatedIme: Boolean) {
        render(Fixture(content))
        repeat(2) { cycle ->
            assertFabPainted()
            tap(node("creator:fab"))
            frames(4)
            report("open ${cycle + 1}, first 64ms")
            assertTrue(fixture.visible.value)
            assertTrue("The navigation collector did not hide the FAB: ${receipt()}", fixture.navigationVisible)
            assertTrue("The overlay collector lagged behind navigation: ${receipt()}", fixture.overlayVisible)
            assertEquals("The FAB must actually stop painting", 0, pixels(android.graphics.Color.GREEN))
            if (simulatedIme) dispatchIme(300)
            frames(80)
            report("open ${cycle + 1}, settled")
            assertEquals(if (simulatedIme) 300 else 0, fixture.imeBottom)
            assertEquals(simulatedIme, fixture.imeVisible)
            assertPresented()
            assertEquals("Opening must not need an unrelated destination", listOf("home"), fixture.backStack.toList())
            assertEquals(0, fixture.settingsVisits)
            if (content == Content.Parsed) assertTrue("Fake parser was not reached", fixture.parserCalls.get() > cycle)
            tap(cancel())
            frames(2)
            assertFalse("Cancel did not close the current session: ${receipt()}", fixture.visible.value)
            assertFalse("Closing content retained input focus: ${receipt()}", isFocused())
            if (simulatedIme) dispatchIme(0)
            assertClosed(cycle + 1)
        }
        assertEquals("A retained callback closed the wrong session", listOf(1, 2), fixture.closes)
        assertEquals(listOf(1, 2), fixture.dismissAttempts)
        tap(node("settings"))
        frames(32)
        assertEquals(listOf("home", "settings"), fixture.backStack.toList())
        assertEquals(1, fixture.settingsVisits)
        node("settings-page")
        assertFalse("Unrelated navigation resurrected the creator", fixture.mounted)
    }

    @Composable
    private fun App(state: Fixture) {
        val presented by state.visible.collectAsStateWithLifecycle()
        val session by state.session.collectAsStateWithLifecycle()
        var origin by remember { mutableStateOf<Rect?>(null) }
        SideEffect { state.navigationVisible = presented }
        Box(Modifier.fillMaxSize().background(Color.White)) {
            NavDisplay(
                modifier = if (presented) Modifier.clearAndSetSemantics { } else Modifier,
                backStack = state.backStack,
                onBack = { state.backStack.removeLastOrNull() },
                transitionSpec = {
                    slideInHorizontally { it } togetherWith slideOutHorizontally { -it }
                },
                popTransitionSpec = {
                    slideInHorizontally { -it } togetherWith slideOutHorizontally { it }
                },
                entryProvider = { route ->
                    NavEntry(route) {
                        if (route == "home") {
                            Scaffold(
                                floatingActionButton = {
                                    FloatingActionButton(
                                        onClick = {
                                            if (!state.visible.value) {
                                                state.session.value++
                                                state.visible.value = true
                                            }
                                        },
                                        modifier = Modifier.onGloballyPositioned { origin = it.boundsInRoot() }
                                            .graphicsLayer { alpha = if (presented) 0f else 1f }
                                            .testTag("creator:fab")
                                            .then(if (presented) Modifier.clearAndSetSemantics { } else Modifier),
                                    ) {
                                        Box(Modifier.size(16.dp).background(Color.Green))
                                    }
                                },
                            ) { padding ->
                                TextButton(
                                    modifier = Modifier.padding(padding).testTag("settings"),
                                    onClick = { state.settingsVisits++; state.backStack.add("settings") },
                                ) { Text("Settings") }
                            }
                        } else {
                            Text("Settings destination", Modifier.testTag("settings-page"))
                        }
                    }
                },
            )
            Overlay(state, origin, presented, session)
            InsetsReceipt(state)
        }
    }

    @OptIn(ExperimentalLayoutApi::class)
    @Composable
    private fun InsetsReceipt(state: Fixture) {
        val bottom = WindowInsets.ime.getBottom(LocalDensity.current)
        val visible = WindowInsets.isImeVisible
        SideEffect {
            state.imeBottom = bottom
            state.imeVisible = visible
        }
    }

    @Composable
    private fun Overlay(state: Fixture, origin: Rect?, visible: Boolean, session: Int) {
        var retained by remember { mutableStateOf(visible) }
        LaunchedEffect(visible) { if (visible) retained = true }
        SideEffect { state.overlayVisible = visible }
        if (visible || retained) {
            val sharedSaving by state.saving.collectAsStateWithLifecycle()
            val submittingSession by state.submittingSession.collectAsStateWithLifecycle()
            val saving = submittingSession == session
            DisposableEffect(Unit) {
                state.mounted = true
                onDispose { state.mounted = false }
            }
            val dismiss = {
                state.dismissAttempts.add(session)
                if (!saving) {
                    state.closes.add(session)
                    state.visible.value = false
                }
                Unit
            }
            CreatorPresentation(
                visible = visible,
                origin = origin,
                onDismissRequest = dismiss,
                onHidden = {
                    state.hiddenCalls++
                    if (!state.visible.value) retained = false
                },
            ) { requester ->
                key(session) {
                    Column {
                        // A pixel witness inside the real recorded/transformed content layer.
                        Box(Modifier.size(20.dp).background(Color.Magenta).testTag("paint-witness"))
                        when (state.content) {
                            Content.Basic -> BasicForm(requester, saving || !visible, dismiss)
                            Content.Parsed -> {
                                var draft by remember { mutableStateOf(ParsedCreatorDraft.initial()) }
                                ParsedItemCreator(
                                    draft = draft,
                                    onDraftChange = { draft = it },
                                    focusRequester = requester,
                                    onCreate = { error("Opening/cancelling must not submit") },
                                    onCancel = dismiss,
                                    isSubmitting = saving || !visible,
                                    creationEnabled = !sharedSaving,
                                    parser = state.parser,
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    @Composable
    private fun BasicForm(requester: FocusRequester, disabled: Boolean, dismiss: () -> Unit) {
        var text by remember { mutableStateOf("") }
        BasicTextField(
            value = text,
            onValueChange = { text = it },
            enabled = !disabled,
            modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).focusRequester(requester)
                .semantics { contentDescription = "Item title" },
        )
        TextButton(onClick = dismiss, enabled = !disabled) { Text("Cancel") }
    }

    private fun render(state: Fixture) {
        fixture = state
        val activity = Robolectric.buildActivity(ComponentActivity::class.java)
            .also { controller = it }.setup().get()
        view = ComposeView(activity).apply {
            setParentCompositionContext(
                createLifecycleAwareWindowRecomposer(
                    coroutineContext = CoroutineExceptionHandler { _, failure -> compositionFailure = failure },
                    lifecycle = activity.lifecycle,
                ),
            )
            setContent { SubroutineSimpleTheme(darkTheme = false) { App(state) } }
        }
        bitmap = Bitmap.createBitmap(400, 1000, Bitmap.Config.ARGB_8888)
        activity.setContentView(view)
        controller?.visible()
        frames(2)
        controller?.windowFocusChanged(true)
        frames(14)
        assertFabPainted()
    }

    private fun frames(count: Int) {
        val looper = shadowOf(Looper.getMainLooper())
        repeat(count) {
            // AndroidUiDispatcher uses DefaultExecutor for delays: advancing Robolectric's
            // uptime alone does not expire the production withTimeoutOrNull(800).
            Thread.sleep(frameInterval.toMillis())
            looper.idleFor(frameInterval)
            assertCompositionHealthy()
            // No snapshot writes, setContent, or forced recompose/relayout in this pump.
            if (view.isLayoutRequested || view.width == 0) {
                view.measure(
                    View.MeasureSpec.makeMeasureSpec(400, View.MeasureSpec.EXACTLY),
                    View.MeasureSpec.makeMeasureSpec(1000, View.MeasureSpec.EXACTLY),
                )
                view.layout(0, 0, view.measuredWidth, view.measuredHeight)
            }
            bitmap.eraseColor(android.graphics.Color.TRANSPARENT)
            try {
                view.draw(Canvas(bitmap))
            } catch (failure: Throwable) {
                throw AssertionError("Android draw pass failed: ${receipt()}", failure)
            }
            draws++
            assertCompositionHealthy()
        }
    }

    private fun dispatchIme(bottom: Int) {
        val insets = AndroidWindowInsets.Builder()
            .setInsets(AndroidWindowInsets.Type.ime(), Insets.of(0, 0, 0, bottom))
            .setVisible(AndroidWindowInsets.Type.ime(), bottom > 0)
            .build()
        view.dispatchApplyWindowInsets(insets)
    }

    private fun tap(target: SemanticsNode) {
        val bounds = target.boundsInRoot
        assertBounds("touch target", bounds)
        tapAt(bounds.center.x, bounds.center.y)
    }

    private fun tapScrim() {
        val y = node("creator:surface").boundsInRoot.top / 2f
        assertTrue("No scrim above the form", y > 0)
        tapAt(200f, y)
    }

    private fun tapAt(x: Float, y: Float) {
        val down = SystemClock.uptimeMillis()
        for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
            val event =
                MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, x, y, 0)
            try {
                assertTrue(
                    "Android dispatchTouchEvent rejected $action at ($x, $y): ${receipt()}",
                    view.dispatchTouchEvent(event)
                )
            } finally {
                event.recycle()
            }
            if (action == MotionEvent.ACTION_DOWN) shadowOf(Looper.getMainLooper()).idleFor(frameInterval)
            assertCompositionHealthy()
        }
    }

    private fun assertPresented(expectFocus: Boolean = true) {
        assertTrue("Creator did not remain mounted: ${receipt()}", fixture.mounted)
        val surface = node("creator:surface").boundsInRoot
        assertBounds("surface", surface)
        assertEquals("Surface stayed FAB-sized instead of expanding: ${receipt()}", 376f, surface.width, 1f)
        assertTrue(
            "Form did not fit above the IME: ${receipt()}",
            surface.bottom <= view.height - fixture.imeBottom - 11f
        )
        assertBounds("input", input().boundsInRoot)
        assertBounds("Cancel", cancel().boundsInRoot)
        assertTrue("Cancel is clipped outside the form", surface.contains(cancel().boundsInRoot.center))
        assertEquals("Unexpected input focus: ${receipt()}", expectFocus, inputFocused())
        val witness = node("paint-witness").boundsInRoot
        assertEquals(
            "Form semantics exist, but its recorded layer is not visibly painted: ${receipt()}",
            android.graphics.Color.MAGENTA, bitmap.getPixel(witness.center.x.toInt(), witness.center.y.toInt())
        )
        assertTrue("Form witness was scaled/clipped/transparent", pixels(android.graphics.Color.MAGENTA) >= 350)
        assertEquals(0, fixture.settingsVisits)
    }

    private fun assertClosed(expected: Int) {
        frames(64)
        report("closed $expected")
        assertFalse("Visibility stayed true after Cancel: ${receipt()}", fixture.visible.value)
        assertFalse("Exit never released retained composition: ${receipt()}", fixture.mounted)
        assertEquals("onHidden did not finish exactly once per exit: ${receipt()}", expected, fixture.hiddenCalls)
        assertFalse(nodes().any { it.config.getOrNull(SemanticsProperties.TestTag) == "creator:surface" })
        assertFalse(isFocused())
        assertEquals("Closed form is still painted", 0, pixels(android.graphics.Color.MAGENTA))
        assertFabPainted()
    }

    private fun assertFabPainted() {
        assertBounds("FAB", node("creator:fab").boundsInRoot)
        assertTrue("FAB is not painted: ${receipt()}", pixels(android.graphics.Color.GREEN) >= 200)
    }

    private fun assertBounds(label: String, bounds: Rect) {
        assertTrue(
            "$label has no visible on-screen bounds ($bounds): ${receipt()}",
            bounds.width > 0 && bounds.height > 0 && bounds.left >= 0 && bounds.top >= 0 &&
                    bounds.right <= view.width + 1 && bounds.bottom <= view.height + 1
        )
    }

    private fun nodes(merged: Boolean = false) = root.semanticsOwner.getAllSemanticsNodes(mergingEnabled = merged)
    private fun node(tag: String): SemanticsNode = requireNotNull(nodes().singleOrNull {
        it.config.getOrNull(SemanticsProperties.TestTag) == tag
    }) { "Missing/duplicate '$tag': ${receipt()}" }

    private fun input() =
        nodes().single { it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains("Item title") == true }

    private fun cancel() = nodes(merged = true).single {
        it.config.getOrNull(SemanticsProperties.Text)?.any { text -> text.text == "Cancel" } == true &&
                it.config.contains(SemanticsActions.OnClick)
    }

    private fun inputFocused() = input().config.getOrNull(SemanticsProperties.Focused) == true
    private fun isFocused() = nodes().any { it.config.getOrNull(SemanticsProperties.Focused) == true }

    private fun pixels(color: Int): Int {
        val values = IntArray(bitmap.width * bitmap.height)
        bitmap.getPixels(values, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
        return values.count { it == color }
    }

    private fun receipt(): String = "t=${SystemClock.uptimeMillis()} draws=$draws " +
            "visible=${fixture.visible.value}/${fixture.navigationVisible}/${fixture.overlayVisible} " +
            "mounted=${fixture.mounted} hidden=${fixture.hiddenCalls} closes=${fixture.closes} " +
            "ime=${fixture.imeBottom}/${fixture.imeVisible} nav=${fixture.backStack} " +
            "nodes=${
                if (view.childCount == 0) emptyList() else nodes().map { node ->
                    Triple(
                        node.config.getOrNull(SemanticsProperties.TestTag) ?: node.config.getOrNull(SemanticsProperties.ContentDescription),
                        node.boundsInRoot, node.config.getOrNull(SemanticsProperties.Focused)
                    )
                }
            }"

    private fun report(stage: String) = println("$stage: ${receipt()}")
    private fun assertCompositionHealthy() {
        compositionFailure?.let {
            throw AssertionError(
                "Creator composition/effect failed after $draws draw passes",
                it
            )
        }
    }
}
