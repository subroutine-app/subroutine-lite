package com.example.subroutine_simple.ui.calendar

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn

import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.CutCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material3.Button

import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.ui.agenda.AgendaEntry
import com.example.subroutine_simple.ui.components.ItemCardMenuAction
import com.example.subroutine_simple.ui.components.InlineActionCreator
import com.example.subroutine_simple.ui.components.InlineActionCreatorState
import com.example.subroutine_simple.ui.components.compoundItemCards
import com.example.subroutine_simple.data.models.Action
import com.example.subroutine_simple.data.models.Event
import com.example.subroutine_simple.data.models.Marker
import com.example.subroutine_simple.data.models.Signal
import com.example.subroutine_simple.ui.theme.SubroutineTheme
import com.example.subroutine_simple.ui.theme.subroutineButtonColors
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.YearMonth
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.util.Locale

private val MinimumCalendarWidth = 336.dp
private val MonthFormatter = DateTimeFormatter.ofPattern("MMMM yyyy")
private val SelectedDateFormatter = DateTimeFormatter.ofPattern("EEEE, MMMM d")


object CalendarTestTags {
    const val Root = "calendar"
    const val MonthTitle = "calendar:month-title"
    const val PreviousMonth = "calendar:previous-month"
    const val NextMonth = "calendar:next-month"
    const val Today = "calendar:today"
    const val WeekdayHeader = "calendar:weekdays"
    const val Grid = "calendar:grid"
    const val SelectedDayAgenda = "calendar:selected-day-agenda"
    const val AgendaEmpty = "calendar:agenda:empty"
    const val CreateOnDay = "calendar:create-on-day"
    const val ViewInTimeline = "calendar:view-in-timeline"

    fun day(date: LocalDate): String = "calendar:day:$date"
    fun workIndicator(date: LocalDate): String = "calendar:day:$date:work"
    fun markerIndicator(date: LocalDate): String = "calendar:day:$date:marker"
    fun signalIndicator(date: LocalDate): String = "calendar:day:$date:signal"
    fun overflow(date: LocalDate): String = "calendar:day:$date:overflow"
    fun agendaItem(key: String): String = "calendar:agenda:item:$key"
    fun editItem(key: String): String = "calendar:agenda:item:$key:edit"
}

@Composable
fun CalendarScreen(
    projection: CalendarProjection,
    onVisibleMonthChange: (YearMonth) -> Unit,
    onSelectedDateChange: (LocalDate) -> Unit,
    onCreateOnDay: (LocalDate) -> Unit,
    onViewInTimeline: (LocalDate) -> Unit,
    onEditAction: (Action) -> Unit,
    onEditEvent: (Event) -> Unit,
    onEditMarker: (Marker) -> Unit,
    onEditSignal: (Signal) -> Unit,
    modifier: Modifier = Modifier,
    contentPadding: PaddingValues = PaddingValues(),
    onCompleteAction: ((String) -> Unit)? = null,
    completing: Set<String> = emptySet(),
    menuActions: (AgendaEntry) -> List<ItemCardMenuAction> = { emptyList() },
    actionsEnabled: Boolean = true,
    creatorState: InlineActionCreatorState? = null,
    onCreateAction: ((String, () -> Unit) -> Unit)? = null,
    errorMessage: String? = null,
    status: (@Composable () -> Unit)? = null,
    onMoreOptions: (() -> Unit)? = null,
) {
    LazyColumn(
        modifier = modifier
            .fillMaxSize()
            .consumeWindowInsets(contentPadding)
            .imePadding()
            .testTag(CalendarTestTags.Root),
        contentPadding = contentPadding,
    ) {
        if (status != null) item(key = "calendar:status") { status() }
        item(key = "month-header") {
            CalendarMonthHeader(
                projection = projection,
                onVisibleMonthChange = onVisibleMonthChange,
                onSelectedDateChange = onSelectedDateChange,
            )
        }
        item(key = "month-grid") {
            ResponsiveCalendarGrid(
                days = projection.days,
                onDaySelected = { day ->
                    if (!day.isInVisibleMonth) onVisibleMonthChange(YearMonth.from(day.date))
                    onSelectedDateChange(day.date)
                },
            )
        }
        item(key = "agenda-header") {
            SelectedDayHeader(
                day = projection.selectedDay,
                onCreateOnDay = onCreateOnDay,
                onViewInTimeline = onViewInTimeline,
                actionsEnabled = actionsEnabled,
                showCreate = creatorState == null,
            )
        }
        if (projection.selectedDay.items.isEmpty()) {
            item(key = "agenda-empty") {
                Text(
                    text = "No items",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp, vertical = 24.dp)
                        .testTag(CalendarTestTags.AgendaEmpty),
                )
            }
        } else {
            compoundItemCards(
                entries = projection.selectedDay.items.map(CalendarItem::asAgendaEntry),
                keyPrefix = "calendar:agenda:${projection.selectedDate}",
                dateContext = projection.selectedDate,
                onEdit = { entry ->
                    when (entry) {
                        is AgendaEntry.ActionEntry -> onEditAction(entry.action)
                        is AgendaEntry.EventEntry -> onEditEvent(entry.event)
                        is AgendaEntry.MarkerEntry -> onEditMarker(entry.marker)
                        is AgendaEntry.SignalEntry -> onEditSignal(entry.signal)
                    }
                },
                onCompleteAction = onCompleteAction.takeIf { actionsEnabled },
                completing = completing,
                menuActions = { entry -> if (actionsEnabled) menuActions(entry) else emptyList() },
                itemModifier = Modifier.padding(horizontal = 16.dp),
            )
        }
        if (creatorState != null && onCreateAction != null) {
            item(key = "calendar:create:${projection.selectedDate}") {
                InlineActionCreator(
                    state = creatorState,
                    saving = !actionsEnabled,
                    onCreate = onCreateAction,
                    error = errorMessage,
                    onMoreOptions = onMoreOptions ?: { onCreateOnDay(projection.selectedDate) },
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }
        }
        item(key = "bottom-space") {
            Box(Modifier.size(16.dp))
        }
    }
}

@Composable
private fun CalendarMonthHeader(
    projection: CalendarProjection,
    onVisibleMonthChange: (YearMonth) -> Unit,
    onSelectedDateChange: (LocalDate) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(
            onClick = { onVisibleMonthChange(projection.visibleMonth.minusMonths(1)) },
            modifier = Modifier.testTag(CalendarTestTags.PreviousMonth),
        ) {
            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Previous month")
        }
        Text(
            text = projection.visibleMonth.format(MonthFormatter),
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.SemiBold,
            textAlign = TextAlign.Center,
            modifier = Modifier
                .weight(1f)
                .testTag(CalendarTestTags.MonthTitle),
        )
        TextButton(
            onClick = {
                if (projection.visibleMonth != YearMonth.from(projection.today)) {
                    onVisibleMonthChange(YearMonth.from(projection.today))
                }
                if (projection.selectedDate != projection.today) {
                    onSelectedDateChange(projection.today)
                }
            },
            enabled = projection.visibleMonth != YearMonth.from(projection.today) ||
                    projection.selectedDate != projection.today,
            modifier = Modifier
                .heightIn(min = 48.dp)
                .testTag(CalendarTestTags.Today),
        ) {
            Text("Today")
        }
        IconButton(
            onClick = { onVisibleMonthChange(projection.visibleMonth.plusMonths(1)) },
            modifier = Modifier.testTag(CalendarTestTags.NextMonth),
        ) {
            Icon(Icons.AutoMirrored.Filled.ArrowForward, contentDescription = "Next month")
        }
    }
}

@Composable
private fun ResponsiveCalendarGrid(
    days: List<CalendarDay>,
    onDaySelected: (CalendarDay) -> Unit,
) {
    BoxWithConstraints(modifier = Modifier.fillMaxWidth()) {
        val gridWidth = maxWidth.coerceAtLeast(MinimumCalendarWidth)
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState()),
        ) {
            CalendarGrid(
                days = days,
                onDaySelected = onDaySelected,
                modifier = Modifier
                    .width(gridWidth)
                    .testTag(CalendarTestTags.Grid),
            )
        }
    }
}

@Composable
private fun CalendarGrid(
    days: List<CalendarDay>,
    onDaySelected: (CalendarDay) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .testTag(CalendarTestTags.WeekdayHeader),
        ) {
            DayOfWeek.entries
                .sortedBy(DayOfWeek::getValue)
                .forEach { dayOfWeek ->
                    Text(
                        text = dayOfWeek.getDisplayName(TextStyle.NARROW, Locale.getDefault()),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                        modifier = Modifier
                            .weight(1f)
                            .padding(vertical = 8.dp),
                    )
                }
        }
        days.chunked(7).forEach { week ->
            Row(modifier = Modifier.fillMaxWidth()) {
                week.forEach { day ->
                    CalendarDayCell(day = day, onClick = { onDaySelected(day) })
                }
            }
        }
    }
}

@Composable
private fun RowScope.CalendarDayCell(
    day: CalendarDay,
    onClick: () -> Unit,
) {
    val colors = calendarDayColors(
        day = day,
        scheme = MaterialTheme.colorScheme,
        currentTime = SubroutineTheme.colors.currentTime,
        onCurrentTime = SubroutineTheme.colors.onCurrentTime,
    )
    val summaryDescription = day.summaryDescription()

    Surface(
        color = colors.container,
        contentColor = colors.content,
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier
            .weight(1f)
            .sizeIn(minWidth = 48.dp, minHeight = 56.dp)
            .testTag(CalendarTestTags.day(day.date))
            .semantics {
                contentDescription = buildString {
                    append(day.date)
                    if (day.isToday) append(", today")
                    if (summaryDescription.isNotEmpty()) append(", $summaryDescription")

                }
            }
            .selectable(
                selected = day.isSelected,
                onClick = onClick,
                role = Role.RadioButton,
            )
            .padding(2.dp),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(vertical = 5.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(
                text = day.date.dayOfMonth.toString(),
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = if (day.isSelected || day.isToday) FontWeight.Bold else FontWeight.Normal,
                color = colors.numberContent,
                modifier = Modifier.drawBehind {
                    val radius = size.maxDimension / 2f
                    drawCircle(color = colors.numberContainer, radius = radius)
                    val borderWidth = 1.dp.toPx()
                    drawCircle(
                        color = colors.numberBorder,
                        radius = radius + borderWidth / 2f,
                        style = Stroke(width = borderWidth),
                    )
                },
            )
            Row(
                horizontalArrangement = Arrangement.spacedBy(3.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (day.summary.hasWork) {
                    CalendarIndicator(
                        kind = CalendarIndicatorKind.Work,
                        modifier = Modifier.testTag(CalendarTestTags.workIndicator(day.date)),
                    )
                }
                if (day.summary.hasMarkers) {
                    CalendarIndicator(
                        kind = CalendarIndicatorKind.Marker,
                        modifier = Modifier.testTag(CalendarTestTags.markerIndicator(day.date)),
                    )
                }
                if (day.summary.hasSignals) {
                    CalendarIndicator(
                        kind = CalendarIndicatorKind.Signal,
                        modifier = Modifier.testTag(CalendarTestTags.signalIndicator(day.date)),
                    )
                }

            }
        }
    }
}

internal enum class CalendarIndicatorKind(val shape: Shape) {
    Work(RectangleShape),
    Marker(CircleShape),
    Signal(CutCornerShape(percent = 50)),
}

@Composable
private fun CalendarIndicator(
    kind: CalendarIndicatorKind,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .size(6.dp)
            .background(color = MaterialTheme.colorScheme.onSurfaceVariant, shape = kind.shape),
    )
}

@Composable
private fun SelectedDayHeader(
    day: CalendarDay,
    onCreateOnDay: (LocalDate) -> Unit,
    onViewInTimeline: (LocalDate) -> Unit,
    actionsEnabled: Boolean,
    showCreate: Boolean,
) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 12.dp)
            .testTag(CalendarTestTags.SelectedDayAgenda),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = day.date.format(SelectedDateFormatter),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (showCreate) Button(
                    onClick = { onCreateOnDay(day.date) },
                    enabled = actionsEnabled,
                    colors = subroutineButtonColors(),
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 48.dp)
                        .testTag(CalendarTestTags.CreateOnDay),
                ) {
                    Text("Add item", maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                OutlinedButton(
                    onClick = { onViewInTimeline(day.date) },
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 48.dp)
                        .testTag(CalendarTestTags.ViewInTimeline),
                ) {
                    Text("Timeline", maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

private fun CalendarItem.asAgendaEntry(): AgendaEntry = when (this) {
    is CalendarItem.ActionItem -> AgendaEntry.ActionEntry(action)
    is CalendarItem.EventItem -> AgendaEntry.EventEntry(event)
    is CalendarItem.MarkerItem -> AgendaEntry.MarkerEntry(marker)
    is CalendarItem.SignalItem -> AgendaEntry.SignalEntry(signal)
}
