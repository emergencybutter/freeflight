package ws.freeflight.ui

import android.content.Context
import android.content.Intent
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.builtins.nullable
import uniffi.ff_uniffi.Airport
import uniffi.ff_uniffi.AirportDetail
import uniffi.ff_uniffi.Airspace
import uniffi.ff_uniffi.BoundingBox
import uniffi.ff_uniffi.CoreException
import uniffi.ff_uniffi.Plate
import uniffi.ff_uniffi.Procedure
import uniffi.ff_uniffi.ProcedureDetail
import uniffi.ff_uniffi.SearchHit
import ws.freeflight.AppContainer
import ws.freeflight.data.ChartKinds
import ws.freeflight.data.ChartSet
import ws.freeflight.data.ChartSheets
import ws.freeflight.data.CycleStatus
import ws.freeflight.data.CycleStatusReader
import ws.freeflight.data.Cwa
import ws.freeflight.data.GAirmet
import ws.freeflight.data.Metar
import ws.freeflight.data.MixedCycle
import ws.freeflight.data.Pirep
import ws.freeflight.data.Sigmet
import ws.freeflight.data.StaleSource
import ws.freeflight.data.Taf
import ws.freeflight.data.WeatherHazardTap

/** Everything the map screen draws, in one snapshot. */
data class MapUiState(
    val airports: List<Airport> = emptyList(),
    val airspace: List<Airspace> = emptyList(),
    val gairmets: List<GAirmet> = emptyList(),
    val sigmets: List<Sigmet> = emptyList(),
    val cwas: List<Cwa> = emptyList(),
    val pireps: List<Pirep> = emptyList(),
    /** Live flight categories by ICAO, from the last weather fetch. */
    val flightCategories: Map<String, String> = emptyMap(),
    val weatherFetchedAtMillis: Long? = null,
    val weatherLoading: Boolean = false,
    /** Set when a query failed for a reason worth telling the pilot about. */
    val message: String? = null,
    /** True below the zoom at which drawing every airport stops being legible. */
    val zoomedOutTooFar: Boolean = false,
)

/** The airport sheet's contents, loaded in stages so the sheet can open instantly. */
data class AirportUiState(
    val detail: AirportDetail,
    val procedures: List<Procedure> = emptyList(),
    val metar: Metar? = null,
    val taf: Taf? = null,
    val weatherLoading: Boolean = false,
    val weatherError: String? = null,
    val fetchedAtMillis: Long? = null,
    /** Plates this airport publishes, with `installed` as of the last read. */
    val plates: List<Plate> = emptyList(),
)

class FreeflightViewModel(private val container: AppContainer) : ViewModel() {

    private val core = container.core

    val cycle = container.cycles.cycle
    val sync = container.cycles.sync
    /**
     * The catalogue in the order it is offered: kinds grouped the way
     * the layers menu groups them, and sheets within a kind in their own
     * order — which for IFR is by panel number, so L-2 precedes L-10.
     */
    val charts = container.cycles.charts
        .map { list ->
            list.sortedWith(
                compareBy({ ChartKinds.order(it.kind) }, { ChartSheets.sortKey(it) }),
            )
        }
        .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())
    val chartDownloads = container.cycles.chartDownloads
    val setDownload = container.cycles.setDownload

    private val _chartSets = MutableStateFlow<List<ChartSet>>(emptyList())

    /** Bulk download options, recomputed as the catalogue or the map moves. */
    val chartSets: StateFlow<List<ChartSet>> = _chartSets.asStateFlow()
    val settings = container.settings

    private val _map = MutableStateFlow(MapUiState())
    val map: StateFlow<MapUiState> = _map.asStateFlow()

    val api = container.api

    /** The plate store, for the viewer to read from disk. */
    val plates = container.plates

    val plateDownload = container.plates.download

    /** Sources older than the cycle itself, for the mixed-cycle notice. */
    private val _cycleStatus = MutableStateFlow<CycleStatus?>(null)
    val cycleStatus: StateFlow<CycleStatus?> = _cycleStatus.asStateFlow()

    private val _staleSources = MutableStateFlow<List<StaleSource>>(emptyList())
    val staleSources: StateFlow<List<StaleSource>> = _staleSources.asStateFlow()

    private val _plateBytes = MutableStateFlow(0L)
    val plateBytes: StateFlow<Long> = _plateBytes.asStateFlow()

    private val _searchResults = MutableStateFlow<List<SearchHit>>(emptyList())
    val searchResults: StateFlow<List<SearchHit>> = _searchResults.asStateFlow()

    private val _airport = MutableStateFlow<AirportUiState?>(null)
    val airport: StateFlow<AirportUiState?> = _airport.asStateFlow()

    private val _procedure = MutableStateFlow<ProcedureDetail?>(null)
    val procedure: StateFlow<ProcedureDetail?> = _procedure.asStateFlow()

    private val _activePlate = MutableStateFlow<PlateTarget?>(null)
    val activePlate: StateFlow<PlateTarget?> = _activePlate.asStateFlow()

    fun openPlate(url: String, title: String, subtitle: String? = null) {
        _activePlate.value = PlateTarget(url, title, subtitle)
    }

    fun closePlate() {
        _activePlate.value = null
    }

    private val _selectedWeatherHazard = MutableStateFlow<WeatherHazardTap?>(null)
    val selectedWeatherHazard: StateFlow<WeatherHazardTap?> = _selectedWeatherHazard.asStateFlow()

    fun openWeatherHazard(hazard: WeatherHazardTap?) {
        _selectedWeatherHazard.value = hazard
    }

    fun closeWeatherHazard() {
        _selectedWeatherHazard.value = null
    }

    private val jsonSerializer = kotlinx.serialization.json.Json { ignoreUnknownKeys = true; isLenient = true }

    private val _routeWaypoints = MutableStateFlow<List<ws.freeflight.data.PlannedWaypoint>>(emptyList())
    val routeWaypoints: StateFlow<List<ws.freeflight.data.PlannedWaypoint>> = _routeWaypoints.asStateFlow()

    /** Every aircraft on this device, and which one planning uses. */
    val fleet = container.aircraft.fleet
    val selectedAircraftId = container.aircraft.selectedId

    private val _aircraftProfile = MutableStateFlow(ws.freeflight.data.AircraftProfileData())
    val aircraftProfile: StateFlow<ws.freeflight.data.AircraftProfileData> = _aircraftProfile.asStateFlow()

    private val _planSummary = MutableStateFlow<ws.freeflight.data.FlightPlanSummaryData?>(null)
    val planSummary: StateFlow<ws.freeflight.data.FlightPlanSummaryData?> = _planSummary.asStateFlow()

    private val _windsBulletin = MutableStateFlow<ws.freeflight.data.WindsAloftBulletin?>(null)
    private val _windsStatus = MutableStateFlow<String?>(null)
    val windsStatus: StateFlow<String?> = _windsStatus.asStateFlow()

    private val _crossedAirspace = MutableStateFlow<List<ws.freeflight.data.AirspaceCrossingWarning>>(emptyList())
    val crossedAirspace: StateFlow<List<ws.freeflight.data.AirspaceCrossingWarning>> = _crossedAirspace.asStateFlow()

    val savedRoutePlans: StateFlow<List<ws.freeflight.data.SavedRoutePlan>> = container.routePlanning.savedPlans

    /**
     * Winds are fetched the first time planning is actually looked at, not on
     * startup: it is a network call whose answer only matters once there is a
     * nav log on screen.
     */
    fun ensureWindsLoaded() {
        if (_windsBulletin.value == null) {
            fetchWindsAloft()
        }
    }

    private val _isFlightLogOpen = MutableStateFlow(false)
    val isFlightLogOpen: StateFlow<Boolean> = _isFlightLogOpen.asStateFlow()

    fun openFlightLog() { _isFlightLogOpen.value = true }
    fun closeFlightLog() { _isFlightLogOpen.value = false }

    fun addWaypoint(ident: String, name: String? = null, lat: Double, lon: Double) {
        val current = _routeWaypoints.value.toMutableList()
        current.add(ws.freeflight.data.PlannedWaypoint(ident, name, lat, lon))
        _routeWaypoints.value = current
        recalculatePlan()
    }

    fun removeWaypoint(index: Int) {
        val current = _routeWaypoints.value.toMutableList()
        if (index in current.indices) {
            current.removeAt(index)
            _routeWaypoints.value = current
            recalculatePlan()
        }
    }

    fun clearRoute() {
        _routeWaypoints.value = emptyList()
        _planSummary.value = null
        _crossedAirspace.value = emptyList()
        viewModelScope.launch {
            container.routePlanning.clearActiveRoute()
        }
    }

    // ---- fleet -----------------------------------------------------------

    /**
     * Point planning at a different aircraft.
     *
     * The loading figures — who is aboard, how much fuel — belong to the
     * flight rather than the airframe, so they survive the switch; only
     * the aircraft's own numbers are replaced.
     */
    fun selectAircraft(id: Long) {
        container.aircraft.select(id)
        viewModelScope.launch {
            fleet.value.firstOrNull { it.id == id }?.let { aircraft ->
                _aircraftProfile.value = aircraft.toProfile(_aircraftProfile.value)
                recalculatePlan()
            }
        }
    }

    fun saveAircraft(aircraft: ws.freeflight.data.Aircraft) {
        viewModelScope.launch {
            val saved = container.aircraft.save(aircraft)
            if (saved.id == selectedAircraftId.value) {
                _aircraftProfile.value = saved.toProfile(_aircraftProfile.value)
                recalculatePlan()
            }
        }
    }

    /**
     * Delete an aircraft. The repository refuses to remove the last one —
     * planning has no meaning without one — and the picker hides the
     * control in that case, so this is fire-and-forget.
     */
    fun deleteAircraft(id: Long) {
        viewModelScope.launch { container.aircraft.delete(id) }
    }

    fun setAircraftPerformance(
        aircraftId: Long,
        phase: ws.freeflight.data.PerformancePhase,
        rows: List<ws.freeflight.data.PerformancePoint>,
    ) {
        viewModelScope.launch {
            container.aircraft.setPerformance(aircraftId, phase, rows)
            fleet.value.firstOrNull { it.id == aircraftId }?.let {
                if (it.id == selectedAircraftId.value) {
                    _aircraftProfile.value = it.toProfile(_aircraftProfile.value)
                    recalculatePlan()
                }
            }
        }
    }

    fun markAircraftVerified(id: Long, verified: Boolean) {
        viewModelScope.launch { container.aircraft.markVerified(id, verified) }
    }

    fun updateProfile(profile: ws.freeflight.data.AircraftProfileData) {
        _aircraftProfile.value = profile
        // Persist the airframe half back to the fleet, or an edit would
        // last exactly as long as the current selection.
        fleet.value.firstOrNull { it.id == selectedAircraftId.value }?.let { aircraft ->
            viewModelScope.launch { container.aircraft.save(aircraft.updatedFrom(profile)) }
        }
        recalculatePlan()
    }

    fun saveCurrentRoute(name: String) {
        val waypoints = _routeWaypoints.value
        if (waypoints.isEmpty()) return
        viewModelScope.launch {
            container.routePlanning.saveNamedPlan(name, waypoints)
        }
    }

    fun loadSavedRoute(plan: ws.freeflight.data.SavedRoutePlan) {
        _routeWaypoints.value = plan.waypoints
        recalculatePlan()
    }

    fun deleteSavedRoute(id: Long) {
        viewModelScope.launch {
            container.routePlanning.deleteSavedPlan(id)
        }
    }

    fun fetchWindsAloft() {
        viewModelScope.launch {
            try {
                val bulletin = container.api.windsAloft()
                _windsBulletin.value = bulletin
                _windsStatus.value = "NOAA ${bulletin.validTime} forecast"
                recalculatePlan()
            } catch (_: Exception) {
                _windsStatus.value = null
            }
        }
    }

    private fun recalculatePlan() {
        viewModelScope.launch(Dispatchers.Default) {
            val waypoints = _routeWaypoints.value
            val profile = _aircraftProfile.value

            // Auto-persist active route to SQLite
            container.routePlanning.saveActiveRoute(waypoints, profile)

            if (waypoints.size < 2) {
                _planSummary.value = null
                _crossedAirspace.value = emptyList()
                return@launch
            }

            // 1. Winds-aloft interpolation per leg
            val winds = _windsBulletin.value?.let { bulletin ->
                ws.freeflight.data.WindsAloftResolver.windsForRoute(
                    points = waypoints,
                    cruiseAltitudeFt = profile.cruiseAltitudeFt ?: 5500.0,
                    bulletin = bulletin,
                )
            } ?: emptyList()

            val windsJson = if (winds.isNotEmpty()) {
                jsonSerializer.encodeToString(
                    kotlinx.serialization.builtins.ListSerializer(
                        ws.freeflight.data.PlanningWind.serializer().nullable
                    ),
                    winds,
                )
            } else {
                "[]"
            }

            // 2. Compute dynamic decimal year for magnetic declination model
            val now = java.time.LocalDate.now()
            val year = now.year + (now.dayOfYear.toDouble() / (if (now.isLeapYear) 366.0 else 365.0))

            try {
                val pointsJson = jsonSerializer.encodeToString(
                    kotlinx.serialization.builtins.ListSerializer(ws.freeflight.data.PlannedWaypoint.serializer()),
                    waypoints,
                )
                val profileJson = jsonSerializer.encodeToString(
                    ws.freeflight.data.AircraftProfileData.serializer(),
                    profile,
                )

                val resultJson = uniffi.ff_uniffi.planRouteJson(
                    pointsJson = pointsJson,
                    profileJson = profileJson,
                    windsJson = windsJson,
                    decimalYear = year,
                )
                val summary = jsonSerializer.decodeFromString(
                    ws.freeflight.data.FlightPlanSummaryData.serializer(),
                    resultJson,
                )
                _planSummary.value = summary
            } catch (_: Exception) {
                // Ignore calculation errors for incomplete route
            }

            // 3. Airspace crossing detection along route
            try {
                val lats = waypoints.map { it.lat }
                val lons = waypoints.map { it.lon }
                val minLat = (lats.minOrNull() ?: 0.0) - 0.5
                val maxLat = (lats.maxOrNull() ?: 0.0) + 0.5
                val minLon = (lons.minOrNull() ?: 0.0) - 0.5
                val maxLon = (lons.maxOrNull() ?: 0.0) + 0.5

                val bbox = uniffi.ff_uniffi.BoundingBox(minLat, minLon, maxLat, maxLon)
                val volumes = withContext(Dispatchers.IO) {
                    runCatching { core.airspaceInBbox(bbox, ROUTE_AIRSPACE_LIMIT) }
                        .getOrDefault(emptyList())
                }
                val crossings = ws.freeflight.data.AirspaceCrossingDetector.findCrossedAirspace(waypoints, volumes)
                _crossedAirspace.value = crossings
            } catch (_: Exception) {
                _crossedAirspace.value = emptyList()
            }
        }
    }

    // ---- Flight Recording & Post-flight Analysis ------------------------

    val flightRecording = container.flightRecording
    val isRecording = flightRecording.isRecording
    val activeRecordingPoints = flightRecording.activePoints
    val recordingElapsedSeconds = flightRecording.elapsedSeconds
    val latestRecordingPoint = flightRecording.latestPoint
    val latestRecordingSpeedKt = flightRecording.latestSpeedKt
    val savedFlights = flightRecording.savedFlights
    val reviewFlight = flightRecording.reviewFlight

    private val _mapTrackPoints = MutableStateFlow<List<ws.freeflight.data.RecordedPoint>>(emptyList())
    val mapTrackPoints: StateFlow<List<ws.freeflight.data.RecordedPoint>> = _mapTrackPoints.asStateFlow()

    fun startFlightRecording(context: Context) {
        ws.freeflight.data.FlightRecordingService.start(context)
    }

    fun stopFlightRecording(context: Context) {
        ws.freeflight.data.FlightRecordingService.stop(context)
    }

    fun selectFlightForReview(flight: ws.freeflight.data.RecordedFlight?) {
        flightRecording.selectFlightForReview(flight)
    }

    fun closeFlightReview() {
        flightRecording.selectFlightForReview(null)
    }

    fun showFlightOnMap(flight: ws.freeflight.data.RecordedFlight) {
        _mapTrackPoints.value = flight.points
        closeFlightReview()
        // The log is an overlay on the map now, so drawing a track under it
        // would put the answer behind the thing that was asked.
        closeFlightLog()
    }

    fun clearMapTrack() {
        _mapTrackPoints.value = emptyList()
    }

    fun deleteFlight(flight: ws.freeflight.data.RecordedFlight) {
        flightRecording.deleteFlight(flight.id)
        if (_mapTrackPoints.value == flight.points) {
            clearMapTrack()
        }
    }

    fun exportFlightGpx(context: Context, flight: ws.freeflight.data.RecordedFlight) {
        val gpx = flightRecording.exportGpx(flight)
        if (gpx.isEmpty()) return
        val intent = Intent(Intent.ACTION_SEND).apply {
            type = "text/xml"
            putExtra(Intent.EXTRA_SUBJECT, "${flight.name}.gpx")
            putExtra(Intent.EXTRA_TEXT, gpx)
        }
        context.startActivity(Intent.createChooser(intent, "Export GPX Track"))
    }

    fun exportFlightCsv(context: Context, flight: ws.freeflight.data.RecordedFlight) {
        val csv = flightRecording.exportCsv(flight)
        if (csv.isEmpty()) return
        val intent = Intent(Intent.ACTION_SEND).apply {
            type = "text/csv"
            putExtra(Intent.EXTRA_SUBJECT, "${flight.name}.csv")
            putExtra(Intent.EXTRA_TEXT, csv)
        }
        context.startActivity(Intent.createChooser(intent, "Export Flight Log CSV"))
    }

    private var viewportJob: Job? = null
    private var chartSetJob: Job? = null
    private var searchJob: Job? = null
    private var lastViewport: BoundingBox? = null
    private var lastZoom: Double = MIN_AIRPORT_ZOOM

    init {
        // The catalogue arrives asynchronously after a sync, so the sets
        // follow it rather than being built once.
        viewModelScope.launch {
            charts.collect { refreshChartSets() }
        }

        // Every plate that lands changes a tick in the open airport sheet,
        // so the list tracks the store rather than the moment it opened.
        viewModelScope.launch {
            container.plates.revision.collect {
                _airport.value?.detail?.airport?.icao?.let { refreshPlates(it) }
                _plateBytes.value = container.plates.storedBytes()
            }
        }

        // Restore active flight plan from SQLite
        viewModelScope.launch {
            val active = container.routePlanning.loadActiveRoute()
            if (active != null) {
                _routeWaypoints.value = active.first
                _aircraftProfile.value = active.second
                recalculatePlan()
            }
        }

        // The cycle's own date is the FAA's; anything older in the
        // bundle has to be called out rather than hidden behind it.
        viewModelScope.launch {
            cycle.collect { info ->
                // Falls back to the cycle id, which is the same date
                // string: no bundle published before `add_airac_cycle`
                // existed carries an `airac_cycle` row, and without this
                // the comparison would silently find nothing stale in
                // exactly the bundles most likely to be mixed.
                val cycleDate = info?.effectiveDate ?: info?.cycleId
                _staleSources.value = MixedCycle.staleSources(attributions(), cycleDate)
                _cycleStatus.value = CycleStatusReader.of(cycleDate)
            }
        }

        // Fetch winds aloft forecast on boot
        fetchWindsAloft()
    }

    private fun refreshChartSets() {
        _chartSets.value = ChartSet.forCatalogue(charts.value, lastViewport)
    }

    /**
     * Reload what is in view.
     *
     * Debounced rather than run per camera frame: a pan fires this
     * continuously, and each call is a SQLite query plus a GeoJSON rebuild
     * of up to a few hundred features.
     */
    fun onViewportChanged(bbox: BoundingBox, zoom: Double) {
        lastViewport = bbox
        lastZoom = zoom
        // "Covering the map view" depends on where the map is, so it is
        // rebuilt as the map settles — debounced with everything else, not
        // per camera frame.
        chartSetJob?.cancel()
        chartSetJob = viewModelScope.launch {
            delay(VIEWPORT_DEBOUNCE_MS)
            refreshChartSets()
        }
        viewportJob?.cancel()
        viewportJob = viewModelScope.launch {
            delay(VIEWPORT_DEBOUNCE_MS)
            loadViewport(bbox, zoom)
        }
    }

    private suspend fun loadViewport(bbox: BoundingBox, zoom: Double) {
        // Below this nothing is drawn at all, so there is nothing to
        // fetch. Above it the map layer decides *which* of these to show
        // for the zoom (ChartMap.airportDisplayFilter) — this gate is only
        // about not querying for markers that cannot appear.
        if (zoom < MIN_AIRPORT_ZOOM) {
            _map.value = _map.value.copy(
                airports = emptyList(),
                airspace = emptyList(),
                zoomedOutTooFar = true,
                message = null,
            )
            return
        }
        try {
            val airports = withContext(Dispatchers.IO) {
                core.airportsInBbox(bbox, AIRPORT_LIMIT)
            }
            val airspace = withContext(Dispatchers.IO) {
                if (container.settings.showAirspace.value) {
                    core.airspaceInBbox(bbox, AIRSPACE_LIMIT)
                } else {
                    emptyList()
                }
            }
            _map.value = _map.value.copy(
                airports = if (container.settings.showAirports.value) airports else emptyList(),
                airspace = airspace,
                zoomedOutTooFar = false,
                message = null,
            )
        } catch (e: CoreException.NoCycle) {
            // Not an error state to apologise for — it is the first-run
            // state, and the UI says so plainly (DESIGN.md §11).
            _map.value = MapUiState(message = null)
        } catch (e: Exception) {
            _map.value = _map.value.copy(message = e.message)
        }
    }

    /** Re-run the last viewport query — after a sync, or a layer toggle. */
    fun refreshViewport() {
        val bbox = lastViewport ?: return
        viewModelScope.launch { loadViewport(bbox, lastZoom) }
    }

    // ---- search ----------------------------------------------------------

    fun onSearchQueryChanged(query: String) {
        searchJob?.cancel()
        if (query.isBlank()) {
            _searchResults.value = emptyList()
            return
        }
        searchJob = viewModelScope.launch {
            delay(SEARCH_DEBOUNCE_MS)
            _searchResults.value = try {
                withContext(Dispatchers.IO) { core.search(query, SEARCH_LIMIT) }
            } catch (e: Exception) {
                emptyList()
            }
        }
    }

    fun clearSearch() {
        searchJob?.cancel()
        _searchResults.value = emptyList()
    }

    // ---- airport sheet ---------------------------------------------------

    fun openAirport(icao: String) {
        viewModelScope.launch {
            val detail = try {
                withContext(Dispatchers.IO) { core.airport(icao) }
            } catch (e: Exception) {
                _map.value = _map.value.copy(message = "Couldn't open $icao: ${e.message}")
                return@launch
            }
            _airport.value = AirportUiState(detail = detail, weatherLoading = true)

            val procedures = try {
                withContext(Dispatchers.IO) { core.airportProcedures(icao) }
            } catch (e: Exception) {
                emptyList()
            }
            _airport.value = _airport.value?.copy(procedures = procedures)
            refreshPlates(icao)
            fetchAirportWeather(icao)
        }
    }

    fun closeAirport() {
        _airport.value = null
    }

    /** Re-read the plate list so ticks track what is actually on disk. */
    fun refreshPlates(icao: String) {
        viewModelScope.launch {
            val plates = container.plates.plates(icao)
            if (_airport.value?.detail?.airport?.icao.equals(icao, ignoreCase = true)) {
                _airport.value = _airport.value?.copy(plates = plates)
            }
        }
    }

    /** Take every plate this airport publishes that isn't already here. */
    fun downloadAirportPlates() {
        val state = _airport.value ?: return
        container.plates.downloadAll(state.detail.airport.icao, state.plates)
    }

    fun cancelPlateDownload() = container.plates.cancel()

    fun dismissPlateDownload() = container.plates.dismiss()

    fun clearPlates() {
        viewModelScope.launch { container.plates.clear() }
    }

    /**
     * Fetch this airport's observation and forecast. Failure is reported in
     * the sheet rather than swallowed: "no weather shown" and "weather
     * couldn't be fetched" mean very different things to a pilot.
     */
    fun fetchAirportWeather(icao: String) {
        viewModelScope.launch {
            _airport.value = _airport.value?.copy(weatherLoading = true, weatherError = null)
            try {
                val metars = container.api.metars(listOf(icao))
                val tafs = container.api.tafs(listOf(icao))
                _airport.value = _airport.value?.copy(
                    metar = metars.firstOrNull { it.icaoId.equals(icao, ignoreCase = true) },
                    taf = tafs.firstOrNull { it.icaoId.equals(icao, ignoreCase = true) },
                    weatherLoading = false,
                    fetchedAtMillis = System.currentTimeMillis(),
                )
            } catch (e: Exception) {
                _airport.value = _airport.value?.copy(
                    weatherLoading = false,
                    weatherError = e.message ?: "couldn't reach ff-api",
                )
            }
        }
    }

    /**
     * Colour the airports in view by their current flight category, and fetch
     * active graphical weather hazards (G-AIRMETs, SIGMETs, CWAs, PIREPs).
     *
     * Explicit, never automatic on pan: it is a network call per viewport,
     * and the map has to be usable with the radio off. The timestamp it
     * records is what the status line ages.
     */
    fun refreshVisibleWeather() {
        val idents = _map.value.airports.take(WEATHER_BATCH).map { it.icao }
        val bbox = lastViewport
        if (idents.isEmpty() && bbox == null) return
        viewModelScope.launch {
            _map.value = _map.value.copy(weatherLoading = true)
            try {
                val metarsDeferred = async {
                    if (idents.isNotEmpty()) {
                        runCatching { container.api.metars(idents) }.getOrDefault(emptyList())
                    } else emptyList()
                }
                val gairmetsDeferred = async {
                    runCatching { container.api.gairmets() }.getOrDefault(emptyList())
                }
                val sigmetsDeferred = async {
                    runCatching { container.api.sigmets() }.getOrDefault(emptyList())
                }
                val cwasDeferred = async {
                    runCatching { container.api.cwas() }.getOrDefault(emptyList())
                }
                val pirepsDeferred = async {
                    if (bbox != null) {
                        runCatching { container.api.pireps(bbox) }.getOrDefault(emptyList())
                    } else emptyList()
                }

                val metars = metarsDeferred.await()
                val gairmets = gairmetsDeferred.await()
                val sigmets = sigmetsDeferred.await()
                val cwas = cwasDeferred.await()
                val pireps = pirepsDeferred.await()

                _map.value = _map.value.copy(
                    flightCategories = metars.mapNotNull { metar ->
                        metar.flightCategory?.let { metar.icaoId.uppercase() to it }
                    }.toMap(),
                    gairmets = gairmets,
                    sigmets = sigmets,
                    cwas = cwas,
                    pireps = pireps,
                    weatherFetchedAtMillis = System.currentTimeMillis(),
                    weatherLoading = false,
                    message = null,
                )
            } catch (e: Exception) {
                _map.value = _map.value.copy(
                    weatherLoading = false,
                    message = "Weather unavailable: ${e.message}",
                )
            }
        }
    }

    // ---- procedures ------------------------------------------------------

    fun openProcedure(id: String) {
        viewModelScope.launch {
            _procedure.value = try {
                withContext(Dispatchers.IO) { core.procedure(id) }
            } catch (e: Exception) {
                _map.value = _map.value.copy(message = "Couldn't open procedure: ${e.message}")
                null
            }
        }
    }

    fun closeProcedure() {
        _procedure.value = null
    }

    // ---- data management -------------------------------------------------

    fun checkForUpdate() = container.cycles.checkForUpdate()
    fun downloadLatestCycle() = container.cycles.downloadLatestCycle()
    fun downloadChart(chart: uniffi.ff_uniffi.Chart) = container.cycles.downloadChart(chart)
    fun cancelChartDownload(chartId: String) = container.cycles.cancelChartDownload(chartId)
    fun downloadChartSet(set: ChartSet) = container.cycles.downloadChartSet(set)
    fun cancelChartSetDownload() = container.cycles.cancelChartSetDownload()
    fun dismissChartSetDownload() = container.cycles.dismissChartSetDownload()

    fun removeChart(chartId: String) {
        viewModelScope.launch { container.cycles.removeChart(chartId) }
    }

    fun pruneOldCycles() {
        viewModelScope.launch {
            val freed = container.cycles.pruneOldCycles()
            _map.value = _map.value.copy(message = "Reclaimed ${formatBytes(freed.toLong())}")
        }
    }

    fun dismissMessage() {
        _map.value = _map.value.copy(message = null)
    }

    /**
     * The credits that ship inside the installed cycle, which the app is
     * required to display (DESIGN.md §11 — the French SIA's Licence
     * Ouverte, and the FAA/NOAA terms). Empty rather than an error when no
     * cycle is installed: there is then nothing to credit.
     */
    suspend fun attributions(): List<uniffi.ff_uniffi.DataSourceCredit> =
        withContext(Dispatchers.IO) {
            runCatching { core.attributions() }.getOrDefault(emptyList())
        }

    fun tileUrlTemplate(chartId: String): String = container.tileServer.tileUrlTemplate(chartId)

    class Factory(private val container: AppContainer) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T =
            FreeflightViewModel(container) as T
    }

    private companion object {
        const val VIEWPORT_DEBOUNCE_MS = 300L
        const val SEARCH_DEBOUNCE_MS = 160L
        const val SEARCH_LIMIT = 20u
        /**
         * How many airports one viewport query returns, most significant
         * first (`query::airports_in_bbox` orders by "has procedures",
         * then landplane airports). A wide viewport over the northeast
         * holds far more than this, so the cap is doing real work.
         *
         * Known wart: within one significance tier the SQL tiebreak is
         * `icao`, so a truncated result is an alphabetical slice rather
         * than a geographic spread. Visible only when zoomed out far
         * enough to exceed the cap, and fixing it needs spatial sampling
         * the bundle has no index for.
         */
        const val AIRPORT_LIMIT = 400u

        /**
         * How many airspace volumes one viewport may draw.
         *
         * Airports were capped from the start; airspace was not, and a
         * viewport panned out over the country selected every volume it
         * touched — each with a boundary polygon attached — until the heap
         * gave out. The query hands back the highest-priority volumes first
         * (Class B/C/D, then the Special Use you may not fly into), so what
         * a cap discards is the wide-area Class E and G the map already
         * draws as context rather than as a boundary to avoid.
         */
        const val AIRSPACE_LIMIT = 600u

        /**
         * The same cap for route crossing detection, set higher.
         *
         * This one is not about what fits on screen: the bbox spans the
         * whole route, and a volume dropped here is a crossing warning not
         * given. The priority order means a long route loses Class E before
         * anything a warning would be about.
         */
        const val ROUTE_AIRSPACE_LIMIT = 2000u

        /**
         * Below this the map draws no airports, so the viewport query is
         * skipped. Matches `ChartMap.AIRPORT_PROCEDURES_ZOOM`, which is
         * where the first tier of markers starts appearing.
         */
        const val MIN_AIRPORT_ZOOM = 5.0
        // aviationweather.gov takes a comma-separated ident list; a whole
        // viewport's worth would be an unreasonable URL and an unreasonable
        // ask of a proxy shared with the web client.
        const val WEATHER_BATCH = 50
    }
}
