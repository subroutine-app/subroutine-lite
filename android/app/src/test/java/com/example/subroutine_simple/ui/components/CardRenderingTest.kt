package com.example.subroutine_simple.ui.components

import android.os.Looper
import android.view.KeyEvent as AndroidKeyEvent
import android.view.View
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.node.RootForTest
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.createLifecycleAwareWindowRecomposer
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getAllSemanticsNodes
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.Density
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.ui.agenda.AgendaEntry
import com.example.subroutine_simple.ui.agenda.AgendaSection
import com.example.subroutine_simple.ui.agenda.UpcomingAgenda
import androidx.lifecycle.SavedStateHandle
import java.time.LocalDate
import com.example.subroutine_simple.ui.theme.SubroutineTypography
import java.time.Duration
import kotlinx.coroutines.CoroutineExceptionHandler
import org.junit.After
import org.junit.Before
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
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
@Config(manifest = Config.NONE, sdk = [28], qualifiers = "w400dp-h1000dp-mdpi")
@LooperMode(LooperMode.Mode.PAUSED)
class CardRenderingTest {
    private var controller: ActivityController<ComponentActivity>? = null
    private lateinit var view: ComposeView
    private val root: RootForTest get() = view.getChildAt(0) as RootForTest
    private val frameInterval = Duration.ofMillis(16)
    private var compositionFailure: Throwable? = null

    @Before
    fun controlFrames() {
        // A paused looper still auto-advances vsync unless the Choreographer is paused too.
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
    fun editCompletionAndContextOperationsRemainSeparate() {
        val entry = entry("task")
        val tag = cardTag(entry)
        var edits = 0
        var completions = 0
        var duplicates = 0
        render {
            CompoundItemCard(
                entry = entry,
                onEdit = { edits++ },
                onComplete = { completions++ },
                menuActions = listOf(ItemCardMenuAction("duplicate", "Duplicate action") { duplicates++ }),
            )
        }
        assertFalse(nodes().any { it.config.getOrNull(SemanticsProperties.TestTag) == "$tag:more" })
        click("$tag:identity")
        assertEquals(1, edits)
        assertEquals(0, completions)
        click("$tag:complete")
        assertEquals(1, edits)
        assertEquals(1, completions)

        val identity = node("$tag:identity")
        val actions = identity.config[SemanticsActions.CustomActions]
        assertEquals(listOf("Edit action", "Duplicate action"), actions.map { it.label })
        assertEquals("Actions for task", identity.config[SemanticsActions.OnLongClick].label)
        assertTrue(requireNotNull(identity.config[SemanticsActions.OnLongClick].action).invoke())
        assertEquals(1, edits)
        assertEquals(0, duplicates)
        assertTrue(actions.single { it.label == "Duplicate action" }.action())
        assertEquals(1, duplicates)

        assertTrue(requireNotNull(identity.config[SemanticsActions.RequestFocus].action).invoke())
        for ((key, modifiers) in listOf(
            AndroidKeyEvent.KEYCODE_MENU to 0,
            AndroidKeyEvent.KEYCODE_F10 to AndroidKeyEvent.META_SHIFT_ON,
        )) {
            assertTrue(
                root.sendKeyEvent(
                    KeyEvent(
                        AndroidKeyEvent(
                            0,
                            0,
                            AndroidKeyEvent.ACTION_DOWN,
                            key,
                            0,
                            modifiers
                        )
                    )
                )
            )
            assertTrue(root.sendKeyEvent(KeyEvent(AndroidKeyEvent(0, 0, AndroidKeyEvent.ACTION_UP, key, 0, modifiers))))
        }
        assertEquals(1, edits)
        assertEquals(1, completions)
    }

    @Test
    fun selectionAndCompletionSuppressItemMenusWithoutLosingSelectionTaps() {
        val entry = entry("selection")
        val tag = cardTag(entry)
        var selectionMode by mutableStateOf(true)
        var selected by mutableStateOf(false)
        var completing by mutableStateOf(false)
        render {
            CompoundItemCard(
                entry = entry,
                isCompleting = completing,
                onEdit = { error("Editing must not run during selection or completion") },
                onComplete = { error("Completion must not run during selection") },
                menuActions = listOf(ItemCardMenuAction("duplicate", "Duplicate action") { error("Unavailable") }),
                selection = if (selectionMode) ItemCardSelection(selected) { selected = !selected } else null,
            )
        }
        assertNoMenu("$tag:identity")
        click("$tag:identity")
        settle()
        assertTrue(selected)
        assertEquals(ToggleableState.On, node("$tag:select").config[SemanticsProperties.ToggleableState])
        click("$tag:select")
        assertFalse(selected)

        selectionMode = false
        completing = true
        settle()
        assertNoMenu("$tag:identity")
        assertFalse(node("$tag:identity").config.contains(SemanticsActions.OnClick))
    }

    @Test
    fun collapsedRowsAndInlineDraftShareHeightAndTitleAlignmentAtLargeFontScale() {
        val plain = entry("plain")
        val detailed = entry("detail").let {
            it.copy(
                action = it.action.copy(
                    title = "A very long title that must not grow this row ".repeat(8),
                    start = "2026-10-15T10:00:00Z",
                    duration = "PT30M",
                    content = "Notes stay hidden until expanded.\n".repeat(4),
                )
            )
        }
        val draft = InlineActionCreatorState("geometry", saved = null, persist = {})
        val creatorTag = "inline-creator:geometry"
        var scale by mutableStateOf(1f)
        render {
            CompositionLocalProvider(LocalDensity provides Density(1f, scale)) {
                Column {
                    CompoundItemCard(plain)
                    CompoundItemCard(detailed)
                    InlineActionCreator(draft, saving = false, onCreate = { _, _ -> })
                }
            }
        }
        for (fontScale in listOf(1f, 2f)) {
            scale = fontScale
            settle()
            val height = CardAppearance.rowGeometry(SubroutineTypography, Density(1f, scale)).height.value
            assertEquals(height, node(cardTag(plain)).boundsInRoot.height, 1f)
            assertEquals(height, node(cardTag(detailed)).boundsInRoot.height, 1f)
            assertEquals(height, node(creatorTag).boundsInRoot.height, 1f)
            assertFalse(nodes().any { it.config.getOrNull(SemanticsProperties.TestTag) == "${cardTag(detailed)}:notes" })

            val titleOffset = node("${cardTag(plain)}:title").boundsInRoot.top - node(cardTag(plain)).boundsInRoot.top
            assertEquals(
                titleOffset,
                node("$creatorTag:prompt").boundsInRoot.top - node(creatorTag).boundsInRoot.top,
                1f
            )
            val titleLeft = node("$creatorTag:prompt").boundsInRoot.left
            click("$creatorTag:expand")
            assertTrue(draft.expanded)
            settle()
            assertEquals(height, node(creatorTag).boundsInRoot.height, 1f)
            assertEquals(titleLeft, node("$creatorTag:title").boundsInRoot.left, 1f)
            assertTrue(node("$creatorTag:title").config[SemanticsProperties.Focused])
            for (action in listOf("submit", "cancel")) {
                val bounds = node("$creatorTag:$action").boundsInRoot
                assertTrue(bounds.width >= 48f && bounds.height >= 48f)
            }
            click("$creatorTag:cancel")
            settle()
        }
        click("${cardTag(detailed)}:notes-toggle")
        settle()
        assertTrue(node(cardTag(detailed)).boundsInRoot.height > node(cardTag(plain)).boundsInRoot.height)
        node("${cardTag(detailed)}:notes")
    }

    @Test
    fun inlineMoreOptionsRemainsAnExplicitCallbackAndDoesNotSubmit() {
        val draft = InlineActionCreatorState("options", saved = null, persist = {})
        var moreOptions = 0
        render {
            InlineActionCreator(
                draft, saving = false,
                onCreate = { _, _ -> error("More options must not submit") },
                onMoreOptions = { moreOptions++ },
            )
        }
        click("inline-creator:options:expand")
        assertTrue(draft.expanded)
        settle()
        val input = node("inline-creator:options:title")
        assertTrue(input.config[SemanticsProperties.Focused])
        assertTrue(requireNotNull(input.config[SemanticsActions.SetText].action).invoke(AnnotatedString("Unfinished title")))
        settle()
        click("inline-creator:options:more-options")
        assertEquals(1, moreOptions)
        assertEquals("Unfinished title", draft.title)
        assertEquals(80f, node("inline-creator:options:row").boundsInRoot.height, 1f)
    }

    @Test
    fun expandedDaysHaveHeaderAddButtonsAndOnlyAnActiveDraftTakesListSpace() {
        val today = LocalDate.of(2026, 10, 20)
        val dates = List(3) { today.plusDays(it.toLong()) }
        val sections = dates.map { date -> AgendaSection("day:$date", date, listOf(entry(date.toString()))) }
        val drafts = InlineActionDraftsViewModel(SavedStateHandle())
        val submitted = mutableListOf<Pair<LocalDate, String>>()
        render {
            CompositionLocalProvider(LocalInlineActionDrafts provides drafts) {
                UpcomingAgenda(
                    sections = sections,
                    today = today,
                    stateKey = "compact",
                    onEdit = {},
                    onCreateOnDate = { error("The header must edit locally") },
                    onCreateAction = { date, title, onSuccess, _ ->
                        submitted += date to title
                        onSuccess()
                    },
                )
            }
        }
        click("upcoming:toggle-all")
        settle()
        click("upcoming:toggle-all")
        settle()
        assertFalse(nodes().any {
            it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("agenda:create:") == true
        })
        dates.forEach { date ->
            val button = node("agenda:add:$date").boundsInRoot
            val header = node("agenda:section:day:$date").boundsInRoot
            assertEquals(48f, button.height, 1f)
            assertEquals(48f, button.width, 1f)
            assertTrue(header.contains(button.center))
        }
        val date = dates[1]
        click("agenda:add:$date")
        settle()
        val tag = "inline-creator:upcoming:compact:day:$date"
        assertTrue(node("agenda:create:$date").boundsInRoot.height >= 80f)
        val input = node("$tag:title")
        assertTrue(input.config[SemanticsProperties.Focused])
        assertTrue(requireNotNull(input.config[SemanticsActions.SetText].action).invoke(AnnotatedString("Read")))
        settle()
        click("$tag:submit")
        settle()
        assertEquals(listOf(date to "Read"), submitted)
        assertFalse(nodes().any { it.config.getOrNull(SemanticsProperties.TestTag) == "agenda:create:$date" })
        node("agenda:add:$date")
    }

    private fun render(content: @Composable () -> Unit) {
        val activity = Robolectric.buildActivity(ComponentActivity::class.java).also { controller = it }.setup().get()
        view = ComposeView(activity).apply {
            setParentCompositionContext(
                createLifecycleAwareWindowRecomposer(
                    coroutineContext = CoroutineExceptionHandler { _, failure -> compositionFailure = failure },
                    lifecycle = activity.lifecycle,
                )
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
            looper.idleFor(frameInterval)
            assertCompositionHealthy()
            view.measure(
                View.MeasureSpec.makeMeasureSpec(400, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(1000, View.MeasureSpec.EXACTLY),
            )
            view.layout(0, 0, view.measuredWidth, view.measuredHeight)
            root.measureAndLayoutForTest()
        }
    }

    private fun assertCompositionHealthy() {
        compositionFailure?.let { throw AssertionError("Card test composition failed", it) }
    }

    private fun nodes() = root.semanticsOwner.getAllSemanticsNodes(mergingEnabled = false)
    private fun node(tag: String): SemanticsNode {
        assertCompositionHealthy()
        val nodes = nodes()
        val matches = nodes.filter { it.config.getOrNull(SemanticsProperties.TestTag) == tag }
        val tags = nodes.mapNotNull { it.config.getOrNull(SemanticsProperties.TestTag) }
        assertEquals("Expected one node tagged '$tag'; present tags: $tags", 1, matches.size)
        return matches.single()
    }

    private fun click(tag: String) =
        assertTrue(requireNotNull(node(tag).config[SemanticsActions.OnClick].action).invoke())

    private fun assertNoMenu(tag: String) {
        assertFalse(node(tag).config.contains(SemanticsActions.OnLongClick))
        assertFalse(node(tag).config.contains(SemanticsActions.CustomActions))
    }

    private fun cardTag(entry: AgendaEntry) = "item-card:undated:${entry.key}"
    private fun entry(id: String) = AgendaEntry.ActionEntry(Action(id = id, recurrenceId = id, title = id))
}
