package ws.freeflight.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Cloud
import androidx.compose.material.icons.filled.FiberManualRecord
import androidx.compose.material.icons.filled.Layers
import androidx.compose.material.icons.filled.MyLocation
import androidx.compose.material.icons.filled.Navigation
import androidx.compose.material.icons.filled.Route
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import uniffi.ff_uniffi.BoundingBox
import uniffi.ff_uniffi.ProcedureDetail
import ws.freeflight.map.ChartLayer
import ws.freeflight.map.ChartMap
import ws.freeflight.map.GeoJson
import ws.freeflight.map.LocationTrackingMode
import ws.freeflight.map.MapController

import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Route

/**
 * The chart view: the screen this app exists for.
 *
 * Chrome is kept to the edges and translucent — everything overlaid here is
 * covering a chart the pilot is trying to read.
 */
@Composable
fun MapScreen(viewModel: FreeflightViewModel, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val controller = remember { MapController() }
    val mapState by viewModel.map.collectAsState()
    val cycle by viewModel.cycle.collectAsState()
    val selectedChartId by viewModel.settings.selectedChartId.collectAsState()
    val charts by viewModel.charts.collectAsState()
    val airport by viewModel.airport.collectAsState()
    val procedure by viewModel.procedure.collectAsState()
    val activePlate by viewModel.activePlate.collectAsState()

    val routeWaypoints by viewModel.routeWaypoints.collectAsState()
    val aircraftProfile by viewModel.aircraftProfile.collectAsState()
    val planSummary by viewModel.planSummary.collectAsState()
    val isPlanningOpen by viewModel.isPlanningOpen.collectAsState()
    val windsStatus by viewModel.windsStatus.collectAsState()
    val crossedAirspace by viewModel.crossedAirspace.collectAsState()
    val savedRoutePlans by viewModel.savedRoutePlans.collectAsState()

    val showGairmets by viewModel.settings.showGairmets.collectAsState()
    val showSigmets by viewModel.settings.showSigmets.collectAsState()
    val showCwas by viewModel.settings.showCwas.collectAsState()
    val showPireps by viewModel.settings.showPireps.collectAsState()
    val showBasemap by viewModel.settings.showBasemap.collectAsState()
    val selectedWeatherHazard by viewModel.selectedWeatherHazard.collectAsState()

    val isRecording by viewModel.isRecording.collectAsState()
    val activePoints by viewModel.activeRecordingPoints.collectAsState()
    val mapTrackPoints by viewModel.mapTrackPoints.collectAsState()
    val reviewFlight by viewModel.reviewFlight.collectAsState()
    val recordingElapsedSeconds by viewModel.recordingElapsedSeconds.collectAsState()
    val latestPoint by viewModel.latestRecordingPoint.collectAsState()
    val latestSpeedKt by viewModel.latestRecordingSpeedKt.collectAsState()

    var locationTrackingMode by remember { mutableStateOf(controller.currentTrackingMode) }

    val locationPermissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestMultiplePermissions()
    ) { permissions ->
        val fineGranted = permissions[Manifest.permission.ACCESS_FINE_LOCATION] == true
        val coarseGranted = permissions[Manifest.permission.ACCESS_COARSE_LOCATION] == true
        if (fineGranted || coarseGranted) {
            controller.cycleLocationTrackingMode(context)
        }
    }

    LaunchedEffect(controller) {
        controller.onViewportChanged = viewModel::onViewportChanged
        controller.onAirportTapped = { icao ->
            if (icao != null) viewModel.openAirport(icao) else viewModel.closeAirport()
        }
        controller.onWeatherHazardTapped = { hazard ->
            viewModel.openWeatherHazard(hazard)
        }
        controller.onLocationTrackingModeChanged = { mode ->
            locationTrackingMode = mode
        }
    }

    // The chart layer follows the selection, but only once the chart is
    // actually installed — pointing a raster source at an archive that
    // isn't there produces a screenful of 404s rather than an empty map.
    val selectedChart = charts.firstOrNull { it.id == selectedChartId && it.installed }
    LaunchedEffect(selectedChart?.id, selectedChart?.maxZoom) {
        controller.setChart(
            selectedChart?.let {
                ChartLayer(
                    tileUrlTemplate = viewModel.tileUrlTemplate(it.id),
                    minZoom = it.minZoom.toInt(),
                    maxZoom = it.maxZoom.toInt(),
                )
            }
        )
    }
    LaunchedEffect(mapState.airports, mapState.flightCategories) {
        controller.setAirports(GeoJson.airports(mapState.airports, mapState.flightCategories))
    }
    LaunchedEffect(mapState.airspace) {
        controller.setAirspace(GeoJson.airspace(mapState.airspace))
    }
    LaunchedEffect(mapState.gairmets, showGairmets) {
        controller.setGairmets(if (showGairmets) GeoJson.gairmets(mapState.gairmets) else GeoJson.empty)
    }
    LaunchedEffect(mapState.sigmets, showSigmets) {
        controller.setSigmets(if (showSigmets) GeoJson.sigmets(mapState.sigmets) else GeoJson.empty)
    }
    LaunchedEffect(mapState.cwas, showCwas) {
        controller.setCwas(if (showCwas) GeoJson.cwas(mapState.cwas) else GeoJson.empty)
    }
    LaunchedEffect(mapState.pireps, showPireps) {
        controller.setPireps(if (showPireps) GeoJson.pireps(mapState.pireps) else GeoJson.empty)
    }
    LaunchedEffect(showBasemap) {
        controller.setBasemapVisible(showBasemap)
    }
    LaunchedEffect(procedure) {
        controller.setProcedure(procedure?.let(GeoJson::procedure) ?: GeoJson.empty)
        // A STAR can begin a hundred miles from the field, so drawing it
        // without moving the camera usually means drawing it off-screen.
        procedure?.extent()?.let { controller.fitBounds(it, sheetCoversBottomHalf = true) }
    }
    LaunchedEffect(routeWaypoints) {
        controller.setRoute(GeoJson.route(routeWaypoints))
    }
    LaunchedEffect(mapTrackPoints, activePoints, isRecording) {
        val points = if (isRecording) activePoints else mapTrackPoints
        controller.setTrack(GeoJson.track(points))
    }
    LaunchedEffect(mapTrackPoints) {
        if (mapTrackPoints.size >= 2 && !isRecording) {
            val minLat = mapTrackPoints.minOf { it.lat }
            val maxLat = mapTrackPoints.maxOf { it.lat }
            val minLon = mapTrackPoints.minOf { it.lon }
            val maxLon = mapTrackPoints.maxOf { it.lon }
            val margin = 0.05
            controller.fitBounds(
                BoundingBox(
                    minLat = minLat - margin,
                    minLon = minLon - margin,
                    maxLat = maxLat + margin,
                    maxLon = maxLon + margin,
                )
            )
        }
    }

    Box(modifier.fillMaxSize()) {
        ChartMap(controller, Modifier.fillMaxSize())

        Column(Modifier.fillMaxWidth().padding(12.dp)) {
            SearchBar(viewModel, controller)
            Spacer(Modifier.width(8.dp))
            CycleStatusLine(
                cycleLabel = cycle?.let { info ->
                    "Cycle ${info.effectiveDate ?: info.cycleId}"
                } ?: "No cycle downloaded",
                weatherLabel = mapState.weatherFetchedAtMillis?.let { "Wx ${formatAge(it)}" },
                // Only once there is data to zoom in on — with no cycle
                // installed the reason the map is empty is a different
                // one, and the card in the middle already says it.
                zoomedOutTooFar = mapState.zoomedOutTooFar && cycle != null,
            )

            if (isRecording) {
                Surface(
                    color = MaterialTheme.colorScheme.errorContainer,
                    shape = RoundedCornerShape(8.dp),
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                ) {
                    Row(
                        Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.size(8.dp).background(Color.Red, RoundedCornerShape(4.dp)))
                            Spacer(Modifier.width(6.dp))
                            val hours = recordingElapsedSeconds / 3600
                            val mins = (recordingElapsedSeconds % 3600) / 60
                            val secs = recordingElapsedSeconds % 60
                            Text(
                                String.format("REC %02d:%02d:%02d", hours, mins, secs),
                                style = MaterialTheme.typography.labelMedium,
                                fontWeight = FontWeight.Bold,
                                color = MaterialTheme.colorScheme.onErrorContainer,
                            )
                            Spacer(Modifier.width(10.dp))
                            val alt = latestPoint?.alt_ft ?: 0.0
                            val gs = latestSpeedKt ?: 0.0
                            Text(
                                "${alt.toInt()} ft • ${gs.toInt()} kt",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onErrorContainer,
                            )
                        }
                        IconButton(
                            onClick = { viewModel.stopFlightRecording(context) },
                            modifier = Modifier.size(24.dp),
                        ) {
                            Icon(Icons.Default.Stop, contentDescription = "Stop", tint = Color.Red)
                        }
                    }
                }
            } else if (mapTrackPoints.isNotEmpty()) {
                Surface(
                    color = MaterialTheme.colorScheme.secondaryContainer,
                    shape = RoundedCornerShape(8.dp),
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                ) {
                    Row(
                        Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Text(
                            "Flight Track (${mapTrackPoints.size} points)",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSecondaryContainer,
                        )
                        IconButton(
                            onClick = viewModel::clearMapTrack,
                            modifier = Modifier.size(24.dp),
                        ) {
                            Icon(Icons.Default.Clear, contentDescription = "Clear track")
                        }
                    }
                }
            }
        }

        MapControls(
            viewModel = viewModel,
            controller = controller,
            charts = charts,
            selectedChartId = selectedChartId,
            weatherLoading = mapState.weatherLoading,
            locationTrackingMode = locationTrackingMode,
            hasRoute = routeWaypoints.isNotEmpty(),
            isRecording = isRecording,
            onRequestLocationPermission = {
                locationPermissionLauncher.launch(
                    arrayOf(
                        Manifest.permission.ACCESS_FINE_LOCATION,
                        Manifest.permission.ACCESS_COARSE_LOCATION,
                    )
                )
            },
            modifier = Modifier.align(Alignment.CenterEnd).padding(12.dp),
        )

        var noCycleCardDismissed by remember { mutableStateOf(false) }
        if (cycle == null && !noCycleCardDismissed) {
            NoCycleCard(
                onDismiss = { noCycleCardDismissed = true },
                modifier = Modifier.align(Alignment.Center).padding(24.dp),
            )
        }

        mapState.message?.let { message ->
            MessageBar(
                message = message,
                onDismiss = viewModel::dismissMessage,
                modifier = Modifier.align(Alignment.BottomCenter).padding(12.dp),
            )
        }

        airport?.let { state ->
            AirportSheet(
                state = state,
                onDismiss = viewModel::closeAirport,
                onRefreshWeather = { viewModel.fetchAirportWeather(state.detail.airport.icao) },
                onProcedureSelected = { id ->
                    viewModel.openProcedure(id)
                    viewModel.closeAirport()
                },
                onShowOnMap = {
                    controller.flyTo(state.detail.airport.lat, state.detail.airport.lon)
                },
                onViewPlate = { url, title, subtitle ->
                    viewModel.openPlate(url, title, subtitle)
                },
                onAddRouteWaypoint = { ident, name, lat, lon ->
                    viewModel.addWaypoint(ident, name, lat, lon)
                },
            )
        }

        procedure?.let { detail ->
            ProcedureSheet(
                detail = detail,
                onDismiss = viewModel::closeProcedure,
                onViewPlate = { url, title, subtitle ->
                    viewModel.openPlate(url, title, subtitle)
                },
            )
        }

        activePlate?.let { target ->
            PlateViewer(
                target = target,
                apiClient = viewModel.api,
                onDismiss = viewModel::closePlate,
            )
        }

        if (isPlanningOpen) {
            FlightPlanningSheet(
                waypoints = routeWaypoints,
                profile = aircraftProfile,
                planSummary = planSummary,
                windsStatus = windsStatus,
                crossedAirspace = crossedAirspace,
                savedPlans = savedRoutePlans,
                onDismiss = viewModel::closePlanningSheet,
                onRemoveWaypoint = viewModel::removeWaypoint,
                onClearRoute = viewModel::clearRoute,
                onAddWaypointClick = {
                    viewModel.closePlanningSheet()
                },
                onProfileChange = viewModel::updateProfile,
                onSaveRoute = viewModel::saveCurrentRoute,
                onLoadRoute = viewModel::loadSavedRoute,
                onDeleteRoute = viewModel::deleteSavedRoute,
                onRefreshWinds = viewModel::fetchWindsAloft,
            )
        }

        reviewFlight?.let { flight ->
            FlightReviewSheet(
                flight = flight,
                onDismiss = viewModel::closeFlightReview,
                onShowOnMap = { viewModel.showFlightOnMap(it) },
                onExportGpx = { viewModel.exportFlightGpx(context, it) },
                onExportCsv = { viewModel.exportFlightCsv(context, it) },
                onDelete = { viewModel.deleteFlight(it) },
            )
        }

        selectedWeatherHazard?.let { hazard ->
            WeatherHazardSheet(
                hazard = hazard,
                onDismiss = viewModel::closeWeatherHazard,
            )
        }
    }
}


@Composable
private fun SearchBar(viewModel: FreeflightViewModel, controller: MapController) {
    var query by remember { mutableStateOf("") }
    val results by viewModel.searchResults.collectAsState()

    Column {
        TextField(
            value = query,
            onValueChange = {
                query = it
                viewModel.onSearchQueryChanged(it)
            },
            singleLine = true,
            placeholder = { Text("Airport, navaid or fix") },
            leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
            trailingIcon = {
                if (query.isNotEmpty()) {
                    IconButton(onClick = {
                        query = ""
                        viewModel.clearSearch()
                    }) { Icon(Icons.Default.Clear, contentDescription = "Clear search") }
                }
            },
            shape = RoundedCornerShape(12.dp),
            colors = TextFieldDefaults.colors(
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
            ),
            modifier = Modifier.fillMaxWidth(),
        )

        if (results.isNotEmpty()) {
            Card(
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surface.copy(alpha = 0.97f)
                ),
                modifier = Modifier.fillMaxWidth().heightIn(max = 260.dp),
            ) {
                LazyColumn {
                    items(results, key = { "${it.kind}:${it.ident}" }) { hit ->
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable {
                                    query = ""
                                    viewModel.clearSearch()
                                    controller.flyTo(hit.lat, hit.lon)
                                    if (hit.kind == "airport") viewModel.openAirport(hit.ident)
                                }
                                .padding(horizontal = 16.dp, vertical = 10.dp),
                        ) {
                            Text(
                                hit.ident,
                                fontFamily = FontFamily.Monospace,
                                fontWeight = FontWeight.SemiBold,
                                modifier = Modifier.width(72.dp),
                            )
                            Column(Modifier.weight(1f)) {
                                hit.name?.let { Text(it, style = MaterialTheme.typography.bodyMedium) }
                                Text(
                                    hit.kind.replaceFirstChar(Char::titlecase),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            IconButton(
                                onClick = {
                                    viewModel.addWaypoint(hit.ident, hit.name, hit.lat, hit.lon)
                                    query = ""
                                    viewModel.clearSearch()
                                }
                            ) {
                                Icon(Icons.Default.Add, contentDescription = "Add to route")
                            }
                        }
                        HorizontalDivider()
                    }
                }
            }
        }
    }
}

/**
 * Cycle currency and briefing age, always visible.
 *
 * This is the §11 requirement made literal: the AIRAC cycle in use and the
 * age of the last weather fetch are never more than a glance away, so stale
 * data cannot quietly pass for current.
 */
@Composable
private fun CycleStatusLine(
    cycleLabel: String,
    weatherLabel: String?,
    zoomedOutTooFar: Boolean,
) {
    Surface(
        color = MaterialTheme.colorScheme.surface.copy(alpha = 0.85f),
        shape = RoundedCornerShape(8.dp),
        modifier = Modifier.padding(top = 8.dp),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
        ) {
            Text(cycleLabel, style = MaterialTheme.typography.labelMedium)
            weatherLabel?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (zoomedOutTooFar) {
                Text(
                    "Zoom in for airports",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun MapControls(
    viewModel: FreeflightViewModel,
    controller: MapController,
    charts: List<uniffi.ff_uniffi.Chart>,
    selectedChartId: String?,
    weatherLoading: Boolean,
    locationTrackingMode: LocationTrackingMode,
    hasRoute: Boolean,
    isRecording: Boolean,
    onRequestLocationPermission: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    var layersOpen by remember { mutableStateOf(false) }
    val showBasemap by viewModel.settings.showBasemap.collectAsState()
    val showAirspace by viewModel.settings.showAirspace.collectAsState()
    val showAirports by viewModel.settings.showAirports.collectAsState()
    val showGairmets by viewModel.settings.showGairmets.collectAsState()
    val showSigmets by viewModel.settings.showSigmets.collectAsState()
    val showCwas by viewModel.settings.showCwas.collectAsState()
    val showPireps by viewModel.settings.showPireps.collectAsState()

    val onLocateClick = {
        val fineGranted = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED
        val coarseGranted = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED
        if (!fineGranted && !coarseGranted) {
            onRequestLocationPermission()
        } else {
            controller.cycleLocationTrackingMode(context)
        }
    }

    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        FilledTonalIconButton(
            onClick = {
                if (isRecording) {
                    viewModel.stopFlightRecording(context)
                } else {
                    viewModel.startFlightRecording(context)
                }
            },
            colors = IconButtonDefaults.filledTonalIconButtonColors(
                containerColor = if (isRecording) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.secondaryContainer,
                contentColor = if (isRecording) MaterialTheme.colorScheme.onError else MaterialTheme.colorScheme.onSecondaryContainer,
            ),
        ) {
            Icon(
                if (isRecording) Icons.Default.Stop else Icons.Default.FiberManualRecord,
                contentDescription = if (isRecording) "Stop Recording" else "Record Flight Track",
                tint = if (isRecording) MaterialTheme.colorScheme.onError else Color.Red,
            )
        }

        FilledTonalIconButton(
            onClick = viewModel::openPlanningSheet,
            colors = IconButtonDefaults.filledTonalIconButtonColors(
                containerColor = if (hasRoute) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.secondaryContainer,
                contentColor = if (hasRoute) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSecondaryContainer,
            ),
        ) {
            Icon(Icons.Default.Route, contentDescription = "Flight Plan & Nav Log")
        }
        val locateContainerColor = when (locationTrackingMode) {
            LocationTrackingMode.NONE -> MaterialTheme.colorScheme.secondaryContainer
            LocationTrackingMode.TRACKING, LocationTrackingMode.TRACKING_COMPASS -> MaterialTheme.colorScheme.primary
        }
        val locateContentColor = when (locationTrackingMode) {
            LocationTrackingMode.NONE -> MaterialTheme.colorScheme.onSecondaryContainer
            LocationTrackingMode.TRACKING, LocationTrackingMode.TRACKING_COMPASS -> MaterialTheme.colorScheme.onPrimary
        }
        val locateIcon = when (locationTrackingMode) {
            LocationTrackingMode.TRACKING_COMPASS -> Icons.Default.Navigation
            else -> Icons.Default.MyLocation
        }
        val locateDescription = when (locationTrackingMode) {
            LocationTrackingMode.NONE -> "Locate me"
            LocationTrackingMode.TRACKING -> "Tracking location (North Up)"
            LocationTrackingMode.TRACKING_COMPASS -> "Tracking location (Heading Up)"
        }

        FilledTonalIconButton(
            onClick = { onLocateClick() },
            colors = IconButtonDefaults.filledTonalIconButtonColors(
                containerColor = locateContainerColor,
                contentColor = locateContentColor,
            ),
        ) {
            Icon(locateIcon, contentDescription = locateDescription)
        }

        Box {
            FilledTonalIconButton(onClick = { layersOpen = true }) {
                Icon(Icons.Default.Layers, contentDescription = "Layers")
            }
            DropdownMenu(expanded = layersOpen, onDismissRequest = { layersOpen = false }) {
                Text(
                    "Chart",
                    style = MaterialTheme.typography.labelLarge,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
                DropdownMenuItem(
                    text = { Text("No chart") },
                    onClick = {
                        viewModel.settings.setSelectedChartId(null)
                        layersOpen = false
                    },
                    trailingIcon = { if (selectedChartId == null) Text("✓") },
                )
                // Only what is actually on the device: an uninstalled chart
                // offered here would draw nothing and look broken. The Data
                // tab is where charts are downloaded.
                charts.filter { it.installed }.forEach { chart ->
                    DropdownMenuItem(
                        text = { Text(chart.name) },
                        onClick = {
                            viewModel.settings.setSelectedChartId(chart.id)
                            layersOpen = false
                        },
                        trailingIcon = { if (selectedChartId == chart.id) Text("✓") },
                    )
                }
                if (charts.none { it.installed }) {
                    Text(
                        "No charts downloaded — see the Data tab",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                    )
                }
                HorizontalDivider()
                DropdownMenuItem(
                    text = { Text("Basemap") },
                    onClick = {
                        viewModel.settings.setShowBasemap(!showBasemap)
                    },
                    trailingIcon = { Switch(checked = showBasemap, onCheckedChange = null) },
                )
                DropdownMenuItem(
                    text = { Text("Airports") },
                    onClick = {
                        viewModel.settings.setShowAirports(!showAirports)
                        viewModel.refreshViewport()
                    },
                    trailingIcon = { Switch(checked = showAirports, onCheckedChange = null) },
                )
                DropdownMenuItem(
                    text = { Text("Airspace") },
                    onClick = {
                        viewModel.settings.setShowAirspace(!showAirspace)
                        viewModel.refreshViewport()
                    },
                    trailingIcon = { Switch(checked = showAirspace, onCheckedChange = null) },
                )
                HorizontalDivider()
                Text(
                    "Weather Overlays",
                    style = MaterialTheme.typography.labelLarge,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
                DropdownMenuItem(
                    text = { Text("G-AIRMETs") },
                    onClick = {
                        val next = !showGairmets
                        viewModel.settings.setShowGairmets(next)
                        if (next && viewModel.map.value.weatherFetchedAtMillis == null) {
                            viewModel.refreshVisibleWeather()
                        }
                    },
                    trailingIcon = { Switch(checked = showGairmets, onCheckedChange = null) },
                )
                DropdownMenuItem(
                    text = { Text("SIGMETs") },
                    onClick = {
                        val next = !showSigmets
                        viewModel.settings.setShowSigmets(next)
                        if (next && viewModel.map.value.weatherFetchedAtMillis == null) {
                            viewModel.refreshVisibleWeather()
                        }
                    },
                    trailingIcon = { Switch(checked = showSigmets, onCheckedChange = null) },
                )
                DropdownMenuItem(
                    text = { Text("CWAs") },
                    onClick = {
                        val next = !showCwas
                        viewModel.settings.setShowCwas(next)
                        if (next && viewModel.map.value.weatherFetchedAtMillis == null) {
                            viewModel.refreshVisibleWeather()
                        }
                    },
                    trailingIcon = { Switch(checked = showCwas, onCheckedChange = null) },
                )
                DropdownMenuItem(
                    text = { Text("PIREPs") },
                    onClick = {
                        val next = !showPireps
                        viewModel.settings.setShowPireps(next)
                        if (next && viewModel.map.value.weatherFetchedAtMillis == null) {
                            viewModel.refreshVisibleWeather()
                        }
                    },
                    trailingIcon = { Switch(checked = showPireps, onCheckedChange = null) },
                )
            }
        }

        FilledTonalIconButton(onClick = viewModel::refreshVisibleWeather) {
            if (weatherLoading) {
                CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
            } else {
                Icon(Icons.Default.Cloud, contentDescription = "Refresh weather")
            }
        }
    }
}

@Composable
private fun NoCycleCard(onDismiss: () -> Unit, modifier: Modifier = Modifier) {
    Card(modifier) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("No aeronautical data yet", style = MaterialTheme.typography.titleMedium)
                IconButton(onClick = onDismiss, modifier = Modifier.size(24.dp)) {
                    Icon(Icons.Default.Clear, contentDescription = "Dismiss")
                }
            }
            Text(
                "Download a cycle from the Data tab to use charts, airports and " +
                    "procedures offline.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun MessageBar(message: String, onDismiss: () -> Unit, modifier: Modifier = Modifier) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant,
        shape = RoundedCornerShape(10.dp),
        modifier = modifier,
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.padding(start = 14.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
        ) {
            Text(message, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f, false))
            IconButton(onClick = onDismiss) {
                Icon(Icons.Default.Clear, contentDescription = "Dismiss")
            }
        }
    }
}

/**
 * The geographic extent of a procedure's resolvable fixes, or null when
 * fewer than two of them resolved to a coordinate — a one-point "extent"
 * is not something to frame the camera on.
 *
 * Padded slightly so the outermost fixes do not sit exactly on the edge of
 * the screen.
 */
private fun ProcedureDetail.extent(): BoundingBox? {
    val points = transitions
        .flatMap { it.legs }
        .mapNotNull { leg ->
            val lat = leg.lat ?: return@mapNotNull null
            val lon = leg.lon ?: return@mapNotNull null
            lat to lon
        }
    if (points.size < 2) return null
    val margin = 0.05
    return BoundingBox(
        minLat = points.minOf { it.first } - margin,
        minLon = points.minOf { it.second } - margin,
        maxLat = points.maxOf { it.first } + margin,
        maxLon = points.maxOf { it.second } + margin,
    )
}

/** A flight-category dot, in the colours every other briefing product uses. */
@Composable
fun FlightCategoryDot(category: String?, modifier: Modifier = Modifier) {
    val color = when (category?.uppercase()) {
        "VFR" -> Color(0xFF4CAF50)
        "MVFR" -> Color(0xFF2196F3)
        "IFR" -> Color(0xFFF44336)
        "LIFR" -> Color(0xFFE040FB)
        else -> Color(0xFF9E9E9E)
    }
    Box(modifier.size(10.dp).background(color, RoundedCornerShape(5.dp)))
}
