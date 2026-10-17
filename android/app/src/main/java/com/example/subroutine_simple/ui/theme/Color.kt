package com.example.subroutine_simple.ui.theme

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color

// Source: assets/themes/subroutine-{light,dark}.json and assets/themes/README.md.
object SubroutineBrand {
    val Brick = Color(0xFFB0413E)
    val Charcoal = Color(0xFF2E282A)
    val Cream = Color(0xFFFAF0CA)
    val Teal = Color(0xFF548687)
    val Tangerine = Color(0xFFEE964B)
    val Gold = Color(0xFFF4D35E)
    val Sage = Color(0xFF7D9463)
    val Oxblood = Color(0xFFA4031F)
    val Aubergine = Color(0xFF240B36)
}

@Immutable
data class SubroutineColors(
    val brick: Color,
    val primaryFill: Color,
    val onPrimaryFill: Color,
    val context: Color,
    val contextStrong: Color,
    val onContext: Color,
    val selected: Color,
    val action: Color,
    val actionText: Color,
    val attention: Color,
    val stable: Color,
    val onStable: Color,
    val critical: Color,
    val onCritical: Color,
    val currentTime: Color,
    val currentTimeText: Color,
    val onCurrentTime: Color,
    val recognitionFill: Color,
    val recognitionBorder: Color,
)

internal val SubroutineLightColors = SubroutineColors(
    brick = SubroutineBrand.Brick,
    primaryFill = Color(0xFFB35A24),
    onPrimaryFill = Color(0xFFFFFFFC),
    context = Color(0xFF3F6566),
    contextStrong = Color(0xFF2E5253),
    onContext = Color(0xFFFFFFFC),
    selected = Color(0x203F6566),
    action = Color(0xFF925B2D),
    actionText = Color(0xFF855026),
    attention = Color(0xFF9A8122),
    stable = Color(0xFF536342),
    onStable = Color(0xFFFFFFFC),
    critical = SubroutineBrand.Oxblood,
    onCritical = Color(0xFFFFFFFC),
    currentTime = Color(0xFFD12C3D),
    currentTimeText = Color(0xFFB21C30),
    onCurrentTime = Color(0xFFFFFFFC),
    recognitionFill = Color(0x473F6566),
    recognitionBorder = Color(0x993F6566),
)

internal val SubroutineDarkColors = SubroutineColors(
    brick = Color(0xFFD68A85),
    primaryFill = SubroutineBrand.Tangerine,
    onPrimaryFill = SubroutineBrand.Charcoal,
    context = Color(0xFF88ABAC),
    contextStrong = Color(0xFFAAC9C7),
    onContext = Color(0xFF181719),
    selected = Color(0x2E88ABAC),
    action = SubroutineBrand.Tangerine,
    actionText = SubroutineBrand.Tangerine,
    attention = SubroutineBrand.Gold,
    stable = Color(0xFFA4B88E),
    onStable = SubroutineBrand.Charcoal,
    critical = Color(0xFFE2A0AE),
    onCritical = Color(0xFF181719),
    currentTime = Color(0xFFF04B58),
    currentTimeText = Color(0xFFFF96A0),
    onCurrentTime = Color(0xFF181719),
    recognitionFill = Color(0x4788ABAC),
    recognitionBorder = Color(0x9988ABAC),
)

internal val SubroutineLightBackdrop = Color(0xFFEEECE6)
internal val SubroutineLightCanvas = Color(0xFFF7F6F2)
internal val SubroutineLightPanel = Color(0xFFFCFBF7)
internal val SubroutineLightRaised = Color(0xFFFFFEFA)
internal val SubroutineLightControl = Color(0xFFFAF9F5)
internal val SubroutineLightControlPressed = Color(0xFFE9E6DF)
internal val SubroutineLightText = SubroutineBrand.Charcoal
internal val SubroutineLightTextMuted = Color(0xFF514E4F)
internal val SubroutineLightOutline = Color(0xFF77716E)
internal val SubroutineLightOutlineVariant = Color(0xFFD5D2CF)

internal val SubroutineDarkBackdrop = Color(0xFF100F11)
internal val SubroutineDarkCanvas = Color(0xFF181719)
internal val SubroutineDarkSunken = Color(0xFF131214)
internal val SubroutineDarkPanel = Color(0xFF211F21)
internal val SubroutineDarkControl = Color(0xFF262326)
internal val SubroutineDarkRaised = Color(0xFF292629)
internal val SubroutineDarkOverlay = Color(0xFF2B282B)
internal val SubroutineDarkText = Color(0xFFDEDCDA)
internal val SubroutineDarkTextMuted = Color(0xFFBBB8B7)
internal val SubroutineDarkOutline = Color(0xFF968D89)
internal val SubroutineDarkOutlineVariant = Color(0xFF343034)
