package ws.freeflight.ui

import android.content.Context
import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.FileDownload
import androidx.compose.material.icons.filled.FlightLand
import androidx.compose.material.icons.filled.FlightTakeoff
import androidx.compose.material.icons.filled.Map
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material.icons.filled.Timer
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ws.freeflight.data.RecordedFlight

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FlightReviewSheet(
    flight: RecordedFlight,
    onDismiss: () -> Unit,
    onShowOnMap: (RecordedFlight) -> Unit,
    onExportGpx: (RecordedFlight) -> Unit,
    onExportCsv: (RecordedFlight) -> Unit,
    onDelete: (RecordedFlight) -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 20.dp, vertical = 8.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column {
                    Text(
                        flight.name,
                        style = MaterialTheme.typography.titleLarge,
                        fontWeight = FontWeight.Bold,
                    )
                    Text(
                        "${flight.points.size} track points logged",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                IconButton(onClick = onDismiss) {
                    Icon(Icons.Default.Close, contentDescription = "Close")
                }
            }

            val analysis = flight.analysis
            if (analysis != null) {
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    MetricCard(
                        title = "Total Time",
                        value = formatDuration(analysis.total_time_seconds),
                        icon = Icons.Default.Timer,
                        modifier = Modifier.weight(1f),
                    )
                    MetricCard(
                        title = "Airborne",
                        value = formatDuration(analysis.airborne_time_seconds),
                        icon = Icons.Default.FlightTakeoff,
                        modifier = Modifier.weight(1f),
                    )
                    MetricCard(
                        title = "Taxi",
                        value = formatDuration(analysis.taxi_time_seconds),
                        icon = Icons.Default.FlightLand,
                        modifier = Modifier.weight(1f),
                    )
                }

                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    MetricCard(
                        title = "Landings",
                        value = "${analysis.full_stop_count} stop / ${analysis.touch_and_go_count} T&G",
                        icon = Icons.Default.FlightLand,
                        modifier = Modifier.weight(1f),
                    )
                    MetricCard(
                        title = "Max Speed",
                        value = "${analysis.max_ground_speed_kt.toInt()} kt",
                        icon = Icons.Default.Speed,
                        modifier = Modifier.weight(1f),
                    )
                    MetricCard(
                        title = "Max Alt",
                        value = "${analysis.max_altitude_ft.toInt()} ft",
                        icon = Icons.Default.FlightTakeoff,
                        modifier = Modifier.weight(1f),
                    )
                }

                if (analysis.segments.isNotEmpty()) {
                    Text(
                        "Flight Phase Segmentation",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)
                        ),
                    ) {
                        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            analysis.segments.forEach { seg ->
                                Row(
                                    Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Row(verticalAlignment = Alignment.CenterVertically) {
                                        PhaseBadge(seg.kind)
                                        Spacer(Modifier.width(8.dp))
                                        Text(
                                            seg.kind,
                                            style = MaterialTheme.typography.bodyMedium,
                                            fontWeight = FontWeight.Medium,
                                        )
                                    }
                                    Text(
                                        "${formatTime(seg.start)} → ${formatTime(seg.end)}",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                        }
                    }
                }
            } else {
                Card(
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)
                    ),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        "Track contains too few points for automatic phase analysis.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }

            HorizontalDivider()

            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Button(
                    onClick = { onShowOnMap(flight) },
                    modifier = Modifier.weight(1f),
                ) {
                    Icon(Icons.Default.Map, contentDescription = null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Show on Map")
                }

                OutlinedButton(
                    onClick = { onExportGpx(flight) },
                    modifier = Modifier.weight(1f),
                ) {
                    Icon(Icons.Default.Share, contentDescription = null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("GPX")
                }

                OutlinedButton(
                    onClick = { onExportCsv(flight) },
                    modifier = Modifier.weight(1f),
                ) {
                    Icon(Icons.Default.FileDownload, contentDescription = null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("CSV")
                }
            }

            OutlinedButton(
                onClick = { onDelete(flight) },
                colors = ButtonDefaults.outlinedButtonColors(
                    contentColor = MaterialTheme.colorScheme.error,
                ),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Icon(Icons.Default.Delete, contentDescription = null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text("Delete Flight Log")
            }

            Spacer(Modifier.height(16.dp))
        }
    }
}

@Composable
private fun MetricCard(
    title: String,
    value: String,
    icon: ImageVector,
    modifier: Modifier = Modifier,
) {
    Card(
        modifier = modifier,
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)),
    ) {
        Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    icon,
                    contentDescription = null,
                    modifier = Modifier.size(14.dp),
                    tint = MaterialTheme.colorScheme.primary,
                )
                Spacer(Modifier.width(4.dp))
                Text(
                    title,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                value,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
            )
        }
    }
}

@Composable
private fun PhaseBadge(kind: String) {
    val color = when (kind.lowercase()) {
        "airborne" -> Color(0xFF4CAF50)
        "taxi" -> Color(0xFF2196F3)
        "ground" -> Color(0xFFFF9800)
        else -> Color.Gray
    }
    Box(
        Modifier
            .size(8.dp)
            .background(color, RoundedCornerShape(4.dp))
    )
}

private fun formatDuration(seconds: Long): String {
    val hours = seconds / 3600
    val minutes = (seconds % 3600) / 60
    return if (hours > 0) {
        "${hours}h ${minutes}m"
    } else {
        "${minutes}m"
    }
}

private fun formatTime(isoString: String): String {
    return runCatching {
        isoString.substringAfter('T').substringBefore('.')
    }.getOrDefault(isoString)
}
