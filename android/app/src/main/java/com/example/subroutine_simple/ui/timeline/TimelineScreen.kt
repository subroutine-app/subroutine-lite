package com.example.subroutine_simple.ui.timeline

import android.os.PowerManager
import androidx.compose.animation.core.AnimationState
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.animateDecay
import androidx.compose.animation.rememberSplineBasedDecay
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateCentroid
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.activity.compose.BackHandler

import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.PaddingValues

import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth

import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding


import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.FitScreen
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.key
import androidx.compose.runtime.withFrameNanos
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableDoubleStateOf

import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf

import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds

import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.geometry.Offset

import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.scrollBy
import androidx.compose.ui.semantics.semantics

import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.ui.theme.SubroutineTheme
import com.example.subroutine_simple.ui.creator.CreateItemRequest
import com.example.subroutine_simple.ui.creator.CreatorItemType

import java.time.Instant
import java.time.LocalDate

import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.Job
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlin.math.roundToLong
import kotlin.math.sign

private val DayTitleFormatter = DateTimeFormatter.ofPattern("EEEE, MMMM d, yyyy")

private val RulerWidth = 64.dp
private val TimelineOverscan = 200.dp
private val MinimumLaneWidth = 112.dp
private val MinimumCardHeight = 72.dp
private val CardGap = 8.dp
private val AnnotationTargetHeight = 48.dp
private const val ZoomStep = 2f
private const val RealtimeFrameIntervalNanos = 40_000_000L
private const val ZoomTransitionDurationMillis = 100
private const val ScrollTransitionDurationMillis = 200
private val TimelineEaseOutCubic = CubicBezierEasing(0.215f, 0.61f, 0.355f, 1f)

object TimelineTestTags {
    const val Root = "timeline"
    const val OuterPeriod = "timeline:outer-period"

    const val Empty = "timeline:empty"
    const val HourRuler = "timeline:hour-ruler"
    const val ZoomOut = "timeline:zoom-out"
    const val ZoomIn = "timeline:zoom-in"


    const val Now = "timeline:now"

    fun interval(key: String): String = "timeline:interval:$key"
    fun marker(key: String): String = "timeline:marker:$key"
    fun signal(key: String): String = "timeline:signal:$key"
    fun density(key: String): String = "timeline:$key"
    fun annotations(key: String): String = "timeline:annotations:$key"
    fun complete(key: String): String = "timeline:complete:$key"
}

@Composable
fun TimelineScreen(
    initialDate: LocalDate,
    zoneId: ZoneId,
    actions: List<Action>,
    events: List<Event>,
    markers: List<Marker>,
    signals: List<Signal>,
    onCenteredDateChange: (LocalDate) -> Unit,
    onActionClick: (String) -> Unit,
    onEventClick: (String) -> Unit,
    onMarkerClick: (String) -> Unit,
    onSignalClick: (String) -> Unit,
    creator: TimelineCreatorState,
    onCreate: (CreateItemRequest, String, () -> Unit) -> Unit,
    onApplyEdit: (TimelineEdit, (Boolean) -> Unit) -> Unit,
    creationError: String? = null,
    modifier: Modifier = Modifier,
    onCompleteAction: ((String) -> Unit)? = null,
    actionsEnabled: Boolean = true,
    completingActionIds: Set<String> = emptySet(),
    status: (@Composable () -> Unit)? = null,
) {
    val clockNowEpochMillis = rememberTimelineClockMillis()
    val initialClockEpochMillis = remember { clockNowEpochMillis }
    val initialCenter = remember(initialDate, zoneId, initialClockEpochMillis) {
        if (initialDate == Instant.ofEpochMilli(initialClockEpochMillis).atZone(zoneId).toLocalDate()) {
            Instant.ofEpochMilli(initialClockEpochMillis)
        } else {
            initialDate.atTime(12, 0).atZone(zoneId).toInstant()
        }
    }
    var centerOffsetMillis by rememberSaveable {
        mutableLongStateOf(
            TimelineRealtimeAnchor.fromCenter(initialClockEpochMillis, initialCenter).centerOffsetMillis,
        )
    }
    var secondsPerPixel by rememberSaveable {
        mutableDoubleStateOf(DEFAULT_TIMELINE_SECONDS_PER_PIXEL)
    }
    var viewportSize by remember { mutableStateOf(IntSize.Zero) }
    var lastReportedDate by remember { mutableStateOf(initialDate) }
    var flingJob by remember { mutableStateOf<Job?>(null) }
    var programmaticTransitionJob by remember { mutableStateOf<Job?>(null) }
    var densityDetails by remember { mutableStateOf<TimelineDensityPlacement?>(null) }
    var annotationDetails by remember { mutableStateOf<TimelineAnnotationPlacement?>(null) }
    var createMenu by remember { mutableStateOf<Instant?>(null) }
    var drag by remember { mutableStateOf<TimelineDrag?>(null) }
    var expandedAllDayDates by rememberSaveable { mutableStateOf(emptyList<String>()) }
    var viewportCoordinates by remember { mutableStateOf<LayoutCoordinates?>(null) }
    var creatorHeightPx by remember { mutableStateOf(0) }
    val haptic = LocalHapticFeedback.current
    val flingDecay = rememberSplineBasedDecay<Float>()
    val animationScope = rememberCoroutineScope()

    val realtimeAnchor = TimelineRealtimeAnchor(centerOffsetMillis)
    val now = Instant.ofEpochMilli(clockNowEpochMillis)
    val viewport = TimelineViewport(
        center = realtimeAnchor.centerAt(clockNowEpochMillis),
        secondsPerPixel = secondsPerPixel,
    )
    val density = LocalDensity.current
    val dataIndex = remember(zoneId, actions, events, markers, signals) {
        TimelineDataIndex.build(
            zoneId = zoneId,
            actions = actions,
            events = events,
            markers = markers,
            signals = signals,
        )
    }
    val centeredDate = viewport.center.atZone(zoneId).toLocalDate()
    val outerPeriod = timelineOuterPeriod(
        viewport, viewportSize.height.toFloat().coerceAtLeast(1f), zoneId, density.density,
    )
    val minimumCardHeightPx = with(density) {
        maxOf(MinimumCardHeight.toPx(), MaterialTheme.typography.titleSmall.lineHeight.toPx() * 2 + 24.dp.toPx())
    }
    val annotationTargetHeightPx = with(density) {
        maxOf(AnnotationTargetHeight.toPx(), MaterialTheme.typography.labelSmall.lineHeight.toPx() * 2 + 16.dp.toPx())
    }
    val animateViewportTo: (TimelineViewport, Boolean, Boolean) -> Unit =
        { target, animatePosition, animateScale ->
            flingJob?.cancel()
            programmaticTransitionJob?.cancel()
            val startOffsetMillis = centerOffsetMillis
            val targetOffsetMillis = TimelineRealtimeAnchor.fromCenter(
                clockNowEpochMillis,
                target.center,
            ).centerOffsetMillis
            val startSecondsPerPixel = secondsPerPixel
            programmaticTransitionJob = animationScope.launch {
                coroutineScope {
                    if (animatePosition && startOffsetMillis != targetOffsetMillis) {
                        launch {
                            animate(
                                initialValue = 0f,
                                targetValue = 1f,
                                animationSpec = tween(
                                    durationMillis = ScrollTransitionDurationMillis,
                                    easing = TimelineEaseOutCubic,
                                ),
                            ) { fraction, _ ->
                                centerOffsetMillis = startOffsetMillis +
                                        ((targetOffsetMillis - startOffsetMillis) * fraction).roundToLong()
                            }
                        }
                    } else {
                        centerOffsetMillis = targetOffsetMillis
                    }
                    if (animateScale && startSecondsPerPixel != target.secondsPerPixel) {
                        launch {
                            animate(
                                initialValue = 0f,
                                targetValue = 1f,
                                animationSpec = tween(
                                    durationMillis = ZoomTransitionDurationMillis,
                                    easing = TimelineEaseOutCubic,
                                ),
                            ) { fraction, _ ->
                                secondsPerPixel = startSecondsPerPixel +
                                        (target.secondsPerPixel - startSecondsPerPixel) * fraction
                            }
                        }
                    } else {
                        secondsPerPixel = target.secondsPerPixel
                    }
                }
            }
        }

    fun finishDrag() {
        val active = drag?.takeIf { it.phase == TimelineDragPhase.Dragging } ?: return
        haptic.performHapticFeedback(HapticFeedbackType.GestureEnd)
        if (!active.preview.changed) {
            drag = null
            return
        }
        val pending = active.copy(phase = TimelineDragPhase.Saving)
        drag = pending
        onApplyEdit(pending.preview) { saved ->
            if (drag == pending) {
                drag = null
                if (!saved) haptic.performHapticFeedback(HapticFeedbackType.Reject)
            }
        }
    }

    fun cancelDrag() {
        if (drag?.phase == TimelineDragPhase.Dragging) {
            drag = null
            haptic.performHapticFeedback(HapticFeedbackType.Reject)
        }
    }

    fun updateDrag(pointerYPx: Float, viewport: TimelineViewport, heightPx: Float) {
        val active = drag?.takeIf { it.phase == TimelineDragPhase.Dragging } ?: return
        val next = active.at(pointerYPx, viewport, heightPx, zoneId)
        drag = next
        if (next.preview.anchor != active.preview.anchor) {
            haptic.performHapticFeedback(HapticFeedbackType.SegmentTick)
        }
    }

    fun beginDrag(source: TimelineDragSource, mode: TimelineEditMode, pointerYPx: Float) {
        if (!actionsEnabled || drag != null || creator.anchor != null) return
        flingJob?.cancel()
        programmaticTransitionJob?.cancel()
        createMenu = null
        val original = source.original(mode)
        drag = TimelineDrag(
            source = source,
            division = divisionFor(secondsPerPixel, density.density),
            grabOffsetSeconds = secondsBetween(
                original.anchor,
                viewport.timeAt(pointerYPx, viewportSize.height.toFloat())
            ),
            pointerYPx = pointerYPx,
            preview = original,
        )
        haptic.performHapticFeedback(
            if (mode == TimelineEditMode.Move) HapticFeedbackType.LongPress
            else HapticFeedbackType.GestureThresholdActivate,
        )
    }

    fun stepItem(source: TimelineDragSource, mode: TimelineEditMode, direction: Int) {
        if (!actionsEnabled || drag != null || creator.anchor != null) return
        val original = source.original(mode)
        val division = divisionFor(secondsPerPixel, density.density).let {
            if (original is TimelineEdit.Interval && original.original.isDateSpan && it < TimelineDivision.Day) {
                TimelineDivision.Day
            } else it
        }
        val target = timelineStepInstant(original.anchor, division, zoneId, direction)
        val y = viewport.yAt(original.anchor, viewportSize.height.toFloat())
        beginDrag(source, mode, y)
        drag = drag?.copy(
            preview = when (original) {
                is TimelineEdit.Interval -> editTimelineInterval(original.original, mode, target, 0.0, division, zoneId)
                is TimelineEdit.Signal -> editTimelineSignal(original.original, target, 0.0, division, zoneId)
            },
        )
        finishDrag()
    }

    BackHandler(drag?.phase == TimelineDragPhase.Dragging) { cancelDrag() }
    val currentViewport by rememberUpdatedState(viewport)
    val currentClock by rememberUpdatedState(clockNowEpochMillis)
    val currentHeight by rememberUpdatedState(viewportSize.height.toFloat().coerceAtLeast(1f))
    LaunchedEffect(drag?.phase, density.density, zoneId) {
        if (drag?.phase != TimelineDragPhase.Dragging) return@LaunchedEffect
        var previous = withFrameNanos { it }
        while (isActive) {
            val frame = withFrameNanos { it }
            val elapsed = ((frame - previous) / 1_000_000_000f).coerceAtMost(0.05f)
            previous = frame
            val active = drag?.takeIf { it.phase == TimelineDragPhase.Dragging } ?: break
            val velocity = timelineAutoScrollVelocity(active.pointerYPx, currentHeight, density.density)
            if (velocity != 0f) {
                val next = TimelineViewport(
                    TimelineRealtimeAnchor(centerOffsetMillis).centerAt(currentClock), secondsPerPixel,
                ).panBy(-velocity * elapsed)
                centerOffsetMillis =
                    TimelineRealtimeAnchor.fromCenter(currentClock, next.center).centerOffsetMillis
                updateDrag(active.pointerYPx, next, currentHeight)
            }
        }
    }
    LaunchedEffect(creator.anchor, creatorHeightPx, viewportSize.height) {
        val anchor = creator.anchor ?: return@LaunchedEffect
        if (creatorHeightPx == 0 || viewportSize.height == 0) return@LaunchedEffect
        val padding = with(density) { 8.dp.toPx() }
        val y = currentViewport.yAt(anchor, currentHeight)
        val target = y.coerceIn(padding, maxOf(padding, currentHeight - creatorHeightPx - padding))
        if (target != y) {
            val next = currentViewport.panBy(target - y)
            centerOffsetMillis = TimelineRealtimeAnchor.fromCenter(clockNowEpochMillis, next.center).centerOffsetMillis
        }
    }

    val openInterval: (TimelineIndexedInterval) -> Unit = { interval ->
        flingJob?.cancel()
        programmaticTransitionJob?.cancel()
        when (interval.kind) {
            TimelineItemKind.Action -> onActionClick(interval.id)
            TimelineItemKind.Event -> onEventClick(interval.id)
            TimelineItemKind.Marker -> onMarkerClick(interval.id)
        }
    }

    val openAnnotation: (TimelineAnnotation) -> Unit = { annotation ->
        when (annotation) {
            is TimelineAnnotation.Marker -> onMarkerClick(annotation.interval.id)
            is TimelineAnnotation.Signal -> onSignalClick(annotation.signal.id)
        }
    }

    LaunchedEffect(initialDate) {
        if (initialDate != lastReportedDate) {
            animateViewportTo(
                viewport.copy(
                    center = initialDate.atTime(12, 0).atZone(zoneId).toInstant(),
                ),
                true,
                false,
            )
        }
    }
    LaunchedEffect(centeredDate) {
        lastReportedDate = centeredDate
        onCenteredDateChange(centeredDate)
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .testTag(TimelineTestTags.Root)
            .semantics {
                contentDescription = "Continuous timeline centered on ${centeredDate.format(DayTitleFormatter)}"
            },
    ) {
        status?.invoke()
        TimelineToolbar(
            outerPeriod = outerPeriod,

            canZoomIn = secondsPerPixel > MIN_TIMELINE_SECONDS_PER_PIXEL,
            canZoomOut = secondsPerPixel < MAX_TIMELINE_SECONDS_PER_PIXEL,
            onZoomIn = {
                animateViewportTo(
                    viewport.zoomBy(
                        scale = ZoomStep,
                        anchorYPx = viewportSize.height / 2f,
                        viewportHeightPx = viewportSize.height.toFloat().coerceAtLeast(1f),
                    ),
                    false,
                    true,
                )
            },
            onZoomOut = {
                animateViewportTo(
                    viewport.zoomBy(
                        scale = 1f / ZoomStep,
                        anchorYPx = viewportSize.height / 2f,
                        viewportHeightPx = viewportSize.height.toFloat().coerceAtLeast(1f),
                    ),
                    false,
                    true,
                )
            },
            onReset = {
                animateViewportTo(
                    viewport.copy(secondsPerPixel = RESET_TIMELINE_SECONDS_PER_PIXEL),
                    false,
                    true,
                )
            },
            onFitOuterPeriod = {
                outerPeriod?.let { period ->
                    animateViewportTo(
                        viewport.fit(period.range, viewportSize.height.toFloat().coerceAtLeast(1f)),
                        true,
                        true,
                    )
                }
            },
            onNow = {
                animateViewportTo(
                    viewport.copy(center = now),
                    true,
                    false,
                )
            },
        )

        Box(
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f),
        ) {
            val rulerWidthPx = with(density) { RulerWidth.toPx() }
            val overscanPx = maxOf(with(density) { TimelineOverscan.toPx() }, annotationTargetHeightPx)
            val viewportHeightPx = viewportSize.height.toFloat().coerceAtLeast(1f)
            val viewportWidthPx = viewportSize.width.toFloat().coerceAtLeast(1f)
            val visibleRange = viewport.visibleRange(viewportHeightPx, overscanPx)
            val visibleData = remember(dataIndex, visibleRange) { dataIndex.query(visibleRange) }
            val division = divisionFor(secondsPerPixel, density.density)
            val ticks = remember(visibleRange, zoneId, division) {
                timelineTicks(visibleRange, zoneId, division)
            }
            val gapPx = with(density) { CardGap.toPx() }
            val availableWidthPx = (viewportWidthPx - rulerWidthPx - gapPx * 2).coerceAtLeast(1f)
            val annotationWidthPx = (availableWidthPx * 0.3f)
                .coerceIn(with(density) { 80.dp.toPx() }, with(density) { 160.dp.toPx() })
                .coerceAtMost(
                    (availableWidthPx - gapPx - with(density) { 48.dp.toPx() })
                        .coerceAtLeast(with(density) { 48.dp.toPx() })
                )
            val contentWidthPx = (availableWidthPx - annotationWidthPx - gapPx).coerceAtLeast(1f)
            val itemLeftPx = rulerWidthPx + gapPx
            val annotationLeftPx = itemLeftPx + contentWidthPx + gapPx
            val maximumLanes = (contentWidthPx / with(density) { MinimumLaneWidth.toPx() })
                .toInt()
                .coerceIn(1, 4)
            val packing = remember(dataIndex, secondsPerPixel, maximumLanes, minimumCardHeightPx, gapPx) {
                packTimelineIntervals(
                    intervals = dataIndex.intervals,
                    secondsPerPixel = secondsPerPixel,
                    maximumLanes = maximumLanes,
                    minimumCardHeightPx = minimumCardHeightPx,
                    cardGapPx = gapPx,
                )
            }
            val allDayPreviewHeightPx = timelineAllDayPreviewHeightPx(density)
            val expandedDays = remember(expandedAllDayDates, zoneId) {
                expandedAllDayDates.map { LocalDate.parse(it).atStartOfDay(zoneId).toInstant() }.toSet()
            }
            val layout = remember(packing, viewport, viewportSize, expandedDays, allDayPreviewHeightPx) {
                packing.project(viewport, viewportHeightPx, expandedDays, allDayPreviewHeightPx)
            }
            val annotationPlacements =
                remember(visibleData, viewport, viewportSize, annotationTargetHeightPx, gapPx, drag?.preview?.key) {
                    layoutTimelineAnnotations(
                        data = visibleData.copy(
                            intervals = visibleData.intervals.filterNot { it.key == drag?.preview?.key },
                            signals = visibleData.signals.filterNot { it.key == drag?.preview?.key },
                        ),
                        viewport = viewport,
                        viewportHeightPx = viewportHeightPx,
                        touchTargetHeightPx = annotationTargetHeightPx,
                        gapPx = gapPx,
                    )
                }
            val nowY = viewport.yAt(now, viewportHeightPx)
            val canManipulate = actionsEnabled && drag == null && creator.anchor == null
            val currentCanManipulate by rememberUpdatedState(canManipulate)
            val openCreateMenu by rememberUpdatedState<(Float) -> Unit>({ y ->
                if (currentCanManipulate) {
                    createMenu = snapTimelineInstant(currentViewport.timeAt(y, viewportHeightPx), division, zoneId)
                }
            })
            val beginAction by rememberUpdatedState<(Float) -> Unit>({ y ->
                if (currentCanManipulate) {
                    val instant = snapTimelineInstant(currentViewport.timeAt(y, viewportHeightPx), division, zoneId)
                    creator.begin(instant, zoneId = zoneId)
                    haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                }
            })

            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .clipToBounds()
                    .background(MaterialTheme.colorScheme.surface)
                    .onSizeChanged { next ->
                        if (creator.anchor != null && viewportSize.height > 0 && next.height != viewportSize.height) {
                            val anchored = currentViewport.panBy((viewportSize.height - next.height) / 2f)
                            centerOffsetMillis = TimelineRealtimeAnchor.fromCenter(
                                clockNowEpochMillis,
                                anchored.center
                            ).centerOffsetMillis
                        }
                        viewportSize = next
                    }
                    .onGloballyPositioned { viewportCoordinates = it }
                    .testTag(TimelineTestTags.HourRuler)
                    .then(
                        timelineDragCapture(
                            isDragging = { drag?.phase == TimelineDragPhase.Dragging },
                            onPosition = { y -> updateDrag(y, currentViewport, viewportHeightPx) },
                            onFinish = ::finishDrag,
                            onCancel = ::cancelDrag,
                        )
                    )
                    .pointerInput(viewportSize, flingDecay) {
                        coroutineScope {
                            awaitEachGesture {
                                val firstDown = awaitFirstDown(requireUnconsumed = false)
                                flingJob?.cancel()
                                programmaticTransitionJob?.cancel()
                                val velocityTracker = VelocityTracker().apply {
                                    addPosition(firstDown.uptimeMillis, firstDown.position)
                                }
                                var maximumPointerCount = 1
                                var gestureStarted = false
                                var accumulatedPan = Offset.Zero
                                var event = awaitPointerEvent()
                                var cancelled = false
                                while (true) {
                                    if (drag != null || event.changes.any { it.isConsumed }) {
                                        cancelled = true
                                        break
                                    }
                                    if (event.changes.size == 1) {
                                        val change = event.changes.single()
                                        velocityTracker.addPosition(change.uptimeMillis, change.position)
                                    }
                                    if (event.changes.none { it.pressed }) break

                                    maximumPointerCount = maxOf(maximumPointerCount, event.changes.size)
                                    val pan = event.calculatePan()
                                    val zoom = event.calculateZoom()
                                    val centroid = event.calculateCentroid(useCurrent = true)
                                    var effectivePanY = pan.y
                                    if (!gestureStarted && event.changes.size > 1) {
                                        gestureStarted = true
                                    } else if (!gestureStarted) {
                                        accumulatedPan += pan
                                        if (
                                            abs(accumulatedPan.y) > viewConfiguration.touchSlop &&
                                            abs(accumulatedPan.y) > abs(accumulatedPan.x)
                                        ) {
                                            gestureStarted = true
                                            effectivePanY = accumulatedPan.y -
                                                    sign(accumulatedPan.y) * viewConfiguration.touchSlop
                                        }
                                    }

                                    if (gestureStarted && (effectivePanY != 0f || zoom != 1f)) {
                                        val transformed = TimelineViewport(
                                            center = TimelineRealtimeAnchor(centerOffsetMillis)
                                                .centerAt(currentClock),
                                            secondsPerPixel = secondsPerPixel,
                                        ).panBy(effectivePanY).zoomBy(
                                            scale = zoom,
                                            anchorYPx = centroid.y,
                                            viewportHeightPx = size.height.toFloat(),
                                        )
                                        centerOffsetMillis = TimelineRealtimeAnchor.fromCenter(
                                            currentClock,
                                            transformed.center,
                                        ).centerOffsetMillis
                                        secondsPerPixel = transformed.secondsPerPixel
                                        event.changes.forEach { it.consume() }
                                    }
                                    event = awaitPointerEvent()
                                }

                                if (!cancelled && gestureStarted && maximumPointerCount == 1) {
                                    val velocityY = velocityTracker.calculateVelocity().y
                                    if (abs(velocityY) >= 50f) {
                                        flingJob = launch {
                                            var previousValue = 0f
                                            AnimationState(
                                                initialValue = 0f,
                                                initialVelocity = velocityY,
                                            ).animateDecay(flingDecay) {
                                                val delta = value - previousValue
                                                previousValue = value
                                                val transformed = TimelineViewport(
                                                    center = TimelineRealtimeAnchor(centerOffsetMillis)
                                                        .centerAt(currentClock),
                                                    secondsPerPixel = secondsPerPixel,
                                                ).panBy(delta)
                                                centerOffsetMillis = TimelineRealtimeAnchor.fromCenter(
                                                    currentClock,
                                                    transformed.center,
                                                ).centerOffsetMillis
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    .pointerInput(Unit) {
                        detectTapGestures(
                            onTap = { openCreateMenu(it.y) },
                            onLongPress = { beginAction(it.y) },
                        )
                    }
                    .semantics {
                        contentDescription =
                            "Timeline. Actions and events on the left, markers and signals on the right."
                        scrollBy { _, y ->
                            animateViewportTo(viewport.panBy(-y), true, false)
                            true
                        }
                        customActions = listOf(
                            CustomAccessibilityAction("Earlier") {
                                animateViewportTo(viewport.stepPeriod(-1, zoneId, density.density), true, false)
                                true
                            },
                            CustomAccessibilityAction("Later") {
                                animateViewportTo(viewport.stepPeriod(1, zoneId, density.density), true, false)
                                true
                            },
                        ) + if (canManipulate) listOf(
                            CustomAccessibilityAction("Add item at centered time") {
                                openCreateMenu(viewportHeightPx / 2f)
                                true
                            },
                        ) else emptyList()
                    },
            ) {
                TimelineGrid(
                    ticks = ticks,
                    visibleIntervals = visibleData.intervals,
                    visibleSignals = visibleData.signals,
                    annotationLeftPx = annotationLeftPx,
                    viewport = viewport,
                    viewportHeightPx = viewportHeightPx,
                    rulerWidthPx = rulerWidthPx,
                    nowY = nowY,
                )

                ticks.forEach { tick ->
                    val y = viewport.yAt(tick.instant, viewportHeightPx)
                    if (tick.label.isNotEmpty() && y >= -24f && y <= viewportHeightPx + 24f) {
                        Text(
                            text = tick.label,
                            style = if (tick.major) MaterialTheme.typography.labelMedium else MaterialTheme.typography.labelSmall,
                            color = if (tick.major) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            modifier = Modifier
                                .offset {
                                    IntOffset(
                                        x = with(density) { 4.dp.roundToPx() },
                                        y = (y - with(density) { 8.dp.toPx() }).roundToInt(),
                                    )
                                }
                                .width(RulerWidth - 8.dp),
                        )
                    }
                }

                val timedContextTop = layout.allDay.topClearance(gapPx)
                layout.timed.cards.filter { it.interval.key != drag?.preview?.key }.forEach { placement ->
                    key(placement.interval.key) {
                        val source = TimelineDragSource.Card(placement)
                        val editable = canManipulate && placement.interval.id !in completingActionIds
                        TimelineIntervalCard(
                            placement = placement,
                            zoneId = zoneId,
                            leftPx = itemLeftPx,
                            contentWidthPx = contentWidthPx,
                            viewportHeightPx = viewportHeightPx,
                            stickyContextTopPx = timedContextTop,
                            onClick = { openInterval(placement.interval) },
                            onComplete = onCompleteAction?.let { complete -> { complete(placement.interval.id) } },
                            completing = placement.interval.id in completingActionIds,
                            actionsEnabled = actionsEnabled && drag == null,
                            onMoveStart = if (editable) { position ->
                                val top = placement.bounds.slice(viewportHeightPx)?.topPx ?: 0f
                                beginDrag(source, TimelineEditMode.Move, top + position.y)
                            } else null,
                            moveEarlier = if (editable) ({ stepItem(source, TimelineEditMode.Move, -1) }) else null,
                            moveLater = if (editable) ({ stepItem(source, TimelineEditMode.Move, 1) }) else null,
                        )
                        if (editable) {
                            val laneWidth = (contentWidthPx - gapPx * (placement.laneCount - 1)) / placement.laneCount
                            listOf(
                                TimelineEditMode.Start to placement.topPx,
                                TimelineEditMode.End to placement.bounds.bottomPx,
                            ).forEach { (mode, y) ->
                                TimelineDragHandle(
                                    key = placement.interval.key, mode = mode, centerYPx = y.toFloat(),
                                    leftPx = itemLeftPx + (laneWidth + gapPx) * placement.lane,
                                    widthPx = laneWidth,
                                    viewportHeightPx = viewportHeightPx,
                                    onStart = { position ->
                                        beginDrag(
                                            source,
                                            mode,
                                            y.toFloat() - with(density) { 24.dp.toPx() } + position.y)
                                    },
                                    onEdit = { openInterval(placement.interval) },
                                    onStep = { stepItem(source, mode, it) },
                                )
                            }
                        }
                    }
                }

                layout.timed.density.forEach { placement ->
                    key(placement.key) {
                        TimelineDensityCard(
                            placement = placement,
                            leftPx = itemLeftPx,
                            contentWidthPx = contentWidthPx,
                            viewportHeightPx = viewportHeightPx,
                            stickyContextTopPx = timedContextTop,
                            onClick = {
                                flingJob?.cancel()
                                programmaticTransitionJob?.cancel()
                                densityDetails = placement
                            },
                        )
                    }
                }

                layout.allDay.days.forEach { day ->
                    key(day.key) {
                        TimelineAllDayPreview(
                            placement = day,
                            leftPx = itemLeftPx,
                            widthPx = contentWidthPx,
                            viewportHeightPx = viewportHeightPx,
                            minimumCardHeightPx = minimumCardHeightPx,
                            enabled = drag == null && creator.anchor == null,
                            onToggle = {
                                flingJob?.cancel()
                                programmaticTransitionJob?.cancel()
                                val date = day.range.start.atZone(zoneId).toLocalDate().toString()
                                expandedAllDayDates = if (day.expanded) expandedAllDayDates - date
                                else (expandedAllDayDates + date).distinct()
                            },
                        ) { placement ->
                            val source = TimelineDragSource.Card(placement)
                            val editable = canManipulate && placement.interval.id !in completingActionIds
                            var coordinates by remember { mutableStateOf<LayoutCoordinates?>(null) }
                            if (placement.interval.key != drag?.preview?.key) {
                                TimelineIntervalCard(
                                    placement = placement,
                                    zoneId = zoneId,
                                    leftPx = 0f,
                                    contentWidthPx = contentWidthPx,
                                    viewportHeightPx = minimumCardHeightPx,
                                    modifier = Modifier.onGloballyPositioned { coordinates = it },
                                    onClick = { openInterval(placement.interval) },
                                    onComplete = onCompleteAction?.let { complete -> { complete(placement.interval.id) } },
                                    completing = placement.interval.id in completingActionIds,
                                    actionsEnabled = actionsEnabled && drag == null,
                                    onMoveStart = if (editable) { position ->
                                        val itemCoordinates = coordinates?.takeIf { it.isAttached }
                                        val rootCoordinates = viewportCoordinates?.takeIf { it.isAttached }
                                        if (itemCoordinates != null && rootCoordinates != null) {
                                            val y = rootCoordinates.localPositionOf(itemCoordinates, position).y
                                            beginDrag(source, TimelineEditMode.Move, y)
                                        }
                                    } else null,
                                    moveEarlier = if (editable) ({
                                        stepItem(
                                            source,
                                            TimelineEditMode.Move,
                                            -1
                                        )
                                    }) else null,
                                    moveLater = if (editable) ({
                                        stepItem(
                                            source,
                                            TimelineEditMode.Move,
                                            1
                                        )
                                    }) else null,
                                )
                            }
                        }
                    }
                }

                annotationPlacements.forEach { placement ->
                    key(placement.key) {
                        val source = TimelineDragSource.Annotation(placement)
                        val single = placement.annotations.singleOrNull()
                        val editable = canManipulate && single != null
                        TimelineAnnotationChip(
                            placement = placement,
                            zoneId = zoneId,
                            leftPx = annotationLeftPx,
                            widthPx = annotationWidthPx,
                            targetHeightPx = annotationTargetHeightPx,
                            onClick = {
                                flingJob?.cancel()
                                programmaticTransitionJob?.cancel()
                                placement.annotations.singleOrNull()?.let(openAnnotation)
                                    ?: run { annotationDetails = placement }
                            },
                            onMoveStart = if (editable) { position ->
                                beginDrag(source, TimelineEditMode.Move, placement.topPx + position.y)
                            } else null,
                            onMoveBy = if (editable) ({ stepItem(source, TimelineEditMode.Move, it) }) else null,
                        )
                        if (editable && single is TimelineAnnotation.Marker) {
                            listOf(
                                TimelineEditMode.Start to single.start,
                                TimelineEditMode.End to single.endExclusive
                            )
                                .forEach { (mode, instant) ->
                                    val y = viewport.yAt(instant, viewportHeightPx)
                                    TimelineDragHandle(
                                        key = single.key, mode = mode, centerYPx = y,
                                        leftPx = annotationLeftPx,
                                        widthPx = annotationWidthPx,
                                        viewportHeightPx = viewportHeightPx,
                                        onStart = { position ->
                                            beginDrag(
                                                source,
                                                mode,
                                                y - with(density) { 24.dp.toPx() } + position.y)
                                        },
                                        onEdit = { openAnnotation(single) },
                                        onStep = { stepItem(source, mode, it) },
                                    )
                                }
                        }
                    }
                }

                drag?.let {
                    TimelineDragPreview(
                        it, viewport, viewportHeightPx, itemLeftPx, contentWidthPx,
                        annotationLeftPx, annotationWidthPx, annotationTargetHeightPx, minimumCardHeightPx, zoneId,
                    )
                }
                createMenu?.let { instant ->
                    Box(Modifier.offset {
                        IntOffset(
                            itemLeftPx.roundToInt(),
                            viewport.yAt(instant, viewportHeightPx).roundToInt()
                        )
                    }) {
                        DropdownMenu(expanded = true, onDismissRequest = { createMenu = null }) {
                            listOf(
                                CreatorItemType.Action,
                                CreatorItemType.Event,
                                CreatorItemType.Marker,
                                CreatorItemType.Signal
                            )
                                .forEach { type ->
                                    DropdownMenuItem(
                                        text = { Text(type.label) },
                                        enabled = canManipulate,
                                        onClick = {
                                            createMenu = null
                                            creator.begin(instant, type, zoneId)
                                        },
                                        modifier = Modifier.testTag("timeline:create:${type.name.lowercase()}"),
                                    )
                                }
                        }
                    }
                }
                creator.anchor?.let { anchor ->
                    TimelineInlineCreator(
                        state = creator,
                        enabled = actionsEnabled,
                        errorMessage = creationError,
                        onCreate = onCreate,
                        modifier = Modifier
                            .offset {
                                IntOffset(
                                    itemLeftPx.roundToInt(),
                                    viewport.yAt(anchor, viewportHeightPx).roundToInt()
                                )
                            }
                            .width(with(density) { availableWidthPx.toDp() })
                            .heightIn(max = with(density) {
                                (viewportHeightPx - 16.dp.toPx()).coerceAtLeast(1f).toDp()
                            })
                            .onSizeChanged { creatorHeightPx = it.height }
                            .verticalScroll(rememberScrollState()),
                    )
                }

                if (dataIndex.isEmpty && creator.anchor == null && createMenu == null) {
                    Surface(
                        color = MaterialTheme.colorScheme.surfaceContainerLow,
                        contentColor = MaterialTheme.colorScheme.onSurface,
                        shape = MaterialTheme.shapes.large,
                        modifier = Modifier
                            .align(Alignment.Center)
                            .padding(start = RulerWidth + 16.dp, end = 16.dp)
                            .testTag(TimelineTestTags.Empty),
                    ) {
                        TextButton(enabled = canManipulate, onClick = {
                            openCreateMenu(viewportHeightPx / 2f)
                        }) {
                            Text("Add item", modifier = Modifier.padding(horizontal = 8.dp))
                        }
                    }
                }
            }
        }
    }

    annotationDetails?.let { placement ->
        val members = remember(dataIndex, placement) {
            val current = dataIndex.matching(placement.annotations.map { it.key }.toSet())
            buildList<TimelineAnnotation> {
                current.intervals.forEach { add(TimelineAnnotation.Marker(it)) }
                current.signals.forEach { add(TimelineAnnotation.Signal(it)) }
            }.sortedWith(compareBy({ it.start }, { it.key }))
        }
        val target = viewport.fit(placement.range, viewportSize.height.toFloat().coerceAtLeast(1f))
        AlertDialog(
            onDismissRequest = { annotationDetails = null },
            title = { Text("Markers & signals") },
            text = {
                LazyColumn(modifier = Modifier.heightIn(max = 400.dp)) {
                    if (members.isEmpty()) item { Text("These items are no longer on the timeline.") }
                    items(members, key = { it.key }) { annotation ->
                        TimelineDetailsRow(
                            title = annotation.title,
                            description = when (annotation) {
                                is TimelineAnnotation.Marker -> "Marker · ${annotation.interval.timeLabel(zoneId)}"
                                is TimelineAnnotation.Signal -> "Signal · ${
                                    annotation.start.atZone(zoneId).format(TimelineTimeFormatter)
                                }"
                            },
                            testTag = TimelineTestTags.annotations(annotation.key),
                            onClick = {
                                annotationDetails = null
                                openAnnotation(annotation)
                            },
                        )
                    }
                }
            },
            dismissButton = {
                if (members.isNotEmpty() && target.secondsPerPixel < secondsPerPixel) {
                    TextButton(onClick = {
                        annotationDetails = null
                        animateViewportTo(target, true, true)
                    }) { Text("Zoom in") }
                }
            },
            confirmButton = {
                TextButton(onClick = { annotationDetails = null }) { Text("Close") }
            },
        )
    }

    densityDetails?.let { placement ->
        val members = remember(dataIndex, placement) {
            dataIndex.matching(placement.intervals.map { it.key }.toSet()).intervals
        }
        val target = viewport.fit(placement.range, viewportSize.height.toFloat().coerceAtLeast(1f))
        AlertDialog(
            onDismissRequest = { densityDetails = null },
            title = { Text("Actions & events") },
            text = {
                LazyColumn(modifier = Modifier.heightIn(max = 400.dp)) {
                    if (members.isEmpty()) item { Text("These items are no longer on the timeline.") }
                    items(members, key = { it.key }) { interval ->
                        TimelineDetailsRow(
                            title = interval.title,
                            description = "${interval.kind} · ${interval.timeLabel(zoneId)}",
                            testTag = TimelineTestTags.interval(interval.key),
                            onClick = {
                                densityDetails = null
                                openInterval(interval)
                            },
                            onComplete = onCompleteAction?.takeIf { interval.kind == TimelineItemKind.Action }
                                ?.let { complete -> { complete(interval.id) } },
                            completing = interval.id in completingActionIds,
                            actionsEnabled = actionsEnabled,
                        )
                    }
                }
            },
            dismissButton = {
                if (members.isNotEmpty() && target.secondsPerPixel < secondsPerPixel) {
                    TextButton(onClick = {
                        densityDetails = null
                        animateViewportTo(target, true, true)
                    }) { Text("Zoom in") }
                }
            },
            confirmButton = {
                TextButton(onClick = { densityDetails = null }) { Text("Close") }
            },
        )
    }
}

@Composable
private fun TimelineToolbar(
    outerPeriod: TimelineOuterPeriod?,
    canZoomIn: Boolean,
    canZoomOut: Boolean,
    onZoomIn: () -> Unit,
    onZoomOut: () -> Unit,
    onReset: () -> Unit,
    onFitOuterPeriod: () -> Unit,
    onNow: () -> Unit,
) {
    val containerColor = MaterialTheme.colorScheme.surfaceContainerHigh
    val contentColor = MaterialTheme.colorScheme.onSurfaceVariant
    val zoomColors = IconButtonDefaults.filledTonalIconButtonColors(
        containerColor = containerColor,
        contentColor = contentColor,
    )
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(modifier = Modifier.weight(1f)) {
            outerPeriod?.let { period ->
                FilledTonalButton(
                    onClick = onFitOuterPeriod,
                    colors = ButtonDefaults.filledTonalButtonColors(
                        containerColor = containerColor,
                        contentColor = contentColor,
                    ),
                    contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp)
                        .widthIn(min = 48.dp)
                        .testTag(TimelineTestTags.OuterPeriod)
                        .semantics { contentDescription = "Fit ${period.description}" },
                ) {
                    Icon(Icons.Default.FitScreen, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(
                        period.label,
                        modifier = Modifier.weight(1f).padding(start = 8.dp),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        Box(
            modifier = Modifier
                .heightIn(min = 48.dp)
                .widthIn(min = 64.dp)
                .clip(ButtonDefaults.shape)
                .background(containerColor)
                .combinedClickable(
                    role = Role.Button,
                    onClickLabel = "Go to now",
                    onClick = onNow,
                    onLongClickLabel = "Reset timeline zoom",
                    onLongClick = onReset,
                )
                .testTag(TimelineTestTags.Now)
                .padding(horizontal = 12.dp, vertical = 8.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text("Now", color = contentColor, style = MaterialTheme.typography.labelLarge, maxLines = 1)
        }
        FilledTonalIconButton(
            onClick = onZoomOut,
            enabled = canZoomOut,
            colors = zoomColors,
            modifier = Modifier.size(48.dp).testTag(TimelineTestTags.ZoomOut),
        ) {
            Icon(Icons.Default.Remove, contentDescription = "Zoom timeline out")
        }
        FilledTonalIconButton(
            onClick = onZoomIn,
            enabled = canZoomIn,
            colors = zoomColors,
            modifier = Modifier.size(48.dp).testTag(TimelineTestTags.ZoomIn),
        ) {
            Icon(Icons.Default.Add, contentDescription = "Zoom timeline in")
        }
    }
}

@Composable
private fun TimelineGrid(
    ticks: List<TimelineTick>,
    visibleIntervals: List<TimelineIndexedInterval>,
    visibleSignals: List<TimelineIndexedSignal>,
    annotationLeftPx: Float,
    viewport: TimelineViewport,
    viewportHeightPx: Float,
    rulerWidthPx: Float,
    nowY: Float,
) {
    val outline = MaterialTheme.colorScheme.outlineVariant
    val majorOutline = MaterialTheme.colorScheme.outline
    val nowColor = SubroutineTheme.colors.currentTime
    val annotationColor = MaterialTheme.colorScheme.onSurfaceVariant
    Canvas(modifier = Modifier.fillMaxSize()) {
        ticks.forEach { tick ->
            val y = viewport.yAt(tick.instant, viewportHeightPx)
            drawLine(
                color = if (tick.major) majorOutline else outline,
                start = Offset(rulerWidthPx, y),
                end = Offset(size.width, y),
                strokeWidth = if (tick.major) 1.5f else 1f,
            )
        }
        drawLine(
            color = outline,
            start = Offset(annotationLeftPx - 4.dp.toPx(), 0f),
            end = Offset(annotationLeftPx - 4.dp.toPx(), size.height),
            strokeWidth = 1.dp.toPx(),
        )
        visibleIntervals.filter { it.kind == TimelineItemKind.Marker }.forEach { marker ->
            drawLine(
                color = annotationColor,
                start = Offset(
                    annotationLeftPx,
                    viewport.yAt(marker.start, viewportHeightPx).coerceIn(0f, size.height)
                ),
                end = Offset(
                    annotationLeftPx,
                    viewport.yAt(marker.endExclusive, viewportHeightPx).coerceIn(0f, size.height)
                ),
                strokeWidth = 2.dp.toPx(),
            )
        }
        visibleSignals.forEach { signal ->
            val y = viewport.yAt(signal.instant, viewportHeightPx)
            drawLine(
                color = annotationColor,
                start = Offset(annotationLeftPx - 2.dp.toPx(), y),
                end = Offset(annotationLeftPx + 2.dp.toPx(), y),
                strokeWidth = 1.dp.toPx(),
            )
        }
        if (nowY in -2f..(size.height + 2f)) {
            drawLine(
                color = nowColor,
                start = Offset(rulerWidthPx, nowY),
                end = Offset(size.width, nowY),
                strokeWidth = 3f,
            )
        }
    }
}


@Composable
private fun rememberTimelineClockMillis(): Long {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val powerManager = remember(context) { context.getSystemService(PowerManager::class.java) }
    var nowEpochMillis by remember { mutableLongStateOf(System.currentTimeMillis()) }

    LaunchedEffect(lifecycle, powerManager) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            var lastRealtimeFrameNanos = 0L
            while (isActive) {
                if (powerManager.isPowerSaveMode) {
                    delay(realtimeFrameIntervalMillis(powerSaveMode = true))
                    nowEpochMillis = System.currentTimeMillis()
                    lastRealtimeFrameNanos = 0L
                } else {
                    withFrameNanos { frameNanos ->
                        if (
                            lastRealtimeFrameNanos == 0L ||
                            frameNanos - lastRealtimeFrameNanos >= RealtimeFrameIntervalNanos
                        ) {
                            nowEpochMillis = System.currentTimeMillis()
                            lastRealtimeFrameNanos = if (lastRealtimeFrameNanos == 0L) {
                                frameNanos
                            } else {
                                lastRealtimeFrameNanos + RealtimeFrameIntervalNanos
                            }
                        }
                    }
                }
            }
        }
    }
    return nowEpochMillis
}
