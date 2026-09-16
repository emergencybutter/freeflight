package ws.freeflight.ui

import android.content.Context
import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.FiberManualRecord
import androidx.compose.material.icons.filled.FlightTakeoff
import androidx.compose.material.icons.filled.Map
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ws.freeflight.data.RecordedFlight

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FlightLogScreen(viewModel: FreeflightViewModel) {
    val context = LocalContext.current
    val isRecording by viewModel.isRecording.collectAsState()
    val elapsedSeconds by viewModel.recordingElapsedSeconds.collectAsState()
    val activePoints by viewModel.activeRecordingPoints.collectAsState()
    val latestPoint by viewModel.latestRecordingPoint.collectAsState()
    val latestSpeedKt by viewModel.latestRecordingSpeedKt.collectAsState()
    val savedFlights by viewModel.savedFlights.collectAsState()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Flight Logs & Analysis", fontWeight = FontWeight.Bold) },
                actions = {
                    if (isRecording) {
                        Button(
                            onClick = { viewModel.stopFlightRecording(context) },
                            colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error),
                        ) {
                            Icon(Icons.Default.Stop, contentDescription = null, Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("Stop")
                        }
                    } else {
                        Button(
                            onClick = { viewModel.startFlightRecording(context) },
                        ) {
                            Icon(Icons.Default.PlayArrow, contentDescription = null, Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("Record")
                        }
                    }
                }
            )
        }
    ) { padding ->
        LazyColumn(
            Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            if (isRecording) {
                item {
                    ActiveRecordingCard(
                        elapsedSeconds = elapsedSeconds,
                        altFt = latestPoint?.alt_ft ?: 0.0,
                        speedKt = latestSpeedKt ?: 0.0,
                        pointsCount = activePoints.size,
                        onStop = { viewModel.stopFlightRecording(context) },
                    )
                }
            }

            if (savedFlights.isEmpty() && !isRecording) {
                item {
                    EmptyFlightsCard()
                }
            } else {
                items(savedFlights, key = { it.id }) { flight ->
                    FlightItemCard(
                        flight = flight,
                        onClick = { viewModel.selectFlightForReview(flight) },
                        onShowOnMap = { viewModel.showFlightOnMap(flight) },
                    )
                }
            }

            item {
                Spacer(Modifier.height(24.dp))
            }
        }
    }
}

@Composable
private fun ActiveRecordingCard(
    elapsedSeconds: Long,
    altFt: Double,
    speedKt: Double,
    pointsCount: Int,
    onStop: () -> Unit,
) {
    val hours = elapsedSeconds / 3600
    val minutes = (elapsedSeconds % 3600) / 60
    val seconds = elapsedSeconds % 60
    val timeStr = String.format("%02d:%02d:%02d", hours, minutes, seconds)

    Card(
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.errorContainer.copy(alpha = 0.4f),
        ),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier
                            .size(12.dp)
                            .background(Color.Red, CircleShape)
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(
                        "RECORDING ACTIVE",
                        style = MaterialTheme.typography.labelLarge,
                        fontWeight = FontWeight.Bold,
                        color = Color.Red,
                    )
                }
                Text(
                    timeStr,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                )
            }

            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text("Alt: ${altFt.toInt()} ft", style = MaterialTheme.typography.bodyMedium)
                Text("GS: ${speedKt.toInt()} kt", style = MaterialTheme.typography.bodyMedium)
                Text("Points: $pointsCount", style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

@Composable
private fun FlightItemCard(
    flight: RecordedFlight,
    onClick: () -> Unit,
    onShowOnMap: () -> Unit,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f)),
    ) {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    flight.name,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                val analysis = flight.analysis
                if (analysis != null) {
                    val durationMin = analysis.total_time_seconds / 60
                    val airborneMin = analysis.airborne_time_seconds / 60
                    val landings = analysis.landings.size
                    Spacer(Modifier.height(4.dp))
                    Text(
                        "${durationMin}m total (${airborneMin}m air) • $landings landing(s) • ${analysis.distance_flown_nm.toInt()} nm",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    Spacer(Modifier.height(4.dp))
                    Text(
                        "${flight.points.size} points",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            Row(verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onShowOnMap) {
                    Icon(
                        Icons.Default.Map,
                        contentDescription = "Show on map",
                        tint = MaterialTheme.colorScheme.primary,
                    )
                }
                Icon(
                    Icons.Default.ChevronRight,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun EmptyFlightsCard() {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.3f)),
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .padding(32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(
                Icons.Default.FlightTakeoff,
                contentDescription = null,
                modifier = Modifier.size(48.dp),
                tint = MaterialTheme.colorScheme.primary,
            )
            Text(
                "No Recorded Flights Yet",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                "Tap Record to start tracking your GPS flight path. Freeflight will automatically segment your taxi, climb, cruise, and landings.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            )
        }
    }
}
