package ws.freeflight.ui

import java.util.Locale
import kotlin.math.abs

/** Sizes in the units a download dialog is read in, not exact byte counts. */
fun formatBytes(bytes: Long): String = when {
    bytes < 0 -> "—"
    bytes < 1_000 -> "$bytes B"
    bytes < 1_000_000 -> String.format(Locale.US, "%.0f kB", bytes / 1_000.0)
    bytes < 1_000_000_000 -> String.format(Locale.US, "%.1f MB", bytes / 1_000_000.0)
    else -> String.format(Locale.US, "%.2f GB", bytes / 1_000_000_000.0)
}

/**
 * How long ago something was fetched, phrased so that staleness is
 * unmissable — DESIGN.md §11 requires the age of a briefing to be explicit,
 * so this never rounds an hour-old observation down to "just now".
 */
fun formatAge(fetchedAtMillis: Long, nowMillis: Long = System.currentTimeMillis()): String {
    val seconds = abs(nowMillis - fetchedAtMillis) / 1000
    return when {
        seconds < 60 -> "just now"
        seconds < 3600 -> "${seconds / 60} min ago"
        seconds < 86_400 -> {
            val hours = seconds / 3600
            "$hours ${if (hours == 1L) "hour" else "hours"} ago"
        }
        else -> {
            val days = seconds / 86_400
            "$days ${if (days == 1L) "day" else "days"} ago"
        }
    }
}

/** A COM frequency as a pilot would set it: three decimal places, always. */
fun formatFrequency(megahertz: Double): String = String.format(Locale.US, "%.3f", megahertz)

/** Runway dimensions, e.g. "11,901 × 150 ft". */
fun formatRunwayDimensions(lengthFt: Long, widthFt: Long): String =
    "${String.format(Locale.US, "%,d", lengthFt)} × ${String.format(Locale.US, "%,d", widthFt)} ft"
