package com.example.subroutine_simple.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.data.models.ActionTemplate
import com.example.subroutine_simple.data.models.EventTemplate
import com.example.subroutine_simple.ui.components.durationIso
import com.example.subroutine_simple.ui.components.durationMinutes
import com.example.subroutine_simple.ui.components.formatDuration
import com.example.subroutine_simple.ui.theme.subroutineButtonColors

private val PreferredTimePattern = Regex("^(?:[01]\\d|2[0-3]):[0-5]\\d(?::[0-5]\\d)?$")

@Composable
fun ActionTemplateEditor(
    template: ActionTemplate,
    saving: Boolean,
    onBack: () -> Unit,
    onSave: (title: String, content: String?, naiveTime: String?, duration: String?) -> Unit,
    onDelete: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var title by rememberSaveable(template.id, "action-template-title") {
        mutableStateOf(template.title)
    }
    var content by rememberSaveable(template.id, "action-template-content") {
        mutableStateOf(template.content.orEmpty())
    }
    var preferredTime by rememberSaveable(template.id, "action-template-time") {
        mutableStateOf(template.naiveTime.orEmpty())
    }
    val initialDurationMinutes = durationMinutes(template.duration)
    var selectedDurationMinutes by rememberSaveable(template.id, "action-template-duration") {
        mutableStateOf(initialDurationMinutes)
    }

    val trimmedTime = preferredTime.trim()
    val timeIsValid = trimmedTime.isEmpty() || PreferredTimePattern.matches(trimmedTime)
    val savedTitle = if (title == template.title) template.title else title.trim()
    val savedContent = if (content == template.content.orEmpty()) {
        template.content
    } else {
        content.trim().ifEmpty { null }
    }
    val savedTime = if (preferredTime == template.naiveTime.orEmpty()) {
        template.naiveTime
    } else {
        trimmedTime.ifEmpty { null }
    }
    val durationChanged = selectedDurationMinutes != initialDurationMinutes
    val savedDuration = if (durationChanged) {
        durationIso(selectedDurationMinutes)
    } else {
        template.duration
    }
    val dirty = savedTitle != template.title ||
        savedContent != template.content ||
        savedTime != template.naiveTime ||
        durationChanged
    val valid = savedTitle.isNotBlank() && timeIsValid

    TemplateEditorScaffold(
        editorTitle = "Edit action template",
        itemTitle = template.title,
        itemKind = "action template",
        tagPrefix = "action-template-editor",
        saving = saving,
        dirty = dirty,
        valid = valid,
        onBack = onBack,
        onSave = {
            onSave(savedTitle, savedContent, savedTime, savedDuration)
        },
        onDelete = onDelete,
        modifier = modifier,
    ) {
        TemplateTextFields(
            title = title,
            onTitleChange = { title = it },
            content = content,
            onContentChange = { content = it },
            enabled = !saving,
            tagPrefix = "action-template-editor",
        )
        OutlinedTextField(
            value = preferredTime,
            onValueChange = { preferredTime = it },
            label = { Text("Preferred time (optional)") },
            placeholder = { Text("HH:mm or HH:mm:ss") },
            supportingText = if (timeIsValid) null else {
                { Text("Enter a valid time as HH:mm or HH:mm:ss") }
            },
            trailingIcon = if (preferredTime.isNotEmpty()) {
                {
                    IconButton(
                        onClick = { preferredTime = "" },
                        enabled = !saving,
                        modifier = Modifier.testTag("action-template-editor:preferred-time:clear"),
                    ) {
                        Icon(Icons.Default.Close, contentDescription = "Clear preferred time")
                    }
                }
            } else {
                null
            },
            isError = !timeIsValid,
            enabled = !saving,
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Ascii,
                imeAction = ImeAction.Next,
            ),
            shape = MaterialTheme.shapes.large,
            modifier = Modifier
                .fillMaxWidth()
                .testTag("action-template-editor:preferred-time"),
        )
        TemplateDurationEditor(
            minutes = selectedDurationMinutes,
            required = false,
            enabled = !saving,
            onChange = { selectedDurationMinutes = it },
            tagPrefix = "action-template-editor",
        )
    }
}

@Composable
fun EventTemplateEditor(
    template: EventTemplate,
    saving: Boolean,
    onBack: () -> Unit,
    onSave: (title: String, content: String?, duration: String) -> Unit,
    onDelete: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var title by rememberSaveable(template.id, "event-template-title") {
        mutableStateOf(template.title)
    }
    var content by rememberSaveable(template.id, "event-template-content") {
        mutableStateOf(template.content.orEmpty())
    }
    val initialDurationMinutes = durationMinutes(template.duration)
    var selectedDurationMinutes by rememberSaveable(template.id, "event-template-duration") {
        mutableLongStateOf(initialDurationMinutes ?: 60L)
    }

    val savedTitle = if (title == template.title) template.title else title.trim()
    val savedContent = if (content == template.content.orEmpty()) {
        template.content
    } else {
        content.trim().ifEmpty { null }
    }
    val durationChanged = selectedDurationMinutes != initialDurationMinutes
    val savedDuration = if (durationChanged) {
        requireNotNull(durationIso(selectedDurationMinutes))
    } else {
        template.duration
    }
    val dirty = savedTitle != template.title ||
        savedContent != template.content ||
        durationChanged
    val valid = savedTitle.isNotBlank() && selectedDurationMinutes > 0L

    TemplateEditorScaffold(
        editorTitle = "Edit event template",
        itemTitle = template.title,
        itemKind = "event template",
        tagPrefix = "event-template-editor",
        saving = saving,
        dirty = dirty,
        valid = valid,
        onBack = onBack,
        onSave = { onSave(savedTitle, savedContent, savedDuration) },
        onDelete = onDelete,
        modifier = modifier,
    ) {
        TemplateTextFields(
            title = title,
            onTitleChange = { title = it },
            content = content,
            onContentChange = { content = it },
            enabled = !saving,
            tagPrefix = "event-template-editor",
        )
        TemplateDurationEditor(
            minutes = selectedDurationMinutes,
            required = true,
            enabled = !saving,
            onChange = { next ->
                if (next != null) selectedDurationMinutes = next
            },
            tagPrefix = "event-template-editor",
        )
    }
}

@Composable
private fun TemplateEditorScaffold(
    editorTitle: String,
    itemTitle: String,
    itemKind: String,
    tagPrefix: String,
    saving: Boolean,
    dirty: Boolean,
    valid: Boolean,
    onBack: () -> Unit,
    onSave: () -> Unit,
    onDelete: () -> Unit,
    modifier: Modifier,
    fields: @Composable ColumnScope.() -> Unit,
) {
    var showDiscardDialog by rememberSaveable { mutableStateOf(false) }
    var showDeleteDialog by rememberSaveable { mutableStateOf(false) }

    fun requestBack() {
        when {
            saving -> Unit
            dirty -> showDiscardDialog = true
            else -> onBack()
        }
    }

    BackHandler(onBack = ::requestBack)
    val scrollBehavior = TopAppBarDefaults.pinnedScrollBehavior()

    Scaffold(
        modifier = modifier
            .fillMaxSize()
            .nestedScroll(scrollBehavior.nestedScrollConnection)
            .testTag(tagPrefix),
        topBar = {
            TopAppBar(
                title = { Text(editorTitle) },
                navigationIcon = {
                    IconButton(
                        onClick = ::requestBack,
                        enabled = !saving,
                        modifier = Modifier.testTag("$tagPrefix:back"),
                    ) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    IconButton(
                        onClick = { showDeleteDialog = true },
                        enabled = !saving,
                        colors = IconButtonDefaults.iconButtonColors(contentColor = MaterialTheme.colorScheme.error),
                        modifier = Modifier.testTag("$tagPrefix:delete"),
                    ) {
                        Icon(
                            Icons.Default.Delete,
                            contentDescription = "Delete $itemKind",
                        )
                    }
                },
                scrollBehavior = scrollBehavior,
            )
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(innerPadding)
                .padding(horizontal = 24.dp, vertical = 16.dp)
                .imePadding(),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            fields()
            HorizontalDivider()
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TextButton(
                    onClick = ::requestBack,
                    enabled = !saving,
                    modifier = Modifier.testTag("$tagPrefix:discard"),
                ) {
                    Text("Discard")
                }
                if (saving) {
                    CircularProgressIndicator(
                        modifier = Modifier
                            .size(32.dp)
                            .testTag("$tagPrefix:saving"),
                    )
                } else {
                    Button(
                        onClick = onSave,
                        enabled = valid && dirty,
                        colors = subroutineButtonColors(),
                        shapes = ButtonDefaults.shapes(),
                        modifier = Modifier.testTag("$tagPrefix:save"),
                    ) {
                        Text("Save")
                    }
                }
            }
        }
    }

    if (showDiscardDialog) {
        AlertDialog(
            onDismissRequest = { showDiscardDialog = false },
            title = { Text("Discard changes?") },
            text = { Text("Your unsaved $itemKind changes will be lost.") },
            confirmButton = {
                TextButton(
                    onClick = onBack,
                    modifier = Modifier.testTag("$tagPrefix:discard:confirm"),
                ) {
                    Text("Discard")
                }
            },
            dismissButton = {
                TextButton(
                    onClick = { showDiscardDialog = false },
                    modifier = Modifier.testTag("$tagPrefix:discard:cancel"),
                ) {
                    Text("Keep editing")
                }
            },
            modifier = Modifier.testTag("$tagPrefix:discard:dialog"),
        )
    }

    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("Delete $itemKind?") },
            text = { Text("\"$itemTitle\" will be permanently removed.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteDialog = false
                        onDelete()
                    },
                    modifier = Modifier.testTag("$tagPrefix:delete:confirm"),
                ) {
                    Text("Delete", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(
                    onClick = { showDeleteDialog = false },
                    modifier = Modifier.testTag("$tagPrefix:delete:cancel"),
                ) {
                    Text("Cancel")
                }
            },
            modifier = Modifier.testTag("$tagPrefix:delete:dialog"),
        )
    }
}

@Composable
private fun TemplateTextFields(
    title: String,
    onTitleChange: (String) -> Unit,
    content: String,
    onContentChange: (String) -> Unit,
    enabled: Boolean,
    tagPrefix: String,
) {
    OutlinedTextField(
        value = title,
        onValueChange = onTitleChange,
        label = { Text("Title") },
        enabled = enabled,
        singleLine = true,
        keyboardOptions = KeyboardOptions(
            capitalization = KeyboardCapitalization.Sentences,
            imeAction = ImeAction.Next,
        ),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier
            .fillMaxWidth()
            .testTag("$tagPrefix:title"),
    )
    OutlinedTextField(
        value = content,
        onValueChange = onContentChange,
        label = { Text("Notes (optional)") },
        enabled = enabled,
        minLines = 4,
        maxLines = 8,
        keyboardOptions = KeyboardOptions(
            capitalization = KeyboardCapitalization.Sentences,
            imeAction = ImeAction.Default,
        ),
        shape = MaterialTheme.shapes.large,
        modifier = Modifier
            .fillMaxWidth()
            .testTag("$tagPrefix:notes"),
    )
}

@Composable
private fun TemplateDurationEditor(
    minutes: Long?,
    required: Boolean,
    enabled: Boolean,
    onChange: (Long?) -> Unit,
    tagPrefix: String,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .testTag("$tagPrefix:duration"),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text("Duration", style = MaterialTheme.typography.labelLarge)
            Text(
                text = minutes?.let(::formatDuration) ?: if (required) {
                    "Choose a duration"
                } else {
                    "No fixed duration"
                },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        IconButton(
            onClick = {
                val next = (minutes ?: 0L) - 15L
                onChange(next.takeIf { it > 0L })
            },
            enabled = enabled && minutes != null && (!required || minutes > 15L),
            modifier = Modifier.testTag("$tagPrefix:duration:decrease"),
        ) {
            Icon(Icons.Default.Remove, contentDescription = "Decrease duration by 15 minutes")
        }
        IconButton(
            onClick = { onChange((minutes ?: 0L) + 15L) },
            enabled = enabled,
            modifier = Modifier.testTag("$tagPrefix:duration:increase"),
        ) {
            Icon(Icons.Default.Add, contentDescription = "Increase duration by 15 minutes")
        }
    }
}
