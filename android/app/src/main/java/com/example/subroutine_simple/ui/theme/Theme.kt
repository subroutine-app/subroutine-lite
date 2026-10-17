package com.example.subroutine_simple.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.material3.ButtonColors
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CheckboxColors
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.unit.dp

// Material controls use teal selection; action buttons opt into primaryFill separately.
// Containers resolve interactive.selected or effect.semanticWashFaintAlpha over surface.panel.
internal val SubroutineDarkColorScheme = darkColorScheme(
    primary = SubroutineDarkColors.context,
    onPrimary = SubroutineDarkColors.onContext,
    primaryContainer = SubroutineDarkColors.selected.compositeOver(SubroutineDarkPanel),
    onPrimaryContainer = SubroutineDarkColors.contextStrong,
    inversePrimary = SubroutineLightColors.context,
    secondary = SubroutineDarkColors.context,
    onSecondary = SubroutineDarkColors.onContext,
    secondaryContainer = SubroutineDarkColors.selected.compositeOver(SubroutineDarkPanel),
    onSecondaryContainer = SubroutineDarkColors.contextStrong,
    tertiary = SubroutineDarkColors.actionText,
    onTertiary = SubroutineDarkColors.onPrimaryFill,
    tertiaryContainer = SubroutineDarkColors.action.copy(alpha = 0.09f).compositeOver(SubroutineDarkPanel),
    onTertiaryContainer = SubroutineDarkColors.actionText,
    background = SubroutineDarkCanvas,
    onBackground = SubroutineDarkText,
    surface = SubroutineDarkCanvas,
    onSurface = SubroutineDarkText,
    surfaceVariant = SubroutineDarkRaised,
    onSurfaceVariant = SubroutineDarkTextMuted,
    surfaceTint = Color.Transparent,
    inverseSurface = SubroutineDarkText,
    inverseOnSurface = SubroutineLightText,
    error = SubroutineDarkColors.critical,
    onError = SubroutineDarkColors.onCritical,
    errorContainer = SubroutineDarkColors.critical.copy(alpha = 0.09f).compositeOver(SubroutineDarkPanel),
    onErrorContainer = SubroutineDarkColors.critical,
    outline = SubroutineDarkOutline,
    outlineVariant = SubroutineDarkOutlineVariant,
    scrim = SubroutineDarkBackdrop,
    surfaceBright = SubroutineDarkOverlay,
    surfaceDim = SubroutineDarkSunken,
    surfaceContainerLowest = SubroutineDarkBackdrop,
    surfaceContainerLow = SubroutineDarkPanel,
    surfaceContainer = SubroutineDarkControl,
    surfaceContainerHigh = SubroutineDarkRaised,
    surfaceContainerHighest = SubroutineDarkOverlay,
)

internal val SubroutineLightColorScheme = lightColorScheme(
    primary = SubroutineLightColors.context,
    onPrimary = SubroutineLightColors.onContext,
    primaryContainer = SubroutineLightColors.selected.compositeOver(SubroutineLightPanel),
    onPrimaryContainer = SubroutineLightColors.contextStrong,
    inversePrimary = SubroutineDarkColors.context,
    secondary = SubroutineLightColors.context,
    onSecondary = SubroutineLightColors.onContext,
    secondaryContainer = SubroutineLightColors.selected.compositeOver(SubroutineLightPanel),
    onSecondaryContainer = SubroutineLightColors.contextStrong,
    tertiary = SubroutineLightColors.actionText,
    onTertiary = SubroutineLightColors.onPrimaryFill,
    tertiaryContainer = SubroutineLightColors.action.copy(alpha = 0.09f).compositeOver(SubroutineLightPanel),
    onTertiaryContainer = SubroutineLightColors.actionText,
    background = SubroutineLightCanvas,
    onBackground = SubroutineLightText,
    surface = SubroutineLightCanvas,
    onSurface = SubroutineLightText,
    surfaceVariant = SubroutineLightBackdrop,
    onSurfaceVariant = SubroutineLightTextMuted,
    surfaceTint = Color.Transparent,
    inverseSurface = SubroutineLightText,
    inverseOnSurface = SubroutineLightCanvas,
    error = SubroutineLightColors.critical,
    onError = SubroutineLightColors.onCritical,
    errorContainer = SubroutineLightColors.critical.copy(alpha = 0.09f).compositeOver(SubroutineLightPanel),
    onErrorContainer = SubroutineLightColors.critical,
    outline = SubroutineLightOutline,
    outlineVariant = SubroutineLightOutlineVariant,
    scrim = SubroutineBrand.Charcoal,
    surfaceBright = SubroutineLightRaised,
    surfaceDim = SubroutineLightBackdrop,
    surfaceContainerLowest = SubroutineLightRaised,
    surfaceContainerLow = SubroutineLightPanel,
    surfaceContainer = SubroutineLightControl,
    surfaceContainerHigh = SubroutineLightBackdrop,
    surfaceContainerHighest = SubroutineLightControlPressed,
)

private val LocalSubroutineColors = staticCompositionLocalOf { SubroutineLightColors }

object SubroutineTheme {
    val colors: SubroutineColors
        @Composable
        @ReadOnlyComposable
        get() = LocalSubroutineColors.current
}

@Composable
fun subroutineButtonColors(): ButtonColors = ButtonDefaults.buttonColors(
    containerColor = SubroutineTheme.colors.primaryFill,
    contentColor = SubroutineTheme.colors.onPrimaryFill,
)

@Composable
fun subroutineTonalButtonColors(): ButtonColors = ButtonDefaults.filledTonalButtonColors(
    containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
    contentColor = MaterialTheme.colorScheme.onSurface,
)

@Composable
fun subroutineCompletionCheckboxColors(): CheckboxColors = CheckboxDefaults.colors(
    checkedColor = SubroutineTheme.colors.stable,
    checkmarkColor = SubroutineTheme.colors.onStable,
    uncheckedColor = MaterialTheme.colorScheme.outline,
)

private val SubroutineShapes = Shapes(
    extraSmall = RoundedCornerShape(5.dp),
    small = RoundedCornerShape(8.dp),
    medium = RoundedCornerShape(12.dp),
    large = RoundedCornerShape(16.dp),
    extraLarge = RoundedCornerShape(19.dp),
)

@Composable
fun SubroutineSimpleTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val scheme = if (darkTheme) SubroutineDarkColorScheme else SubroutineLightColorScheme
    CompositionLocalProvider(
        LocalSubroutineColors provides if (darkTheme) SubroutineDarkColors else SubroutineLightColors,
    ) {
        MaterialTheme(
            colorScheme = scheme,
            motionScheme = MotionScheme.expressive(),
            typography = SubroutineTypography,
            shapes = SubroutineShapes,
        ) {
            CompositionLocalProvider(
                LocalTextSelectionColors provides TextSelectionColors(
                    handleColor = scheme.primary,
                    backgroundColor = SubroutineTheme.colors.selected,
                ),
                content = content,
            )
        }
    }
}
