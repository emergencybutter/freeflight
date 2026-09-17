package ws.freeflight.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material.icons.filled.Cloud
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.FlightTakeoff
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import ws.freeflight.data.AirspaceCrossingWarning
import ws.freeflight.data.Aircraft
import ws.freeflight.data.FlightPlanSummaryData
import ws.freeflight.data.PlannedWaypoint
import ws.freeflight.data.SavedRoutePlan
import kotlin.math.roundToInt

/**
 * The Plan tab: the route, its nav log, and the saved-route library.
 *
 * Aircraft and weight & balance live in their own tab — they are set up once
 * for an aeroplane and then left alone, which is a different rhythm from a
 * route that is rebuilt every flight.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PlanScreen(
    viewModel: FreeflightViewModel,
    onAddWaypointClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val waypoints by viewModel.routeWaypoints.collectAsState()
    val planSummary by viewModel.planSummary.collectAsState()
    val windsStatus by viewModel.windsStatus.collectAsState()
    val crossedAirspace by viewModel.crossedAirspace.collectAsState()
    val savedPlans by viewModel.savedRoutePlans.collectAsState()

    LaunchedEffect(Unit) { viewModel.ensureWindsLoaded() }

    var selectedTab by rememberSaveable { mutableIntStateOf(0) }

    Column(
        modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .padding(top = 16.dp, bottom = 32.dp),
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
                TextButton(onClick = viewModel::clearRoute) {
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
                        Icon(Icons.Default.Bookmark, contentDescription = null)
                        Spacer(Modifier.width(6.dp))
                        Text("Routes")
                    }
                },
            )
        }

        Spacer(Modifier.height(16.dp))

        when (selectedTab) {
            0 -> NavLogTab(
                waypoints = waypoints,
                summary = planSummary,
                windsStatus = windsStatus,
                crossedAirspace = crossedAirspace,
                onRemoveWaypoint = viewModel::removeWaypoint,
                onAddWaypointClick = onAddWaypointClick,
                onRefreshWinds = viewModel::fetchWindsAloft,
            )
            1 -> SavedRoutesTab(
                waypoints = waypoints,
                savedPlans = savedPlans,
                onSaveRoute = viewModel::saveCurrentRoute,
                onLoadRoute = { plan ->
                    viewModel.loadSavedRoute(plan)
                    selectedTab = 0
                },
                onDeleteRoute = viewModel::deleteSavedRoute,
            )
        }
    }
}

@Composable
private fun NavLogTab(
    waypoints: List<PlannedWaypoint>,
    summary: FlightPlanSummaryData?,
    windsStatus: String?,
    crossedAirspace: List<AirspaceCrossingWarning>,
    onRemoveWaypoint: (Int) -> Unit,
    onAddWaypointClick: () -> Unit,
    onRefreshWinds: () -> Unit,
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

        // Airspace Crossings Warnings Card
        if (crossedAirspace.isNotEmpty()) {
            Card(
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.errorContainer.copy(alpha = 0.25f),
                ),
                shape = RoundedCornerShape(8.dp),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(Modifier.padding(12.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(
                            Icons.Default.Warning,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.error,
                            modifier = Modifier.size(18.dp),
                        )
                        Spacer(Modifier.width(8.dp))
                        Text(
                            "Airspace Crossings Along Route",
                            fontWeight = FontWeight.Bold,
                            style = MaterialTheme.typography.titleSmall,
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                    Spacer(Modifier.height(6.dp))
                    crossedAirspace.forEach { warning ->
                        Text(
                            "• ${warning.formattedClass}: ${warning.name} (${warning.floor}–${warning.ceiling})",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurface,
                        )
                    }
                }
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
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    "Navigation Log",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.weight(1f),
                )
            }

            // Winds Aloft Status indicator
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .fillMaxWidth()
                    .background(
                        MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.35f),
                        RoundedCornerShape(6.dp),
                    )
                    .padding(horizontal = 10.dp, vertical = 6.dp),
            ) {
                Icon(
                    Icons.Default.Cloud,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = if (windsStatus != null) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    if (windsStatus != null) "Winds aloft: $windsStatus" else "Winds aloft: Offline / no winds data (GS = TAS)",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                IconButton(
                    onClick = onRefreshWinds,
                    modifier = Modifier.size(24.dp),
                ) {
                    Icon(Icons.Default.Refresh, contentDescription = "Refresh winds", modifier = Modifier.size(14.dp))
                }
            }

            Spacer(Modifier.height(4.dp))

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
private fun SavedRoutesTab(
    waypoints: List<PlannedWaypoint>,
    savedPlans: List<SavedRoutePlan>,
    onSaveRoute: (String) -> Unit,
    onLoadRoute: (SavedRoutePlan) -> Unit,
    onDeleteRoute: (Long) -> Unit,
) {
    var routeName by remember { mutableStateOf("") }

    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        // Save active route card
        Card(
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainerHigh
            ),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    "Save Active Route",
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    if (waypoints.isNotEmpty()) {
                        waypoints.joinToString(" → ") { it.ident }
                    } else {
                        "Add waypoints in Nav Log to save this flight plan"
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    OutlinedTextField(
                        value = routeName,
                        onValueChange = { routeName = it },
                        placeholder = { Text(if (waypoints.isNotEmpty()) waypoints.joinToString(" - ") { it.ident } else "Route name") },
                        singleLine = true,
                        modifier = Modifier.weight(1f),
                    )
                    Button(
                        onClick = {
                            val name = routeName.ifBlank {
                                if (waypoints.isNotEmpty()) waypoints.joinToString(" - ") { it.ident } else "Route"
                            }
                            onSaveRoute(name)
                            routeName = ""
                        },
                        enabled = waypoints.isNotEmpty(),
                    ) {
                        Text("Save")
                    }
                }
            }
        }

        HorizontalDivider()

        Text(
            "Saved Route Library",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
        )

        if (savedPlans.isEmpty()) {
            Text(
                "No saved routes yet. Plan a route above and save it to reload anytime.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        } else {
            savedPlans.forEach { plan ->
                Card(
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)
                    ),
                    shape = RoundedCornerShape(8.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(12.dp),
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text(
                                plan.name,
                                fontWeight = FontWeight.Bold,
                                style = MaterialTheme.typography.bodyLarge,
                            )
                            Text(
                                plan.summaryText,
                                fontFamily = FontFamily.Monospace,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.primary,
                            )
                            Text(
                                "${plan.waypoints.size} waypoints",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        TextButton(onClick = { onLoadRoute(plan) }) {
                            Text("Load")
                        }
                        IconButton(onClick = { onDeleteRoute(plan.id) }) {
                            Icon(Icons.Default.Delete, contentDescription = "Delete route")
                        }
                    }
                }
            }
        }
    }
}

@Composable
internal fun SummaryMetric(
    label: String,
    value: String,
    color: Color = Color.Unspecified,
) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            value,
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.Bold,
            color = color,
        )
    }
}

@Composable
private fun HeaderCell(text: String, width: Dp) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        fontWeight = FontWeight.Bold,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.width(width),
    )
}

@Composable
internal fun NumberInputField(
    label: String,
    value: Double,
    onValueChange: (Double) -> Unit,
    modifier: Modifier = Modifier,
) {
    var text by remember(value) { mutableStateOf(if (value % 1.0 == 0.0) value.toInt().toString() else "%.1f".format(value)) }

    OutlinedTextField(
        value = text,
        onValueChange = {
            text = it
            it.toDoubleOrNull()?.let(onValueChange)
        },
        label = { Text(label) },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
        singleLine = true,
        modifier = modifier,
    )
}

private fun formatEte(hours: Double): String {
    val totalMinutes = (hours * 60).roundToInt()
    val h = totalMinutes / 60
    val m = totalMinutes % 60
    return if (h > 0) "${h}h ${m}m" else "${m}m"
}

/**
 * A POH cruise power setting — "65%", "2400 RPM".
 *
 * Free text rather than a picker because POHs disagree about what they
 * key cruise tables on, and forcing one vocabulary would make some
 * aircraft untypeable.
 */
@Composable
internal fun PowerSettingField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        label = { Text("") },
        singleLine = true,
        modifier = modifier,
    )
}
