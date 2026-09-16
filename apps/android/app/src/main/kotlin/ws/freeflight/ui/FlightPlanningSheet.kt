package ws.freeflight.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.FlightTakeoff
import androidx.compose.material.icons.filled.Scale
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import ws.freeflight.data.AircraftProfileData
import ws.freeflight.data.FlightPlanSummaryData
import ws.freeflight.data.PlannedWaypoint
import kotlin.math.roundToInt

/**
 * Interactive Flight Planning, Nav Log, and Weight & Balance sheet.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FlightPlanningSheet(
    waypoints: List<PlannedWaypoint>,
    profile: AircraftProfileData,
    planSummary: FlightPlanSummaryData?,
    onDismiss: () -> Unit,
    onRemoveWaypoint: (Int) -> Unit,
    onClearRoute: () -> Unit,
    onAddWaypointClick: () -> Unit,
    onProfileChange: (AircraftProfileData) -> Unit,
) {
    var selectedTab by remember { mutableIntStateOf(0) }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(bottom = 32.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        "Flight Plan",
                        style = MaterialTheme.typography.headlineSmall,
                        fontWeight = FontWeight.Bold,
                    )
                    Text(
                        if (waypoints.isNotEmpty()) {
                            waypoints.joinToString(" → ") { it.ident }
                        } else {
                            "No waypoints in route"
                        },
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (waypoints.isNotEmpty()) {
                    TextButton(onClick = onClearRoute) {
                        Text("Clear", color = MaterialTheme.colorScheme.error)
                    }
                }
            }

            Spacer(Modifier.height(12.dp))

            PrimaryTabRow(selectedTabIndex = selectedTab) {
                Tab(
                    selected = selectedTab == 0,
                    onClick = { selectedTab = 0 },
                    text = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(Icons.Default.FlightTakeoff, contentDescription = null)
                            Spacer(Modifier.width(6.dp))
                            Text("Nav Log")
                        }
                    },
                )
                Tab(
                    selected = selectedTab == 1,
                    onClick = { selectedTab = 1 },
                    text = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(Icons.Default.Scale, contentDescription = null)
                            Spacer(Modifier.width(6.dp))
                            Text("Aircraft & W&B")
                        }
                    },
                )
            }

            Spacer(Modifier.height(16.dp))

            when (selectedTab) {
                0 -> NavLogTab(
                    waypoints = waypoints,
                    summary = planSummary,
                    onRemoveWaypoint = onRemoveWaypoint,
                    onAddWaypointClick = onAddWaypointClick,
                )
                1 -> AircraftAndWbTab(
                    profile = profile,
                    onProfileChange = onProfileChange,
                )
            }
        }
    }
}

@Composable
private fun NavLogTab(
    waypoints: List<PlannedWaypoint>,
    summary: FlightPlanSummaryData?,
    onRemoveWaypoint: (Int) -> Unit,
    onAddWaypointClick: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        // Summary Card
        Card(
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainerHigh
            ),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(14.dp),
                horizontalArrangement = Arrangement.SpaceAround,
            ) {
                SummaryMetric(
                    label = "DISTANCE",
                    value = summary?.let { "${it.totalDistanceNm.roundToInt()} NM" } ?: "0 NM",
                )
                SummaryMetric(
                    label = "TOTAL ETE",
                    value = summary?.let { formatEte(it.totalEteHours) } ?: "0m",
                )
                SummaryMetric(
                    label = "REQ FUEL",
                    value = summary?.let { "${"%.1f".format(it.fuel.requiredGal)} GAL" } ?: "0 GAL",
                )
            }
        }

        // Waypoints List
        Text(
            "Route Waypoints",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
        )

        waypoints.forEachIndexed { index, wp ->
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .fillMaxWidth()
                    .background(
                        MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f),
                        RoundedCornerShape(8.dp),
                    )
                    .padding(horizontal = 12.dp, vertical = 8.dp),
            ) {
                Text(
                    "${index + 1}.",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.width(24.dp),
                )
                Column(Modifier.weight(1f)) {
                    Text(
                        wp.ident,
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    wp.name?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                IconButton(onClick = { onRemoveWaypoint(index) }) {
                    Icon(Icons.Default.Delete, contentDescription = "Remove waypoint")
                }
            }
        }

        TextButton(
            onClick = onAddWaypointClick,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Icon(Icons.Default.Add, contentDescription = null)
            Spacer(Modifier.width(6.dp))
            Text("Add Waypoint")
        }

        // Nav Log Table
        if (summary != null && summary.legs.isNotEmpty()) {
            HorizontalDivider()
            Text(
                "Navigation Log",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )

            Row(Modifier.fillMaxWidth()) {
                HeaderCell("LEG", 80.dp)
                HeaderCell("MC / MH", 90.dp)
                HeaderCell("DIST", 60.dp)
                HeaderCell("GS", 50.dp)
                HeaderCell("ETE", 50.dp)
                HeaderCell("FUEL", 50.dp)
            }
            HorizontalDivider()

            summary.legs.forEachIndexed { idx, leg ->
                val fromIdent = waypoints.getOrNull(idx)?.ident ?: "—"
                val toIdent = waypoints.getOrNull(idx + 1)?.ident ?: "—"
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = 6.dp),
                ) {
                    Text(
                        "$fromIdent-$toIdent",
                        style = MonoTextStyle,
                        fontWeight = FontWeight.Bold,
                        modifier = Modifier.width(80.dp),
                    )
                    Text(
                        "${leg.magneticCourseDeg.roundToInt()}° / ${leg.magneticHeadingDeg.roundToInt()}°",
                        style = MonoTextStyle,
                        modifier = Modifier.width(90.dp),
                    )
                    Text(
                        "${leg.distanceNm.roundToInt()} nm",
                        style = MonoTextStyle,
                        modifier = Modifier.width(60.dp),
                    )
                    Text(
                        "${leg.groundSpeedKt.roundToInt()} kt",
                        style = MonoTextStyle,
                        modifier = Modifier.width(50.dp),
                    )
                    Text(
                        "${leg.legMinutes.roundToInt()}m",
                        style = MonoTextStyle,
                        modifier = Modifier.width(50.dp),
                    )
                    Text(
                        "%.1f".format(leg.fuelGal),
                        style = MonoTextStyle,
                        modifier = Modifier.width(50.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun AircraftAndWbTab(
    profile: AircraftProfileData,
    onProfileChange: (AircraftProfileData) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            "Aircraft Performance",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
        )

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            NumberInputField(
                label = "Cruise TAS (kt)",
                value = profile.cruiseTasKt,
                onValueChange = { onProfileChange(profile.copy(cruiseTasKt = it)) },
                modifier = Modifier.weight(1f),
            )
            NumberInputField(
                label = "Fuel Burn (gph)",
                value = profile.fuelBurnGph,
                onValueChange = { onProfileChange(profile.copy(fuelBurnGph = it)) },
                modifier = Modifier.weight(1f),
            )
        }

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            NumberInputField(
                label = "Cruise Altitude (ft)",
                value = profile.cruiseAltitudeFt ?: 5500.0,
                onValueChange = { onProfileChange(profile.copy(cruiseAltitudeFt = it)) },
                modifier = Modifier.weight(1f),
            )
            NumberInputField(
                label = "Reserve (min)",
                value = (profile.reserveMinutes ?: 45).toDouble(),
                onValueChange = { onProfileChange(profile.copy(reserveMinutes = it.toInt())) },
                modifier = Modifier.weight(1f),
            )
        }

        HorizontalDivider()

        Text(
            "Weight & Balance Calculator",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
        )

        // Status Box
        val totalWeight = profile.totalWeightLb
        val maxWeight = profile.maxGrossWeightLb
        val isOver = profile.isOverweight
        val statusBg = if (isOver) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.primaryContainer
        val statusTextColor = if (isOver) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onPrimaryContainer

        Card(
            colors = CardDefaults.cardColors(containerColor = statusBg),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(Modifier.padding(14.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        if (isOver) "OVERWEIGHT" else "WEIGHT & CG NORMAL",
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.Bold,
                        color = statusTextColor,
                        modifier = Modifier.weight(1f),
                    )
                    Text(
                        "${totalWeight.roundToInt()} / ${maxWeight.roundToInt()} lbs",
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.SemiBold,
                        color = statusTextColor,
                    )
                }
                Text(
                    "Calculated CG: ${"%.1f".format(profile.centerOfGravityIn)} in",
                    style = MaterialTheme.typography.labelMedium,
                    color = statusTextColor.copy(alpha = 0.8f),
                )
            }
        }

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            NumberInputField(
                label = "Empty Wt (lb)",
                value = profile.emptyWeightLb,
                onValueChange = { onProfileChange(profile.copy(emptyWeightLb = it)) },
                modifier = Modifier.weight(1f),
            )
            NumberInputField(
                label = "Empty CG (in)",
                value = profile.emptyCgIn,
                onValueChange = { onProfileChange(profile.copy(emptyCgIn = it)) },
                modifier = Modifier.weight(1f),
            )
        }

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            NumberInputField(
                label = "Pilot (lb)",
                value = profile.weightPilotLb,
                onValueChange = { onProfileChange(profile.copy(weightPilotLb = it)) },
                modifier = Modifier.weight(1f),
            )
            NumberInputField(
                label = "Passenger (lb)",
                value = profile.weightPassengerLb,
                onValueChange = { onProfileChange(profile.copy(weightPassengerLb = it)) },
                modifier = Modifier.weight(1f),
            )
        }

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            NumberInputField(
                label = "Baggage (lb)",
                value = profile.weightBaggageLb,
                onValueChange = { onProfileChange(profile.copy(weightBaggageLb = it)) },
                modifier = Modifier.weight(1f),
            )
            NumberInputField(
                label = "Fuel (gal)",
                value = profile.gallonsFuel,
                onValueChange = { onProfileChange(profile.copy(gallonsFuel = it)) },
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun SummaryMetric(label: String, value: String) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            value,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
            fontFamily = FontFamily.Monospace,
        )
    }
}

@Composable
private fun NumberInputField(
    label: String,
    value: Double,
    onValueChange: (Double) -> Unit,
    modifier: Modifier = Modifier,
) {
    OutlinedTextField(
        value = if (value % 1.0 == 0.0) value.toLong().toString() else value.toString(),
        onValueChange = { str ->
            str.toDoubleOrNull()?.let(onValueChange)
        },
        label = { Text(label, style = MaterialTheme.typography.labelSmall) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        modifier = modifier,
    )
}

@Composable
private fun HeaderCell(text: String, width: androidx.compose.ui.unit.Dp) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.width(width),
    )
}

private fun formatEte(hours: Double): String {
    val totalMins = (hours * 60).roundToInt()
    val h = totalMins / 60
    val m = totalMins % 60
    return if (h > 0) "${h}h ${m}m" else "${m}m"
}
