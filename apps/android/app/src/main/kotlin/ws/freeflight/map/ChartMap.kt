package ws.freeflight.map

import android.graphics.PointF
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import org.maplibre.android.MapLibre
import org.maplibre.android.camera.CameraUpdateFactory
import org.maplibre.android.geometry.LatLng
import org.maplibre.android.geometry.LatLngBounds
import org.maplibre.android.maps.MapLibreMap
import org.maplibre.android.maps.MapView
import org.maplibre.android.maps.Style
import org.maplibre.android.style.expressions.Expression
import org.maplibre.android.style.layers.CircleLayer
import org.maplibre.android.style.layers.FillLayer
import org.maplibre.android.style.layers.LineLayer
import org.maplibre.android.style.layers.PropertyFactory
import org.maplibre.android.style.layers.RasterLayer
import org.maplibre.android.style.sources.GeoJsonSource
import org.maplibre.android.style.sources.RasterSource
import org.maplibre.android.style.sources.TileSet
import uniffi.ff_uniffi.BoundingBox

/**
 * The map surface, and the only place that knows MapLibre's API.
 *
 * There is no vector basemap and no glyph server. The chart *is* the base
 * map: an FAA sectional already carries its own terrain, airspace and
 * airport labels, rendered by the FAA, and it has to work with the network
 * off (DESIGN.md §8) — so the style is a flat background plus a raster
 * layer of chart tiles from the on-device archive, and the vector overlays
 * on top of it are deliberately label-free geometry. That also means the
 * style needs no `glyphs` URL, which would otherwise be a network
 * dependency on every text label.
 */
/** A chart the map can draw: where its tiles come from, and over what zooms. */
data class ChartLayer(val tileUrlTemplate: String, val minZoom: Int, val maxZoom: Int)

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

    private var pendingChartTemplate: ChartLayer? = null
    private var pendingAirports: String = GeoJson.empty
    private var pendingAirspace: String = GeoJson.empty
    private var pendingProcedure: String = GeoJson.empty

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

        mapLibreMap.setStyle(Style.Builder().fromJson(BASE_STYLE)) { loaded ->
            style = loaded
            installLayers(loaded)
            // Anything the screen asked for before the style finished
            // loading — which is most things on a cold start.
            pendingChartTemplate?.let { applyChart(it) }
            source(AIRPORTS_SOURCE)?.setGeoJson(pendingAirports)
            source(AIRSPACE_SOURCE)?.setGeoJson(pendingAirspace)
            source(PROCEDURE_SOURCE)?.setGeoJson(pendingProcedure)
            emitViewport()
        }

        mapLibreMap.addOnCameraIdleListener { emitViewport() }
        mapLibreMap.addOnMapClickListener { point ->
            val screenPoint = mapLibreMap.projection.toScreenLocation(point)
            onAirportTapped?.invoke(airportAt(mapLibreMap, screenPoint))
            true
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
    fun setChart(chart: ChartLayer?) {
        pendingChartTemplate = chart
        applyChart(chart)
    }

    fun setAirports(geoJson: String) {
        pendingAirports = geoJson
        source(AIRPORTS_SOURCE)?.setGeoJson(geoJson)
    }

    fun setAirspace(geoJson: String) {
        pendingAirspace = geoJson
        source(AIRSPACE_SOURCE)?.setGeoJson(geoJson)
    }

    fun setProcedure(geoJson: String) {
        pendingProcedure = geoJson
        source(PROCEDURE_SOURCE)?.setGeoJson(geoJson)
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

    private fun applyChart(chart: ChartLayer?) {
        val loaded = style ?: return
        loaded.getLayer(CHART_LAYER)?.let { loaded.removeLayer(it) }
        loaded.getSource(CHART_SOURCE)?.let { loaded.removeSource(it) }
        if (chart == null) return

        val tileSet = TileSet("2.1.0", chart.tileUrlTemplate).apply {
            // Straight from the archive's own header. Outside this range a
            // raster source draws nothing, so guessing it wide blanks the
            // chart exactly when the pilot zooms in past the deepest tiles
            // that were rendered; given the true maximum, MapLibre scales
            // those up instead.
            minZoom = chart.minZoom.toFloat()
            maxZoom = chart.maxZoom.toFloat()
        }
        loaded.addSource(RasterSource(CHART_SOURCE, tileSet, 256))
        loaded.addLayerAbove(
            RasterLayer(CHART_LAYER, CHART_SOURCE)
                .withProperties(PropertyFactory.rasterOpacity(1.0f)),
            BACKGROUND_LAYER,
        )
    }

    private fun installLayers(loaded: Style) {
        loaded.addSource(GeoJsonSource(AIRSPACE_SOURCE, pendingAirspace))
        loaded.addSource(GeoJsonSource(PROCEDURE_SOURCE, pendingProcedure))
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

        // Airports last, so they stay tappable over everything else. No
        // labels: the sectional underneath has them, and a text layer would
        // drag in a glyph server this app must work without.
        loaded.addLayer(
            CircleLayer(AIRPORTS_LAYER, AIRPORTS_SOURCE).withProperties(
                PropertyFactory.circleRadius(
                    Expression.switchCase(
                        Expression.get("hasProcedures"), Expression.literal(6.5f),
                        Expression.literal(4.0f),
                    )
                ),
                PropertyFactory.circleColor(flightCategoryColor()),
                PropertyFactory.circleStrokeWidth(1.5f),
                PropertyFactory.circleStrokeColor("#10161C"),
                PropertyFactory.circleOpacity(0.9f),
            )
        )
    }

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
        private const val CHART_SOURCE = "chart"
        private const val CHART_LAYER = "chart-raster"
        private const val AIRSPACE_SOURCE = "airspace"
        private const val AIRSPACE_FILL_LAYER = "airspace-fill"
        private const val AIRSPACE_LINE_LAYER = "airspace-line"
        private const val PROCEDURE_SOURCE = "procedure"
        private const val PROCEDURE_LINE_LAYER = "procedure-missed"
        private const val PROCEDURE_SOLID_LAYER = "procedure-line"
        private const val PROCEDURE_FIX_LAYER = "procedure-fix"
        private const val AIRPORTS_SOURCE = "airports"
        const val AIRPORTS_LAYER = "airports-circle"

        /**
         * No `glyphs` and no `sprite` entries, on purpose — see the class
         * docs. Every layer added at runtime is geometry, never text, so the
         * style has no network dependency of any kind.
         */
        private const val BASE_STYLE = """
            {
              "version": 8,
              "name": "freeflight",
              "sources": {},
              "layers": [
                {
                  "id": "background",
                  "type": "background",
                  "paint": { "background-color": "#0E1116" }
                }
              ]
            }
        """
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
