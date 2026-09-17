package ws.freeflight.map

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.PointF
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import org.maplibre.android.MapLibre
import org.maplibre.android.camera.CameraUpdateFactory
import org.maplibre.android.geometry.LatLng
import org.maplibre.android.geometry.LatLngBounds
import org.maplibre.android.location.LocationComponentActivationOptions
import org.maplibre.android.location.LocationComponentOptions
import org.maplibre.android.location.OnCameraTrackingChangedListener
import org.maplibre.android.location.modes.CameraMode
import org.maplibre.android.location.modes.RenderMode
import org.maplibre.android.maps.MapLibreMap
import org.maplibre.android.maps.MapView
import org.maplibre.android.maps.Style
import org.maplibre.android.style.expressions.Expression
import org.maplibre.android.style.layers.CircleLayer
import org.maplibre.android.style.layers.FillLayer
import org.maplibre.android.style.layers.LineLayer
import org.maplibre.android.style.layers.Property
import org.maplibre.android.style.layers.PropertyFactory
import org.maplibre.android.style.layers.RasterLayer
import org.maplibre.android.style.sources.GeoJsonSource
import org.maplibre.android.style.sources.RasterSource
import org.maplibre.android.style.sources.TileSet
import uniffi.ff_uniffi.BoundingBox

/**
 * The map surface, and the only place that knows MapLibre's API.
 *
 * The basemap uses OpenFreeMap (openfreemap.org) dark vector tiles for global
 * geography, coastlines, and borders context when no FAA chart is loaded, with
 * no API key or rate limits. When an FAA sectional or IFR chart is loaded, it
 * draws as an opaque raster layer above the basemap and below the interactive
 * overlays.
 *
 * To work reliably offline (DESIGN.md §8), the initial style is bundled locally
 * in assets/basemap_style.json and loaded synchronously from memory. If the
 * aircraft has no data connection, remote basemap requests fail silently while
 * the local loopback TileServer chart tiles and GeoJSON overlays render normally.
 */
enum class LocationTrackingMode {
    NONE,
    TRACKING,
    TRACKING_COMPASS,
}

/** A chart the map can draw: where its tiles come from, and over what zooms. */
data class ChartLayer(
    /** `chart_catalog.id`, used to name this chart's source and layer. */
    val chartId: String,
    val tileUrlTemplate: String,
    val minZoom: Int,
    val maxZoom: Int,
)

class MapController {

    private var map: MapLibreMap? = null
    private var style: Style? = null

    /**
     * Kept so camera moves can reason about the surface's pixel height —
     * [fitBounds] has to know how much of the map a bottom sheet is
     * covering, and MapLibre's own API exposes no view dimensions.
     */
    private var surfaceHeightPx: Int = 0

    /** Set by the screen; called whenever the viewport settles. */
    var onViewportChanged: ((BoundingBox, Double) -> Unit)? = null

    /** Called with the ICAO of a tapped airport, or null for a tap on nothing. */
    var onAirportTapped: ((String?) -> Unit)? = null

    /** Called with a tapped weather hazard (PIREP, SIGMET, CWA, AIRMET), or null. */
    var onWeatherHazardTapped: ((ws.freeflight.data.WeatherHazardTap?) -> Unit)? = null

    /** Called when location tracking mode changes (e.g., when the user moves the map). */
    var onLocationTrackingModeChanged: ((LocationTrackingMode) -> Unit)? = null

    var currentTrackingMode: LocationTrackingMode = LocationTrackingMode.NONE
        private set

    private var pendingCharts: List<ChartLayer> = emptyList()

    /** Ids of the chart sources/layers currently installed, so they can
     *  be removed precisely rather than by sweeping the style. */
    private var chartLayerIds: List<String> = emptyList()
    private var pendingAirports: String = GeoJson.empty
    private var pendingAirspace: String = GeoJson.empty
    private var pendingGairmets: String = GeoJson.empty
    private var pendingSigmets: String = GeoJson.empty
    private var pendingCwas: String = GeoJson.empty
    private var pendingPireps: String = GeoJson.empty
    private var pendingProcedure: String = GeoJson.empty
    private var pendingRoute: String = GeoJson.empty
    private var pendingTrack: String = GeoJson.empty
    private var pendingBasemapVisible: Boolean = true
    private var basemapLayerIds: List<String> = emptyList()

    fun attach(mapLibreMap: MapLibreMap, surface: MapView) {
        map = mapLibreMap
        surfaceHeightPx = surface.height
        surface.addOnLayoutChangeListener { _, _, top, _, bottom, _, _, _, _ ->
            surfaceHeightPx = bottom - top
        }
        mapLibreMap.uiSettings.isAttributionEnabled = false
        mapLibreMap.uiSettings.isLogoEnabled = false
        mapLibreMap.uiSettings.isRotateGesturesEnabled = true
        mapLibreMap.uiSettings.isTiltGesturesEnabled = false

        // Center on CONUS overview if starting at unpositioned default (0, 0)
        val target = mapLibreMap.cameraPosition.target
        val isDefaultPosition = mapLibreMap.cameraPosition.zoom <= 1.0 &&
            (target == null || (target.latitude == 0.0 && target.longitude == 0.0))
        if (isDefaultPosition) {
            mapLibreMap.moveCamera(
                CameraUpdateFactory.newLatLngZoom(LatLng(39.8283, -98.5795), 3.8)
            )
        }

        val basemapJson = surface.context.assets.open(BASEMAP_STYLE_ASSET).bufferedReader().use { it.readText() }
        mapLibreMap.setStyle(Style.Builder().fromJson(basemapJson)) { loaded ->
            style = loaded
            basemapLayerIds = loaded.layers.map { it.id }.filter { it != BACKGROUND_LAYER }
            setBasemapVisible(pendingBasemapVisible)
            installLayers(loaded)
            setupLocationComponentIfPermitted(surface.context, loaded)
            // Anything the screen asked for before the style finished
            // loading — which is most things on a cold start.
            applyCharts(pendingCharts)
            source(AIRPORTS_SOURCE)?.setGeoJson(pendingAirports)
            source(AIRSPACE_SOURCE)?.setGeoJson(pendingAirspace)
            source(GAIRMET_SOURCE)?.setGeoJson(pendingGairmets)
            source(SIGMET_SOURCE)?.setGeoJson(pendingSigmets)
            source(CWA_SOURCE)?.setGeoJson(pendingCwas)
            source(PIREP_SOURCE)?.setGeoJson(pendingPireps)
            source(PROCEDURE_SOURCE)?.setGeoJson(pendingProcedure)
            source(ROUTE_SOURCE)?.setGeoJson(pendingRoute)
            source(TRACK_SOURCE)?.setGeoJson(pendingTrack)
            emitViewport()
        }

        mapLibreMap.addOnCameraIdleListener { emitViewport() }
        mapLibreMap.addOnMapClickListener { point ->
            val screenPoint = mapLibreMap.projection.toScreenLocation(point)
            val airport = airportAt(mapLibreMap, screenPoint)
            if (airport != null) {
                onAirportTapped?.invoke(airport)
                onWeatherHazardTapped?.invoke(null)
            } else {
                val hazard = weatherHazardAt(mapLibreMap, screenPoint)
                if (hazard != null) {
                    onAirportTapped?.invoke(null)
                    onWeatherHazardTapped?.invoke(hazard)
                } else {
                    onAirportTapped?.invoke(null)
                    onWeatherHazardTapped?.invoke(null)
                }
            }
            true
        }
    }


    fun setupLocationComponentIfPermitted(context: Context, style: Style? = this.style) {
        val mapLibreMap = map ?: return
        val currentStyle = style ?: return
        val fineGranted = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED
        val coarseGranted = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED

        if (!fineGranted && !coarseGranted) return

        try {
            val locationComponent = mapLibreMap.locationComponent
            if (!locationComponent.isLocationComponentActivated) {
                val options = LocationComponentOptions.builder(context)
                    .pulseEnabled(true)
                    .pulseColor(android.graphics.Color.parseColor("#3B82F6"))
                    .accuracyAlpha(0.15f)
                    .build()
                val activationOptions = LocationComponentActivationOptions.builder(context, currentStyle)
                    .locationComponentOptions(options)
                    .build()
                locationComponent.activateLocationComponent(activationOptions)
            }

            locationComponent.isLocationComponentEnabled = true
            locationComponent.renderMode = RenderMode.COMPASS

            locationComponent.addOnCameraTrackingChangedListener(object : OnCameraTrackingChangedListener {
                override fun onCameraTrackingDismissed() {
                    updateTrackingMode(LocationTrackingMode.NONE)
                }

                override fun onCameraTrackingChanged(currentMode: Int) {
                    val mode = when (currentMode) {
                        CameraMode.TRACKING -> LocationTrackingMode.TRACKING
                        CameraMode.TRACKING_COMPASS, CameraMode.TRACKING_GPS -> LocationTrackingMode.TRACKING_COMPASS
                        else -> LocationTrackingMode.NONE
                    }
                    updateTrackingMode(mode)
                }
            })
        } catch (e: Exception) {
            // Location component setup failure fallback
        }
    }

    fun cycleLocationTrackingMode(context: Context): Boolean {
        val mapLibreMap = map ?: return false
        val fineGranted = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED
        val coarseGranted = ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED

        if (!fineGranted && !coarseGranted) return false

        val locationComponent = mapLibreMap.locationComponent
        if (!locationComponent.isLocationComponentActivated) {
            setupLocationComponentIfPermitted(context)
        }

        if (!locationComponent.isLocationComponentEnabled) {
            locationComponent.isLocationComponentEnabled = true
        }

        val nextMode = when (currentTrackingMode) {
            LocationTrackingMode.NONE -> LocationTrackingMode.TRACKING
            LocationTrackingMode.TRACKING -> LocationTrackingMode.TRACKING_COMPASS
            LocationTrackingMode.TRACKING_COMPASS -> LocationTrackingMode.NONE
        }

        when (nextMode) {
            LocationTrackingMode.NONE -> {
                locationComponent.cameraMode = CameraMode.NONE
            }
            LocationTrackingMode.TRACKING -> {
                locationComponent.cameraMode = CameraMode.TRACKING
                locationComponent.zoomWhileTracking(11.0)
            }
            LocationTrackingMode.TRACKING_COMPASS -> {
                locationComponent.cameraMode = CameraMode.TRACKING_COMPASS
                locationComponent.zoomWhileTracking(11.0)
            }
        }
        updateTrackingMode(nextMode)
        return true
    }

    private fun updateTrackingMode(mode: LocationTrackingMode) {
        if (currentTrackingMode != mode) {
            currentTrackingMode = mode
            onLocationTrackingModeChanged?.invoke(mode)
        }
    }

    fun detach() {
        map = null
        style = null
    }

    /**
     * Which chart archive to draw, or null for none. Recreating the source
     * rather than mutating it is intentional: a raster source's tile URL is
     * fixed at construction, and the tile server's port and the chart id
     * both live in that URL.
     */
    /**
     * Draw these charts as the base layer, replacing whatever was there.
     *
     * A list rather than one chart because the selector picks a *series* —
     * "Sectional" — not a sheet. A flight that crosses from the Seattle
     * sectional onto Great Falls should not need the pilot to notice and
     * switch; every installed chart of the chosen kind is drawn, and
     * MapLibre shows whichever one covers where the map is.
     */
    fun setCharts(charts: List<ChartLayer>) {
        pendingCharts = charts
        applyCharts(charts)
    }

    fun setBasemapVisible(visible: Boolean) {
        pendingBasemapVisible = visible
        val currentStyle = style ?: return
        val visibility = if (visible) Property.VISIBLE else Property.NONE
        for (id in basemapLayerIds) {
            currentStyle.getLayer(id)?.setProperties(PropertyFactory.visibility(visibility))
        }
    }

    fun setAirports(geoJson: String) {
        pendingAirports = geoJson
        source(AIRPORTS_SOURCE)?.setGeoJson(geoJson)
    }

    fun setAirspace(geoJson: String) {
        pendingAirspace = geoJson
        source(AIRSPACE_SOURCE)?.setGeoJson(geoJson)
    }

    fun setGairmets(geoJson: String) {
        pendingGairmets = geoJson
        source(GAIRMET_SOURCE)?.setGeoJson(geoJson)
    }

    fun setSigmets(geoJson: String) {
        pendingSigmets = geoJson
        source(SIGMET_SOURCE)?.setGeoJson(geoJson)
    }

    fun setCwas(geoJson: String) {
        pendingCwas = geoJson
        source(CWA_SOURCE)?.setGeoJson(geoJson)
    }

    fun setPireps(geoJson: String) {
        pendingPireps = geoJson
        source(PIREP_SOURCE)?.setGeoJson(geoJson)
    }


    fun setProcedure(geoJson: String) {
        pendingProcedure = geoJson
        source(PROCEDURE_SOURCE)?.setGeoJson(geoJson)
    }

    fun setRoute(geoJson: String) {
        pendingRoute = geoJson
        source(ROUTE_SOURCE)?.setGeoJson(geoJson)
    }

    fun setTrack(geoJson: String) {
        pendingTrack = geoJson
        source(TRACK_SOURCE)?.setGeoJson(geoJson)
    }

    fun flyTo(lat: Double, lon: Double, zoom: Double = 11.0) {
        map?.animateCamera(CameraUpdateFactory.newLatLngZoom(LatLng(lat, lon), zoom))
    }

    /**
     * Frame a geographic extent.
     *
     * `sheetCoversBottomHalf` exists because the thing being framed — a
     * procedure — is shown alongside a bottom sheet that hides the lower
     * half of the map. Centring in the full viewport would put the path
     * behind the sheet, so the padding pushes it up into what is actually
     * visible. A degenerate extent (a procedure with one resolvable fix, or
     * none) is refused rather than zoomed into infinity.
     */
    fun fitBounds(bbox: BoundingBox, sheetCoversBottomHalf: Boolean = false) {
        if (bbox.minLat >= bbox.maxLat || bbox.minLon >= bbox.maxLon) return
        val bounds = LatLngBounds.Builder()
            .include(LatLng(bbox.minLat, bbox.minLon))
            .include(LatLng(bbox.maxLat, bbox.maxLon))
            .build()
        val edge = 96
        val bottom = if (sheetCoversBottomHalf) {
            (surfaceHeightPx * 0.55).toInt().coerceAtLeast(edge)
        } else {
            edge
        }
        map?.animateCamera(
            CameraUpdateFactory.newLatLngBounds(bounds, edge, edge, edge, bottom)
        )
    }

    private fun applyCharts(charts: List<ChartLayer>) {
        val loaded = style ?: return

        // Remove what was there first. Tracked by id rather than swept out
        // of `loaded.layers`, so this can never take out the basemap or an
        // overlay that happens to sit nearby.
        for (id in chartLayerIds) {
            loaded.getLayer(id)?.let { loaded.removeLayer(it) }
            loaded.getSource(id)?.let { loaded.removeSource(it) }
        }
        chartLayerIds = charts.map { chartLayerId(it.chartId) }

        for (chart in charts) {
            val id = chartLayerId(chart.chartId)
            val tileSet = TileSet("2.1.0", chart.tileUrlTemplate).apply {
                // Straight from the archive's own header. Outside this
                // range a raster source draws nothing, so guessing it wide
                // blanks the chart exactly when the pilot zooms in past the
                // deepest tiles that were rendered; given the true maximum,
                // MapLibre scales those up instead.
                minZoom = chart.minZoom.toFloat()
                maxZoom = chart.maxZoom.toFloat()
            }
            loaded.addSource(RasterSource(id, tileSet, 256))
            val chartLayer = RasterLayer(id, id)
                .withProperties(PropertyFactory.rasterOpacity(1.0f))
            // Under the overlays, which have to stay readable on top of it.
            val airspaceLayer = loaded.getLayer(AIRSPACE_FILL_LAYER)
            if (airspaceLayer != null) {
                loaded.addLayerBelow(chartLayer, AIRSPACE_FILL_LAYER)
            } else {
                loaded.addLayer(chartLayer)
            }
        }
    }

    /** Source and layer share one id per chart; both are ours to remove. */
    private fun chartLayerId(chartId: String) = "$CHART_LAYER_PREFIX$chartId"

    private fun installLayers(loaded: Style) {
        loaded.addSource(GeoJsonSource(AIRSPACE_SOURCE, pendingAirspace))
        loaded.addSource(GeoJsonSource(GAIRMET_SOURCE, pendingGairmets))
        loaded.addSource(GeoJsonSource(SIGMET_SOURCE, pendingSigmets))
        loaded.addSource(GeoJsonSource(CWA_SOURCE, pendingCwas))
        loaded.addSource(GeoJsonSource(PROCEDURE_SOURCE, pendingProcedure))
        loaded.addSource(GeoJsonSource(ROUTE_SOURCE, pendingRoute))
        loaded.addSource(GeoJsonSource(TRACK_SOURCE, pendingTrack))
        loaded.addSource(GeoJsonSource(PIREP_SOURCE, pendingPireps))
        loaded.addSource(GeoJsonSource(AIRPORTS_SOURCE, pendingAirports))

        // Class B/C/D and Special Use, tinted by class. Kept translucent:
        // the sectional underneath already draws these boundaries, and the
        // vector copy is here to be *tapped*, not to repaint the chart.
        loaded.addLayer(
            FillLayer(AIRSPACE_FILL_LAYER, AIRSPACE_SOURCE).withProperties(
                PropertyFactory.fillColor(airspaceColor()),
                PropertyFactory.fillOpacity(0.07f),
            )
        )
        loaded.addLayer(
            LineLayer(AIRSPACE_LINE_LAYER, AIRSPACE_SOURCE).withProperties(
                PropertyFactory.lineColor(airspaceColor()),
                PropertyFactory.lineWidth(1.4f),
                PropertyFactory.lineOpacity(0.75f),
            )
        )

        // Graphical AIRMETs (Sierra, Tango, Zulu)
        loaded.addLayer(
            FillLayer(GAIRMET_FILL_LAYER, GAIRMET_SOURCE).withProperties(
                PropertyFactory.fillColor(gairmetColor()),
                PropertyFactory.fillOpacity(0.15f),
            ).withFilter(Expression.eq(Expression.geometryType(), Expression.literal("Polygon")))
        )
        loaded.addLayer(
            LineLayer(GAIRMET_LINE_LAYER, GAIRMET_SOURCE).withProperties(
                PropertyFactory.lineColor(gairmetColor()),
                PropertyFactory.lineWidth(1.6f),
                PropertyFactory.lineDasharray(arrayOf(3.0f, 2.0f)),
            )
        )

        // SIGMETs & Convective SIGMETs (High hazard, red)
        loaded.addLayer(
            FillLayer(SIGMET_FILL_LAYER, SIGMET_SOURCE).withProperties(
                PropertyFactory.fillColor("#E5484D"),
                PropertyFactory.fillOpacity(0.20f),
            )
        )
        loaded.addLayer(
            LineLayer(SIGMET_LINE_LAYER, SIGMET_SOURCE).withProperties(
                PropertyFactory.lineColor("#E5484D"),
                PropertyFactory.lineWidth(2.2f),
            )
        )

        // Center Weather Advisories (ARTCC short-fuse warnings, orange)
        loaded.addLayer(
            FillLayer(CWA_FILL_LAYER, CWA_SOURCE).withProperties(
                PropertyFactory.fillColor("#FF8C42"),
                PropertyFactory.fillOpacity(0.15f),
            )
        )
        loaded.addLayer(
            LineLayer(CWA_LINE_LAYER, CWA_SOURCE).withProperties(
                PropertyFactory.lineColor("#FF8C42"),
                PropertyFactory.lineWidth(1.6f),
                PropertyFactory.lineDasharray(arrayOf(2.0f, 2.0f)),
            )
        )


        loaded.addLayer(
            LineLayer(PROCEDURE_LINE_LAYER, PROCEDURE_SOURCE).withProperties(
                PropertyFactory.lineColor("#FFB300"),
                PropertyFactory.lineWidth(3.0f),
                PropertyFactory.lineJoin("round"),
                PropertyFactory.lineCap("round"),
                // A missed approach is drawn dashed: it is a contingency
                // path, not the one being flown.
                PropertyFactory.lineDasharray(arrayOf(2.0f, 2.0f)),
            ).withFilter(Expression.eq(Expression.get("missed"), Expression.literal(true)))
        )
        loaded.addLayer(
            LineLayer(PROCEDURE_SOLID_LAYER, PROCEDURE_SOURCE).withProperties(
                PropertyFactory.lineColor("#FFB300"),
                PropertyFactory.lineWidth(3.0f),
                PropertyFactory.lineJoin("round"),
                PropertyFactory.lineCap("round"),
            ).withFilter(Expression.neq(Expression.get("missed"), Expression.literal(true)))
        )
        loaded.addLayer(
            CircleLayer(PROCEDURE_FIX_LAYER, PROCEDURE_SOURCE).withProperties(
                PropertyFactory.circleRadius(4.0f),
                PropertyFactory.circleColor("#FFE082"),
                PropertyFactory.circleStrokeColor("#4E342E"),
                PropertyFactory.circleStrokeWidth(1.5f),
            ).withFilter(Expression.eq(Expression.geometryType(), Expression.literal("Point")))
        )

        // Route line overlay (ForeFlight-style magenta)
        loaded.addLayer(
            LineLayer(ROUTE_LINE_LAYER, ROUTE_SOURCE).withProperties(
                PropertyFactory.lineColor("#E91E63"),
                PropertyFactory.lineWidth(3.5f),
                PropertyFactory.lineJoin("round"),
                PropertyFactory.lineCap("round"),
            ).withFilter(Expression.eq(Expression.geometryType(), Expression.literal("LineString")))
        )
        loaded.addLayer(
            CircleLayer(ROUTE_FIX_LAYER, ROUTE_SOURCE).withProperties(
                PropertyFactory.circleRadius(5.5f),
                PropertyFactory.circleColor("#E91E63"),
                PropertyFactory.circleStrokeColor("#FFFFFF"),
                PropertyFactory.circleStrokeWidth(1.5f),
            ).withFilter(Expression.eq(Expression.geometryType(), Expression.literal("Point")))
        )

        // Recorded flight track (bright cyan)
        loaded.addLayer(
            LineLayer(TRACK_LINE_LAYER, TRACK_SOURCE).withProperties(
                PropertyFactory.lineColor("#00E5FF"),
                PropertyFactory.lineWidth(3.5f),
                PropertyFactory.lineJoin("round"),
                PropertyFactory.lineCap("round"),
            ).withFilter(Expression.eq(Expression.geometryType(), Expression.literal("LineString")))
        )

        // PIREPs: circle point markers colored by severity
        loaded.addLayer(
            CircleLayer(PIREP_LAYER, PIREP_SOURCE).withProperties(
                PropertyFactory.circleRadius(
                    Expression.switchCase(
                        Expression.eq(Expression.get("urgent"), Expression.literal(true)),
                        Expression.literal(7.0f),
                        Expression.literal(4.5f),
                    )
                ),
                PropertyFactory.circleColor(pirepSeverityColor()),
                PropertyFactory.circleStrokeWidth(1.2f),
                PropertyFactory.circleStrokeColor("#10161C"),
                PropertyFactory.circleOpacity(0.95f),
            ).withFilter(Expression.eq(Expression.geometryType(), Expression.literal("Point")))
        )

        // Airports last, so they stay tappable over everything else. No
        // labels: the sectional underneath has them, and a text layer would
        // drag in a glyph server this app must work without.
        loaded.addLayer(
            CircleLayer(AIRPORTS_LAYER, AIRPORTS_SOURCE).withProperties(
                PropertyFactory.circleRadius(airportRadius()),
                PropertyFactory.circleColor(flightCategoryColor()),
                PropertyFactory.circleStrokeWidth(1.5f),
                PropertyFactory.circleStrokeColor("#10161C"),
                PropertyFactory.circleOpacity(0.9f),
            ).withFilter(airportDisplayFilter())
        )
    }

    /**
     * Which airports are worth drawing at the current zoom.
     *
     * Previously this was a cliff: nothing below zoom 6, then every
     * airport the query returned at once. One notch of zoom took the map
     * from empty to a few hundred identical dots, neither of which is a
     * useful picture.
     *
     * So it graduates instead, on the same idea the web client uses
     * (`AIRPORT_MIN_ZOOM` / `AIRPORT_NO_WEATHER_MIN_ZOOM` in
     * `apps/web/src/MapView.tsx`): en-route zooms show only the airports
     * you would actually divert to, and everything else appears as you
     * close in on somewhere specific.
     *
     * Web's middle tier keys on having a current METAR. That would be the
     * wrong signal here — this client is the one that has to work with no
     * network, and gating on live weather would empty the tier in exactly
     * the situation it exists for. `hasProcedures` comes out of the
     * bundle, so it means the same thing in the air as on the ground.
     *
     * A filter expression rather than a re-query: MapLibre re-evaluates it
     * per frame, so zooming is smooth and nothing refetches.
     */
    private fun airportDisplayFilter(): Expression = Expression.any(
        Expression.gte(Expression.zoom(), Expression.literal(AIRPORT_ALL_ZOOM)),
        Expression.all(
            Expression.gte(Expression.zoom(), Expression.literal(AIRPORT_PROCEDURES_ZOOM)),
            Expression.get("hasProcedures"),
        ),
    )

    /**
     * Dots grow as you close in, so a marker reads as a place rather than
     * as speckle on the chart. Instrument airports stay the larger of the
     * two at every zoom — the distinction a pilot is scanning for.
     */
    private fun airportRadius(): Expression = Expression.interpolate(
        Expression.linear(),
        Expression.zoom(),
        Expression.stop(AIRPORT_PROCEDURES_ZOOM, withProcedures(3.5f, 2.5f)),
        Expression.stop(AIRPORT_ALL_ZOOM, withProcedures(5.5f, 3.5f)),
        Expression.stop(11.0, withProcedures(8.0f, 5.5f)),
    )

    private fun withProcedures(ifTrue: Float, ifFalse: Float): Expression =
        Expression.switchCase(
            Expression.get("hasProcedures"), Expression.literal(ifTrue),
            Expression.literal(ifFalse),
        )

    /**
     * The standard flight-category colours pilots already read on every
     * other briefing product — and a distinct grey for "no observation",
     * which must never be mistaken for VFR.
     */
    private fun flightCategoryColor(): Expression = Expression.match(
        Expression.coalesce(Expression.get("flightCategory"), Expression.literal("UNKNOWN")),
        Expression.literal("#9E9E9E"),
        Expression.stop("VFR", Expression.literal("#4CAF50")),
        Expression.stop("MVFR", Expression.literal("#2196F3")),
        Expression.stop("IFR", Expression.literal("#F44336")),
        Expression.stop("LIFR", Expression.literal("#E040FB")),
    )

    private fun airspaceColor(): Expression = Expression.match(
        Expression.coalesce(Expression.get("class"), Expression.literal("OTHER")),
        Expression.literal("#FF7043"),
        Expression.stop("B", Expression.literal("#4FC3F7")),
        Expression.stop("C", Expression.literal("#BA68C8")),
        Expression.stop("D", Expression.literal("#4FC3F7")),
    )

    private fun gairmetColor(): Expression = Expression.match(
        Expression.coalesce(Expression.get("hazard"), Expression.literal("TURB")),
        Expression.literal("#FFB020"),
        Expression.stop("TURB", Expression.literal("#FFB020")),
        Expression.stop("ICE", Expression.literal("#4FC3F7")),
        Expression.stop("MT_OBSC", Expression.literal("#8A8A8A")),
        Expression.stop("IFR", Expression.literal("#9B6BD6")),
        Expression.stop("FZLVL", Expression.literal("#7FD0FF")),
        Expression.stop("SFC_WND", Expression.literal("#E0C341")),
    )

    private fun pirepSeverityColor(): Expression = Expression.match(
        Expression.coalesce(Expression.get("severity"), Expression.literal("NONE")),
        Expression.literal("#7FA8D9"),
        Expression.stop("SEVERE", Expression.literal("#E5484D")),
        Expression.stop("MODERATE", Expression.literal("#E0973F")),
        Expression.stop("LIGHT", Expression.literal("#E0C341")),
        Expression.stop("NONE", Expression.literal("#7FA8D9")),
    )

    private fun airportAt(mapLibreMap: MapLibreMap, point: PointF): String? {
        // A generous touch box: a 4px circle is far smaller than a fingertip.
        val slop = 24f
        val box = android.graphics.RectF(
            point.x - slop, point.y - slop, point.x + slop, point.y + slop
        )
        return mapLibreMap.queryRenderedFeatures(box, AIRPORTS_LAYER)
            .firstOrNull()
            ?.getStringProperty("icao")
    }

    private fun weatherHazardAt(mapLibreMap: MapLibreMap, point: PointF): ws.freeflight.data.WeatherHazardTap? {
        val slop = 24f
        val box = android.graphics.RectF(
            point.x - slop, point.y - slop, point.x + slop, point.y + slop
        )
        // 1. PIREPs (point markers)
        val pirepFeature = mapLibreMap.queryRenderedFeatures(box, PIREP_LAYER).firstOrNull()
        if (pirepFeature != null) {
            return ws.freeflight.data.WeatherHazardTap.PirepTap(
                summary = pirepFeature.getStringProperty("summary").orEmpty(),
                rawOb = pirepFeature.getStringProperty("rawOb").orEmpty(),
                severity = pirepFeature.getStringProperty("severity").orEmpty(),
                acType = pirepFeature.getStringProperty("acType"),
                fltLvl = pirepFeature.getNumberProperty("fltLvl")?.toInt(),
                obsTime = pirepFeature.getNumberProperty("obsTime")?.toLong() ?: 0L,
            )
        }
        // 2. CWAs (polygons)
        val cwaFeature = mapLibreMap.queryRenderedFeatures(point, CWA_FILL_LAYER).firstOrNull()
        if (cwaFeature != null) {
            return ws.freeflight.data.WeatherHazardTap.CwaTap(
                hazard = cwaFeature.getStringProperty("hazard").orEmpty(),
                cwsu = cwaFeature.getStringProperty("cwsu").orEmpty(),
                name = cwaFeature.getStringProperty("name").orEmpty(),
                seriesId = cwaFeature.getStringProperty("seriesId").orEmpty(),
                base = cwaFeature.getNumberProperty("base")?.toInt(),
                top = cwaFeature.getNumberProperty("top")?.toInt(),
                rawText = cwaFeature.getStringProperty("rawText").orEmpty(),
            )
        }
        // 3. SIGMETs (polygons)
        val sigmetFeature = mapLibreMap.queryRenderedFeatures(point, SIGMET_FILL_LAYER).firstOrNull()
        if (sigmetFeature != null) {
            return ws.freeflight.data.WeatherHazardTap.SigmetTap(
                hazard = sigmetFeature.getStringProperty("hazard").orEmpty(),
                seriesId = sigmetFeature.getStringProperty("seriesId").orEmpty(),
                icaoId = sigmetFeature.getStringProperty("icaoId").orEmpty(),
                alphaChar = sigmetFeature.getStringProperty("alphaChar").orEmpty(),
                altitudeLow1 = sigmetFeature.getNumberProperty("altitudeLow1")?.toInt(),
                altitudeHi1 = sigmetFeature.getNumberProperty("altitudeHi1")?.toInt(),
                rawAirSigmet = sigmetFeature.getStringProperty("rawAirSigmet").orEmpty(),
            )
        }
        // 4. G-AIRMETs (polygons)
        val gairmetFeature = mapLibreMap.queryRenderedFeatures(point, GAIRMET_FILL_LAYER).firstOrNull()
        if (gairmetFeature != null) {
            return ws.freeflight.data.WeatherHazardTap.AirmetTap(
                hazard = gairmetFeature.getStringProperty("hazard").orEmpty(),
                tag = gairmetFeature.getStringProperty("tag").orEmpty(),
                severity = gairmetFeature.getStringProperty("severity"),
                base = gairmetFeature.getStringProperty("base"),
                top = gairmetFeature.getStringProperty("top"),
                fzlbase = gairmetFeature.getStringProperty("fzlbase"),
                fzltop = gairmetFeature.getStringProperty("fzltop"),
                validTime = gairmetFeature.getStringProperty("validTime").orEmpty(),
                product = gairmetFeature.getStringProperty("product").orEmpty(),
            )
        }
        return null
    }

    private fun source(id: String): GeoJsonSource? = style?.getSourceAs(id)

    private fun emitViewport() {
        val mapLibreMap = map ?: return
        val region = mapLibreMap.projection.visibleRegion.latLngBounds
        onViewportChanged?.invoke(
            BoundingBox(
                minLat = region.latitudeSouth,
                minLon = region.longitudeWest,
                maxLat = region.latitudeNorth,
                maxLon = region.longitudeEast,
            ),
            mapLibreMap.cameraPosition.zoom,
        )
    }

    companion object {
        private const val BACKGROUND_LAYER = "background"
        private const val BASEMAP_SOURCE = "basemap"
        private const val BASEMAP_LAYER = "basemap-tiles"
        /** One source and layer per drawn chart, named `chart-<catalog id>`. */
        private const val CHART_LAYER_PREFIX = "chart-"
        private const val AIRSPACE_SOURCE = "airspace"
        private const val AIRSPACE_FILL_LAYER = "airspace-fill"
        private const val AIRSPACE_LINE_LAYER = "airspace-line"
        private const val GAIRMET_SOURCE = "gairmets"
        private const val GAIRMET_FILL_LAYER = "gairmet-fill"
        private const val GAIRMET_LINE_LAYER = "gairmet-line"
        private const val SIGMET_SOURCE = "sigmets"
        private const val SIGMET_FILL_LAYER = "sigmet-fill"
        private const val SIGMET_LINE_LAYER = "sigmet-line"
        private const val CWA_SOURCE = "cwas"
        private const val CWA_FILL_LAYER = "cwa-fill"
        private const val CWA_LINE_LAYER = "cwa-line"
        private const val PROCEDURE_SOURCE = "procedure"
        private const val PROCEDURE_LINE_LAYER = "procedure-missed"
        private const val PROCEDURE_SOLID_LAYER = "procedure-line"
        private const val PROCEDURE_FIX_LAYER = "procedure-fix"
        private const val ROUTE_SOURCE = "route"
        private const val ROUTE_LINE_LAYER = "route-line"
        private const val ROUTE_FIX_LAYER = "route-fix"
        private const val TRACK_SOURCE = "track"
        private const val TRACK_LINE_LAYER = "track-line"
        private const val PIREP_SOURCE = "pireps"
        private const val PIREP_LAYER = "pirep-circle"
        /**
         * Below this, no airport markers at all — the chart still reads.
         *
         * Deliberately this client's own number rather than a shared one.
         * Web starts at 7 because it streams from `ff-api` with no
         * per-view cap, so a lower floor means a continent-sized query;
         * this client reads a local bundle behind AIRPORT_LIMIT, so a wide
         * view is already bounded. `ff-core`'s vocabulary module records
         * why the two differ.
         */
        const val AIRPORT_PROCEDURES_ZOOM = 5.0

        /** At and above this, every airport in the viewport, not just
         *  the ones with instrument procedures. */
        const val AIRPORT_ALL_ZOOM = 8.0

        private const val AIRPORTS_SOURCE = "airports"
        const val AIRPORTS_LAYER = "airports-circle"


        private const val BASEMAP_STYLE_ASSET = "basemap_style.json"
    }
}

/**
 * Hosts a [MapView] and pumps it the Android lifecycle callbacks MapLibre
 * requires — the SDK holds a GL surface and leaks it if `onStop`/`onDestroy`
 * never arrive.
 */
@Composable
fun ChartMap(controller: MapController, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val mapView = remember {
        MapLibre.getInstance(context)
        // Tell MapLibre it is always connected.
        //
        // Its renderer suspends *all* tile requests when Android reports no
        // network — sensible for a map whose tiles come from the internet,
        // and exactly wrong here: every tile URL this app uses points at
        // the in-process TileServer on loopback, reading an archive already
        // on the device. Left to the system state, the chart silently stops
        // loading new area the moment the aircraft is out of signal, which
        // is the one moment the whole offline design exists for (DESIGN.md
        // §8). Verified: without this, panning in airplane mode issues no
        // tile requests at all.
        MapLibre.setConnected(true)
        MapView(context).also { view ->
            view.onCreate(null)
            view.getMapAsync { controller.attach(it, view) }
        }
    }

    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_START -> mapView.onStart()
                Lifecycle.Event.ON_RESUME -> mapView.onResume()
                Lifecycle.Event.ON_PAUSE -> mapView.onPause()
                Lifecycle.Event.ON_STOP -> mapView.onStop()
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            controller.detach()
            mapView.onDestroy()
        }
    }

    AndroidView(factory = { mapView }, modifier = modifier)
}
