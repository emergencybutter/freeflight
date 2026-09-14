package ws.freeflight.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.ff_uniffi.Airport
import uniffi.ff_uniffi.AirportDetail
import uniffi.ff_uniffi.Airspace
import uniffi.ff_uniffi.BoundingBox
import uniffi.ff_uniffi.CoreException
import uniffi.ff_uniffi.Procedure
import uniffi.ff_uniffi.ProcedureDetail
import uniffi.ff_uniffi.SearchHit
import ws.freeflight.AppContainer
import ws.freeflight.data.Metar
import ws.freeflight.data.Taf

/** Everything the map screen draws, in one snapshot. */
data class MapUiState(
    val airports: List<Airport> = emptyList(),
    val airspace: List<Airspace> = emptyList(),
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
)

class FreeflightViewModel(private val container: AppContainer) : ViewModel() {

    private val core = container.core

    val cycle = container.cycles.cycle
    val sync = container.cycles.sync
    val charts = container.cycles.charts
    val chartDownloads = container.cycles.chartDownloads
    val settings = container.settings

    private val _map = MutableStateFlow(MapUiState())
    val map: StateFlow<MapUiState> = _map.asStateFlow()

    private val _searchResults = MutableStateFlow<List<SearchHit>>(emptyList())
    val searchResults: StateFlow<List<SearchHit>> = _searchResults.asStateFlow()

    private val _airport = MutableStateFlow<AirportUiState?>(null)
    val airport: StateFlow<AirportUiState?> = _airport.asStateFlow()

    private val _procedure = MutableStateFlow<ProcedureDetail?>(null)
    val procedure: StateFlow<ProcedureDetail?> = _procedure.asStateFlow()

    private var viewportJob: Job? = null
    private var searchJob: Job? = null
    private var lastViewport: BoundingBox? = null
    private var lastZoom: Double = MIN_AIRPORT_ZOOM

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
        viewportJob?.cancel()
        viewportJob = viewModelScope.launch {
            delay(VIEWPORT_DEBOUNCE_MS)
            loadViewport(bbox, zoom)
        }
    }

    private suspend fun loadViewport(bbox: BoundingBox, zoom: Double) {
        // Below this, a nationwide bundle puts thousands of dots on screen
        // and none of them is readable; the chart itself still is.
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
                if (container.settings.showAirspace.value) core.airspaceInBbox(bbox) else emptyList()
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
            fetchAirportWeather(icao)
        }
    }

    fun closeAirport() {
        _airport.value = null
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
     * Colour the airports in view by their current flight category.
     *
     * Explicit, never automatic on pan: it is a network call per viewport,
     * and the map has to be usable with the radio off. The timestamp it
     * records is what the status line ages.
     */
    fun refreshVisibleWeather() {
        val idents = _map.value.airports.take(WEATHER_BATCH).map { it.icao }
        if (idents.isEmpty()) return
        viewModelScope.launch {
            _map.value = _map.value.copy(weatherLoading = true)
            try {
                val metars = container.api.metars(idents)
                _map.value = _map.value.copy(
                    flightCategories = metars.mapNotNull { metar ->
                        metar.flightCategory?.let { metar.icaoId.uppercase() to it }
                    }.toMap(),
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
        const val VIEWPORT_DEBOUNCE_MS = 180L
        const val SEARCH_DEBOUNCE_MS = 160L
        const val SEARCH_LIMIT = 20u
        const val AIRPORT_LIMIT = 400u
        const val MIN_AIRPORT_ZOOM = 6.0
        // aviationweather.gov takes a comma-separated ident list; a whole
        // viewport's worth would be an unreasonable URL and an unreasonable
        // ask of a proxy shared with the web client.
        const val WEATHER_BATCH = 50
    }
}
