package com.example.subroutine_simple.ui.timeline

import android.os.Looper
import android.view.View
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.Modifier
import androidx.compose.ui.node.RootForTest
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.createLifecycleAwareWindowRecomposer
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getAllSemanticsNodes
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.Density
import com.example.subroutine_simple.ui.theme.SubroutineTypography
import java.time.Duration
import java.time.Instant
import java.time.ZoneOffset
import kotlinx.coroutines.CoroutineExceptionHandler
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
import org.robolectric.annotation.LooperMode
import org.robolectric.shadows.ShadowChoreographer

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE, sdk = [28], qualifiers = "w400dp-h600dp-mdpi")
@LooperMode(LooperMode.Mode.PAUSED)
class TimelineAllDayInteractionTest {
    private var controller: ActivityController<ComponentActivity>? = null
    private lateinit var view: ComposeView
    private val root: RootForTest get() = view.getChildAt(0) as RootForTest
    private val frameInterval = Duration.ofMillis(16)
    private var compositionFailure: Throwable? = null

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
        assertCompositionHealthy()
    }

    @Test
    fun headerTogglesRealCardsAndTheListScrollsToTheFinalItem() {
        val start = Instant.parse("2026-10-15T00:00:00Z")
        val intervals = List(40) { index ->
            val id = "task-${index.toString().padStart(2, '0')}"
            TimelineIndexedInterval(
                key = "action:$id", kind = TimelineItemKind.Action, id = id,
                title = "Task $index", content = null,
                start = start, endExclusive = start.plusSeconds(86400), isDateSpan = true,
            )
        }
        val packing = packTimelineIntervals(intervals, 1.0, 1, 72f, 8f)
        val viewport = TimelineViewport(start.plusSeconds(300), 1.0)
        val density = Density(1f, 1f)
        var expandedDays by mutableStateOf(emptySet<Instant>())
        val edits = mutableListOf<String>()
        val headerTag = "timeline:all-day:$start:toggle"
        val listTag = "timeline:all-day:$start:items"
        val expectedCardTags = intervals.map { TimelineTestTags.interval(it.key) }.toSet()
        render {
            CompositionLocalProvider(LocalDensity provides density) {
                Box(Modifier.fillMaxSize()) {
                    val placement = packing.project(
                        viewport, 600f, expandedDays, timelineAllDayPreviewHeightPx(density),
                    ).allDay.days.single()
                    TimelineAllDayPreview(
                        placement = placement,
                        leftPx = 0f,
                        widthPx = 400f,
                        viewportHeightPx = 600f,
                        minimumCardHeightPx = 72f,
                        onToggle = {
                            expandedDays = if (start in expandedDays) expandedDays - start else expandedDays + start
                        },
                        enabled = true,
                    ) { card ->
                        TimelineIntervalCard(
                            placement = card,
                            zoneId = ZoneOffset.UTC,
                            leftPx = 0f,
                            contentWidthPx = 400f,
                            viewportHeightPx = 72f,
                            onClick = { edits += card.interval.key },
                            onComplete = null,
                            completing = false,
                            actionsEnabled = true,
                        )
                    }
                }
            }
        }

        assertHeader(headerTag, "Collapsed", "Expand all-day actions")
        assertTrue(cardTags().isEmpty())
        assertFalse(hasTag(listTag))
        assertTrue(nodes().any { node ->
            node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == "40 all-day actions" } == true
        })

        click(headerTag)
        assertEquals(setOf(start), expandedDays)
        assertHeader(headerTag, "Expanded", "Collapse all-day actions")
        val firstTag = TimelineTestTags.interval(intervals.first().key)
        val lastTag = TimelineTestTags.interval(intervals.last().key)
        assertTrue(hasTag(firstTag))
        assertFalse(hasTag(lastTag))
        assertTrue(expectedCardTags.containsAll(cardTags()))
        assertFalse(nodes().any {
            it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("timeline:density:") == true
        })

        val scroll = requireNotNull(node(listTag).config[SemanticsActions.ScrollToIndex].action)
        assertTrue(scroll(intervals.lastIndex))
        settle()
        val last = node(lastTag)
        val listBounds = node(listTag).boundsInRoot
        assertEquals(72f, last.boundsInRoot.height, 1f)
        assertTrue(last.boundsInRoot.top >= listBounds.top)
        assertTrue(last.boundsInRoot.bottom <= listBounds.bottom)
        assertTrue(expectedCardTags.containsAll(cardTags()))
        click(lastTag)
        assertEquals(listOf(intervals.last().key), edits)

        click(headerTag)
        assertTrue(expandedDays.isEmpty())
        assertHeader(headerTag, "Collapsed", "Expand all-day actions")
        assertTrue(cardTags().isEmpty())
        assertFalse(hasTag(listTag))
    }

    private fun render(content: @Composable () -> Unit) {
        val activity = Robolectric.buildActivity(ComponentActivity::class.java).also { controller = it }.setup().get()
        view = ComposeView(activity).apply {
            setParentCompositionContext(
                createLifecycleAwareWindowRecomposer(
                    coroutineContext = CoroutineExceptionHandler { _, failure -> compositionFailure = failure },
                    lifecycle = activity.lifecycle,
                ),
            )
            setContent { MaterialTheme(typography = SubroutineTypography) { content() } }
        }
        activity.setContentView(view)
        controller?.visible()
        settle()
    }

    private fun settle() {
        val looper = shadowOf(Looper.getMainLooper())
        repeat(32) {
            Snapshot.sendApplyNotifications()
            looper.idleFor(frameInterval)
            assertCompositionHealthy()
            view.measure(
                View.MeasureSpec.makeMeasureSpec(400, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
            )
            view.layout(0, 0, view.measuredWidth, view.measuredHeight)
            root.measureAndLayoutForTest()
        }
    }

    private fun assertCompositionHealthy() {
        compositionFailure?.let { throw AssertionError("All-day test composition failed", it) }
    }

    private fun nodes() = root.semanticsOwner.getAllSemanticsNodes(mergingEnabled = false)
    private fun hasTag(tag: String) = nodes().any { it.config.getOrNull(SemanticsProperties.TestTag) == tag }
    private fun cardTags() = nodes().mapNotNull { it.config.getOrNull(SemanticsProperties.TestTag) }
        .filter { it.startsWith("timeline:interval:") }.toSet()

    private fun node(tag: String): SemanticsNode {
        assertCompositionHealthy()
        val nodes = nodes()
        val matches = nodes.filter { it.config.getOrNull(SemanticsProperties.TestTag) == tag }
        val tags = nodes.mapNotNull { it.config.getOrNull(SemanticsProperties.TestTag) }
        assertEquals("Expected one node tagged '$tag'; present tags: $tags", 1, matches.size)
        return matches.single()
    }

    private fun click(tag: String) {
        assertTrue(requireNotNull(node(tag).config[SemanticsActions.OnClick].action).invoke())
        settle()
    }

    private fun assertHeader(tag: String, state: String, action: String) {
        val header = node(tag)
        assertEquals(state, header.config[SemanticsProperties.StateDescription])
        assertEquals(Role.Button, header.config[SemanticsProperties.Role])
        assertEquals(action, header.config[SemanticsActions.OnClick].label)
        assertTrue(header.boundsInRoot.height >= 48f)
    }
}
