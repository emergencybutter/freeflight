package ws.freeflight.data

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import ws.freeflight.BuildConfig

/**
 * The handful of things the pilot configures. Backed by SharedPreferences
 * rather than the cycle database: these must survive a cycle being wiped
 * and reinstalled, and none of them is aeronautical data.
 */
class Settings(context: Context) {
    private val prefs = context.getSharedPreferences("freeflight", Context.MODE_PRIVATE)

    private val _apiBaseUrl = MutableStateFlow(
        prefs.getString(KEY_API_BASE_URL, null) ?: BuildConfig.DEFAULT_API_BASE_URL
    )

    /** Where `ff-api` lives. Everything network the app does is relative to this. */
    val apiBaseUrl: StateFlow<String> = _apiBaseUrl.asStateFlow()

    fun setApiBaseUrl(value: String) {
        val cleaned = value.trim().trimEnd('/')
        prefs.edit().putString(KEY_API_BASE_URL, cleaned).apply()
        _apiBaseUrl.value = cleaned
    }

    private val _showAirspace = MutableStateFlow(prefs.getBoolean(KEY_SHOW_AIRSPACE, true))
    val showAirspace: StateFlow<Boolean> = _showAirspace.asStateFlow()

    fun setShowAirspace(value: Boolean) {
        prefs.edit().putBoolean(KEY_SHOW_AIRSPACE, value).apply()
        _showAirspace.value = value
    }

    private val _showAirports = MutableStateFlow(prefs.getBoolean(KEY_SHOW_AIRPORTS, true))
    val showAirports: StateFlow<Boolean> = _showAirports.asStateFlow()

    fun setShowAirports(value: Boolean) {
        prefs.edit().putBoolean(KEY_SHOW_AIRPORTS, value).apply()
        _showAirports.value = value
    }

    private val _showGairmets = MutableStateFlow(prefs.getBoolean(KEY_SHOW_GAIRMETS, true))
    val showGairmets: StateFlow<Boolean> = _showGairmets.asStateFlow()

    fun setShowGairmets(value: Boolean) {
        prefs.edit().putBoolean(KEY_SHOW_GAIRMETS, value).apply()
        _showGairmets.value = value
    }

    private val _showSigmets = MutableStateFlow(prefs.getBoolean(KEY_SHOW_SIGMETS, true))
    val showSigmets: StateFlow<Boolean> = _showSigmets.asStateFlow()

    fun setShowSigmets(value: Boolean) {
        prefs.edit().putBoolean(KEY_SHOW_SIGMETS, value).apply()
        _showSigmets.value = value
    }

    private val _showCwas = MutableStateFlow(prefs.getBoolean(KEY_SHOW_CWAS, true))
    val showCwas: StateFlow<Boolean> = _showCwas.asStateFlow()

    fun setShowCwas(value: Boolean) {
        prefs.edit().putBoolean(KEY_SHOW_CWAS, value).apply()
        _showCwas.value = value
    }

    private val _showPireps = MutableStateFlow(prefs.getBoolean(KEY_SHOW_PIREPS, true))
    val showPireps: StateFlow<Boolean> = _showPireps.asStateFlow()

    fun setShowPireps(value: Boolean) {
        prefs.edit().putBoolean(KEY_SHOW_PIREPS, value).apply()
        _showPireps.value = value
    }

    private val _showBasemap = MutableStateFlow(prefs.getBoolean(KEY_SHOW_BASEMAP, true))
    val showBasemap: StateFlow<Boolean> = _showBasemap.asStateFlow()

    fun setShowBasemap(value: Boolean) {
        prefs.edit().putBoolean(KEY_SHOW_BASEMAP, value).apply()
        _showBasemap.value = value
    }

    /**
     * Which chart *series* the map draws under everything else, as a
     * `chart_catalog.kind` — or null for no chart.
     *
     * A kind rather than a single `chart_catalog.id`, matching the web
     * client: a nationwide cycle catalogues 181 sheets, which is not a
     * menu, and picking one sheet means the map goes blank the moment a
     * flight crosses onto the next sectional. A kind survives a cycle
     * update too, where an id would not — ids embed the cycle date.
     *
     * Defaults to [ChartKinds.DEFAULT] on a fresh install, so a pilot who
     * downloads a sectional sees it without first finding this setting.
     */
    private val _selectedChartKind =
        MutableStateFlow(prefs.getString(KEY_CHART_KIND, ChartKinds.DEFAULT))
    val selectedChartKind: StateFlow<String?> = _selectedChartKind.asStateFlow()

    fun setSelectedChartKind(value: String?) {
        prefs.edit().putString(KEY_CHART_KIND, value).apply()
        _selectedChartKind.value = value
    }

    private companion object {
        const val KEY_API_BASE_URL = "api_base_url"
        const val KEY_SHOW_AIRSPACE = "show_airspace"
        const val KEY_SHOW_AIRPORTS = "show_airports"
        const val KEY_SHOW_GAIRMETS = "show_gairmets"
        const val KEY_SHOW_SIGMETS = "show_sigmets"
        const val KEY_SHOW_CWAS = "show_cwas"
        const val KEY_SHOW_PIREPS = "show_pireps"
        const val KEY_SHOW_BASEMAP = "show_basemap"
        // Superseded `selected_chart_id`, which stored a single sheet.
        const val KEY_CHART_KIND = "selected_chart_kind"
    }
}

