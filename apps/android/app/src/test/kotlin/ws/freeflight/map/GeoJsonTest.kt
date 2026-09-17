package ws.freeflight.map

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.ff_uniffi.Airport
import uniffi.ff_uniffi.Airspace

/**
 * The collection is written out as text a feature at a time rather than
 * built as one JSON tree, so these check the seams that change introduces:
 * the commas between features, the escaping of real names, and a spliced
 * raw geometry.
 */
class GeoJsonTest {

    private val json = Json

    private fun features(geoJson: String): JsonArray =
        json.parseToJsonElement(geoJson).jsonObject.getValue("features").jsonArray

    private fun airspace(
        id: String,
        name: String,
        klass: String,
        boundary: String,
    ) = Airspace(
        id = id,
        name = name,
        `class` = klass,
        floor = "SFC",
        ceiling = "MSL:10000",
        boundaryGeojson = boundary,
    )

    private val polygon = """{"type":"Polygon","coordinates":[[[-122.0,47.0],[-122.0,48.0],[-121.0,47.0],[-122.0,47.0]]]}"""

    @Test
    fun `an empty collection is still a FeatureCollection`() {
        val parsed = json.parseToJsonElement(GeoJson.empty).jsonObject
        assertEquals("FeatureCollection", parsed.getValue("type").jsonPrimitive.content)
        assertEquals(0, parsed.getValue("features").jsonArray.size)
    }

    @Test
    fun `several features are separated, not run together`() {
        val volumes = (1..3).map { airspace("V$it", "VOLUME $it", "B", polygon) }
        val parsed = features(GeoJson.airspace(volumes))

        assertEquals(3, parsed.size)
        assertEquals(
            listOf("V1", "V2", "V3"),
            parsed.map { it.jsonObject.getValue("properties").jsonObject.getValue("id").jsonPrimitive.content },
        )
    }

    @Test
    fun `a boundary is spliced through unchanged`() {
        val parsed = features(GeoJson.airspace(listOf(airspace("V1", "SEATTLE", "B", polygon))))
        val geometry = parsed.single().jsonObject.getValue("geometry")

        assertEquals(json.parseToJsonElement(polygon), geometry)
    }

    @Test
    fun `a malformed boundary drops its volume, not the collection`() {
        val volumes = listOf(
            airspace("GOOD1", "FIRST", "B", polygon),
            airspace("BAD", "BROKEN", "B", "{not json"),
            airspace("GOOD2", "SECOND", "C", polygon),
        )
        val parsed = features(GeoJson.airspace(volumes))

        assertEquals(
            listOf("GOOD1", "GOOD2"),
            parsed.map { it.jsonObject.getValue("properties").jsonObject.getValue("id").jsonPrimitive.content },
        )
    }

    @Test
    fun `names carrying quotes and non-ASCII survive the round trip`() {
        // The reason values go through the encoder rather than into the
        // string by hand: a US-wide bundle contains all of these.
        val awkward = """O'Hare "Intl" — LE HAVRE/OCTEVILLE \ é"""
        val parsed = features(
            GeoJson.airports(
                listOf(
                    Airport(
                        icao = "KORD",
                        faaId = null,
                        iata = null,
                        name = awkward,
                        lat = 41.98,
                        lon = -87.9,
                        elevationFt = 672,
                        airportType = "Airport",
                        hasProcedures = true,
                        towered = true,
                    )
                ),
                flightCategories = mapOf("KORD" to "VFR"),
            )
        )

        val properties = parsed.single().jsonObject.getValue("properties").jsonObject
        assertEquals(awkward, properties.getValue("name").jsonPrimitive.content)
        assertEquals("VFR", properties.getValue("flightCategory").jsonPrimitive.content)
    }

    @Test
    fun `a point feature keeps its coordinates in lon-lat order`() {
        val parsed = features(
            GeoJson.airports(
                listOf(
                    Airport(
                        icao = "KSEA",
                        faaId = null,
                        iata = null,
                        name = "SEATTLE TACOMA INTL",
                        lat = 47.45,
                        lon = -122.31,
                        elevationFt = 433,
                        airportType = "Airport",
                        hasProcedures = true,
                        towered = true,
                    )
                ),
                flightCategories = emptyMap(),
            )
        )

        val geometry = parsed.single().jsonObject.getValue("geometry").jsonObject
        assertEquals("Point", geometry.getValue("type").jsonPrimitive.content)
        val coordinates = geometry.getValue("coordinates").jsonArray
        assertEquals(-122.31, coordinates[0].jsonPrimitive.content.toDouble(), 1e-9)
        assertEquals(47.45, coordinates[1].jsonPrimitive.content.toDouble(), 1e-9)
    }

    @Test
    fun `tower status reaches the map layer as a property`() {
        // The map colours an unobserved field by this, so it has to be on
        // every airport feature rather than only the towered ones.
        fun airport(icao: String, towered: Boolean) = Airport(
            icao = icao,
            faaId = null,
            iata = null,
            name = icao,
            lat = 47.0,
            lon = -122.0,
            elevationFt = 100,
            airportType = "Airport",
            hasProcedures = false,
            towered = towered,
        )

        val parsed = features(
            GeoJson.airports(
                listOf(airport("KSEA", towered = true), airport("S43", towered = false)),
                flightCategories = emptyMap(),
            )
        )

        assertEquals(
            listOf(true, false),
            parsed.map { it.jsonObject.getValue("properties").jsonObject.getValue("towered").jsonPrimitive.content.toBoolean() },
        )
    }

    @Test
    fun `every feature is shaped the way MapLibre expects`() {
        val parsed = features(GeoJson.airspace(listOf(airspace("V1", "SEATTLE", "B", polygon))))
        val feature = parsed.single().jsonObject

        assertEquals("Feature", feature.getValue("type").jsonPrimitive.content)
        assertTrue(feature.getValue("geometry") is JsonObject)
        assertTrue(feature.getValue("properties") is JsonObject)
    }
}
