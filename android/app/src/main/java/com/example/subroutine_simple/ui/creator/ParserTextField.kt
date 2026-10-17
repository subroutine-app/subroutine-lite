package com.example.subroutine_simple.ui.creator

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import com.example.subroutine_simple.ui.theme.SubroutineTheme

class ParserHighlightTransformation(
    private val highlights: List<ParserHighlight>,
    private val foreground: Color,
) : VisualTransformation {
    override fun filter(text: AnnotatedString): TransformedText {
        val annotated = AnnotatedString.Builder(text).apply {
            highlights.filter { it.kind != ParserHighlightKind.Title && it.isValidFor(text.text) }.forEach {
                addStyle(SpanStyle(color = foreground), it.start, it.end)
            }
        }.toAnnotatedString()
        return TransformedText(annotated, OffsetMapping.Identity)
    }
}

// Merge visual runs, not a whole logical range: bidirectional text can leave gaps on one line.
internal fun mergeHighlightGlyphs(glyphs: List<Rect>): List<Rect> = buildList {
    glyphs.filter { it.width > 0f && it.height > 0f }.sortedBy { it.left }.forEach { glyph ->
        val previous = lastOrNull()
        if (previous != null && glyph.left <= previous.right + 0.5f) {
            this[lastIndex] = Rect(
                previous.left, minOf(previous.top, glyph.top), maxOf(previous.right, glyph.right),
                maxOf(previous.bottom, glyph.bottom),
            )
        } else add(glyph)
    }
}

@Composable
fun ParserTextField(
    value: TextFieldValue,
    onValueChange: (TextFieldValue) -> Unit,
    highlights: List<ParserHighlight>,
    focusRequester: FocusRequester,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    placeholder: String = "What do you want to do?",
    label: String = "Item title",
    textStyle: TextStyle = MaterialTheme.typography.titleLarge,
    imeAction: ImeAction = ImeAction.Done,
    onSubmit: () -> Unit = {},
) {
    val colors = SubroutineTheme.colors
    var layout by remember { mutableStateOf<TextLayoutResult?>(null) }
    val recognition = remember(highlights, value.text) {
        highlights.filter { it.kind != ParserHighlightKind.Title && it.isValidFor(value.text) }
    }
    val transformation = remember(recognition, colors.contextStrong) {
        ParserHighlightTransformation(recognition, colors.contextStrong)
    }
    BasicTextField(
        value = value,
        onValueChange = onValueChange,
        enabled = enabled,
        textStyle = textStyle.copy(color = MaterialTheme.colorScheme.onSurface),
        cursorBrush = SolidColor(colors.contextStrong),
        visualTransformation = transformation,
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = imeAction),
        keyboardActions = KeyboardActions(onDone = { onSubmit() }),
        onTextLayout = { layout = it },
        modifier = modifier
            .fillMaxWidth()
            .focusRequester(focusRequester)
            .semantics { contentDescription = label }
            .padding(vertical = 8.dp)
            .heightIn(min = 48.dp)
            .clipToBounds()
            .drawWithCache {
                val measured = layout?.takeIf { it.layoutInput.text.text == value.text }
                val rectangles = measured?.let { result ->
                    recognition.flatMap { highlight ->
                        (highlight.start until highlight.end).groupBy(result::getLineForOffset).values.flatMap { offsets ->
                            mergeHighlightGlyphs(offsets.map(result::getBoundingBox))
                        }
                    }
                }.orEmpty()
                val radius = CornerRadius(6.dp.toPx())
                val inset = 1.dp.toPx()
                val horizontalPadding = 2.dp.toPx()
                val stroke = Stroke(1.dp.toPx())
                onDrawBehind {
                    rectangles.forEach { bounds ->
                        val left = (bounds.left - horizontalPadding).coerceAtLeast(inset)
                        val top = (bounds.top + inset).coerceAtLeast(inset)
                        val right = (bounds.right + horizontalPadding).coerceAtMost(size.width - inset)
                        val bottom = bounds.bottom.coerceAtMost(size.height - inset)
                        if (right > left && bottom > top) {
                            val origin = Offset(left, top)
                            val extent = Size(right - left, bottom - top)
                            drawRoundRect(colors.recognitionFill, origin, extent, radius)
                            drawRoundRect(colors.recognitionBorder, origin, extent, radius, style = stroke)
                        }
                    }
                }
            },
        decorationBox = { inner ->
            Box {
                if (value.text.isEmpty()) {
                    Text(placeholder, style = textStyle, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                inner()
            }
        },
    )
}
