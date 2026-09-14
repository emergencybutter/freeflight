package ws.freeflight.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.sp

/**
 * A deliberately dark, low-chroma shell.
 *
 * The chart is the only thing on screen that should be bright: an FAA
 * sectional is a dense, high-contrast, colour-coded document, and UI
 * chrome that competes with it makes it harder to read. Everything here is
 * chosen to sit behind and around that, not next to it.
 */
private val DarkColors = darkColorScheme(
    primary = Color(0xFF4FC3F7),
    onPrimary = Color(0xFF04202B),
    primaryContainer = Color(0xFF0B3A4D),
    onPrimaryContainer = Color(0xFFCFEDFB),
    secondary = Color(0xFFFFB300),
    onSecondary = Color(0xFF2A1D00),
    background = Color(0xFF0E1116),
    onBackground = Color(0xFFE3E7EB),
    surface = Color(0xFF161B22),
    onSurface = Color(0xFFE3E7EB),
    surfaceVariant = Color(0xFF1F262F),
    onSurfaceVariant = Color(0xFFAAB4BF),
    outline = Color(0xFF3A444F),
    error = Color(0xFFFF6B6B),
    onError = Color(0xFF2B0000),
)

/**
 * Monospace for anything a pilot reads character by character — raw METAR
 * and TAF text, frequencies, leg tables. Proportional digits blur the
 * difference between `1` and `7` at a glance in turbulence.
 */
val MonoTextStyle = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp)

/**
 * Always dark, regardless of the system setting.
 *
 * Not an oversight and not a preference: the chart surround is a fixed
 * dark colour baked into the map style, and every screen in this app is
 * either the map or something drawn over it. A light shell around a dark
 * chart reads as a rendering bug, and a light chart surround would put a
 * white field next to a sectional a pilot is trying to read at night.
 */
@Composable
fun FreeflightTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = DarkColors,
        typography = Typography(),
        content = content,
    )
}
