package fr.skimap.app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * High-contrast schemes for snow and full sun (K12, #115). No dynamic colour: wallpaper
 * colours could lower the contrast. Every text / background pair used by the app reaches
 * 7:1 (WCAG AAA), checked by `ThemeContrastTest`.
 */
val SkiLightColors: ColorScheme =
    lightColorScheme(
        primary = Color(0xFF0B3A8C),
        onPrimary = Color(0xFFFFFFFF),
        primaryContainer = Color(0xFFDCE6FF),
        onPrimaryContainer = Color(0xFF001233),
        secondary = Color(0xFF2B3A55),
        onSecondary = Color(0xFFFFFFFF),
        secondaryContainer = Color(0xFFE3E8F2),
        onSecondaryContainer = Color(0xFF0A1424),
        tertiary = Color(0xFF6A2B00),
        onTertiary = Color(0xFFFFFFFF),
        error = Color(0xFF9A0000),
        onError = Color(0xFFFFFFFF),
        errorContainer = Color(0xFFFFE1DC),
        onErrorContainer = Color(0xFF3A0000),
        background = Color(0xFFFFFFFF),
        onBackground = Color(0xFF0A0A0A),
        surface = Color(0xFFFFFFFF),
        onSurface = Color(0xFF0A0A0A),
        surfaceVariant = Color(0xFFE9ECF1),
        onSurfaceVariant = Color(0xFF1E232B),
        surfaceContainerLowest = Color(0xFFFFFFFF),
        surfaceContainerLow = Color(0xFFFAFBFC),
        surfaceContainer = Color(0xFFF4F6F9),
        surfaceContainerHigh = Color(0xFFEEF1F5),
        surfaceContainerHighest = Color(0xFFE6EAF0),
        outline = Color(0xFF1E232B),
        outlineVariant = Color(0xFF5A6270),
        inverseSurface = Color(0xFF15181D),
        inverseOnSurface = Color(0xFFFFFFFF),
        inversePrimary = Color(0xFFB3CBFF),
        scrim = Color(0xFF000000),
    )

val SkiDarkColors: ColorScheme =
    darkColorScheme(
        primary = Color(0xFFB3CBFF),
        onPrimary = Color(0xFF001233),
        primaryContainer = Color(0xFF16336E),
        onPrimaryContainer = Color(0xFFFFFFFF),
        secondary = Color(0xFFC9D3E6),
        onSecondary = Color(0xFF0A1424),
        secondaryContainer = Color(0xFF26324A),
        onSecondaryContainer = Color(0xFFFFFFFF),
        tertiary = Color(0xFFFFC9A6),
        onTertiary = Color(0xFF2E1200),
        error = Color(0xFFFFB4A8),
        onError = Color(0xFF3A0000),
        errorContainer = Color(0xFF6E0E05),
        onErrorContainer = Color(0xFFFFFFFF),
        background = Color(0xFF0B0D11),
        onBackground = Color(0xFFFFFFFF),
        surface = Color(0xFF0B0D11),
        onSurface = Color(0xFFFFFFFF),
        surfaceVariant = Color(0xFF232833),
        onSurfaceVariant = Color(0xFFE6EAF2),
        surfaceContainerLowest = Color(0xFF07080B),
        surfaceContainerLow = Color(0xFF111419),
        surfaceContainer = Color(0xFF161A21),
        surfaceContainerHigh = Color(0xFF1C2129),
        surfaceContainerHighest = Color(0xFF232833),
        outline = Color(0xFFE6EAF2),
        outlineVariant = Color(0xFFA3ABBA),
        inverseSurface = Color(0xFFF4F6F9),
        inverseOnSurface = Color(0xFF0A0A0A),
        inversePrimary = Color(0xFF0B3A8C),
        scrim = Color(0xFF000000),
    )

/** Large type: read at arm's length, in motion, through sunglasses. */
val SkiTypography: Typography =
    Typography().run {
        Typography(
            headlineSmall = headlineSmall.copy(fontSize = 28.sp, lineHeight = 34.sp, fontWeight = FontWeight.Bold),
            titleLarge = titleLarge.copy(fontSize = 26.sp, lineHeight = 32.sp, fontWeight = FontWeight.Bold),
            titleMedium = titleMedium.copy(fontSize = 21.sp, lineHeight = 27.sp, fontWeight = FontWeight.Bold),
            bodyLarge = bodyLarge.copy(fontSize = 20.sp, lineHeight = 27.sp, fontWeight = FontWeight.Medium),
            bodyMedium = bodyMedium.copy(fontSize = 18.sp, lineHeight = 24.sp, fontWeight = FontWeight.Medium),
            bodySmall = bodySmall.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium),
            labelLarge = labelLarge.copy(fontSize = 20.sp, lineHeight = 24.sp, fontWeight = FontWeight.Bold),
            labelMedium = labelMedium.copy(fontSize = 16.sp, lineHeight = 20.sp, fontWeight = FontWeight.Bold),
        )
    }

/** Monospace style of the adb commands shown when no pack is installed. */
val CommandTextStyle: TextStyle =
    TextStyle(fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace, fontSize = 16.sp, lineHeight = 22.sp)

/** Minimum size of every touch target: usable with ski gloves (K12, #115). */
val GloveTarget = 72.dp

@Composable
fun SkimapTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (darkTheme) SkiDarkColors else SkiLightColors,
        typography = SkiTypography,
        content = content,
    )
}
