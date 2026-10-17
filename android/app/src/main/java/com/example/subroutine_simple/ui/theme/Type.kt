package com.example.subroutine_simple.ui.theme

import androidx.compose.material3.Typography
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

private val MaterialType = Typography()

val SubroutineTypography = Typography(
    displayLarge = MaterialType.displayLarge.copy(
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.Bold,
        letterSpacing = (-1.5).sp,
    ),
    displayMedium = MaterialType.displayMedium.copy(
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.Bold,
        letterSpacing = (-1).sp,
    ),
    displaySmall = MaterialType.displaySmall.copy(
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.Bold,
        letterSpacing = (-0.5).sp,
    ),
    headlineLarge = MaterialType.headlineLarge.copy(
        fontWeight = FontWeight.Bold,
        letterSpacing = (-0.4).sp,
    ),
    headlineMedium = MaterialType.headlineMedium.copy(
        fontWeight = FontWeight.Bold,
        letterSpacing = (-0.25).sp,
    ),
    headlineSmall = MaterialType.headlineSmall.copy(fontWeight = FontWeight.SemiBold),
    titleLarge = MaterialType.titleLarge.copy(fontWeight = FontWeight.SemiBold),
    titleMedium = MaterialType.titleMedium.copy(fontWeight = FontWeight.SemiBold),
    titleSmall = MaterialType.titleSmall.copy(fontWeight = FontWeight.SemiBold),
    bodyLarge = MaterialType.bodyLarge.copy(letterSpacing = 0.15.sp),
    bodyMedium = MaterialType.bodyMedium.copy(letterSpacing = 0.15.sp),
    labelLarge = MaterialType.labelLarge.copy(fontWeight = FontWeight.SemiBold),
    labelMedium = MaterialType.labelMedium.copy(fontWeight = FontWeight.SemiBold),
)
