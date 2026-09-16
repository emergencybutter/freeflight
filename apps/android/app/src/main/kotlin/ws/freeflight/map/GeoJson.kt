package ws.freeflight.map

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import uniffi.ff_uniffi.Airport
import uniffi.ff_uniffi.Airspace
import uniffi.ff_uniffi.ProcedureDetail
import ws.freeflight.data.PlannedWaypoint
import ws.freeflight.data.severity
import ws.freeflight.data.summary

/**
 * Builds the GeoJSON the map's vector overlays are fed.
 *
 * Assembled through the JSON model rather than by string concatenation
 * because airport and airspace names are real data — "O'Hare", `"BIG BEAR
 * CITY"`, names with quotes and non-ASCII — and a hand-built string gets
 * that wrong in exactly the cases a US-wide bundle contains.
 */
object GeoJson {

    private val json = Json

    fun airports(airports: List<Airport>, flightCategories: Map<String, String>): String =
        featureCollection(
            airports.map { airport ->
                feature(
                    geometry = point(airport.lon, airport.lat),
                    properties = buildJsonObject {
                        put("icao", airport.icao)
                        put("name", airport.name)
                        put("hasProcedures", airport.hasProcedures)
                        put("type", airport.airportType)
                        flightCategories[airport.icao]?.let { put("flightCategory", it) }
                    },
                )
            }
        )

    fun airspace(volumes: List<Airspace>): String =
        featureCollection(
            volumes.mapNotNull { volume ->
                val geometry = runCatching {
                    json.parseToJsonElement(volume.boundaryGeojson) as? JsonObject
                }.getOrNull() ?: return@mapNotNull null
                feature(
                    geometry = geometry,
                    properties = buildJsonObject {
                        put("id", volume.id)
                        put("name", volume.name)
                        put("class", volume.`class`)
                        put("floor", volume.floor)
                        put("ceiling", volume.ceiling)
                    },
                )
            }
        )

    fun procedure(detail: ProcedureDetail): String {
        val features = mutableListOf<JsonObject>()
        for (transition in detail.transitions) {
            val coordinates = transition.legs.mapNotNull { leg ->
                val lat = leg.lat ?: return@mapNotNull null
                val lon = leg.lon ?: return@mapNotNull null
                lon to lat
            }
            if (coordinates.size >= 2) {
                features += feature(
                    geometry = buildJsonObject {
                        put("type", "LineString")
                        put("coordinates", buildJsonArray {
                            coordinates.forEach { (lon, lat) -> add(coordinate(lon, lat)) }
                        })
                    },
                    properties = buildJsonObject {
                        put("transition", transition.ident)
                        put("missed", transition.kind.equals("MISSED", ignoreCase = true))
                    },
                )
            }
            for (leg in transition.legs) {
                val lat = leg.lat ?: continue
                val lon = leg.lon ?: continue
                features += feature(
                    geometry = point(lon, lat),
                    properties = buildJsonObject {
                        put("fix", leg.fixIdent ?: "")
                        put("altitude", leg.altitudeConstraint ?: "")
                        put("missed", transition.kind.equals("MISSED", ignoreCase = true))
                    },
                )
            }
        }
        return featureCollection(features)
    }

    fun route(waypoints: List<PlannedWaypoint>): String {
        if (waypoints.isEmpty()) return empty
        val features = mutableListOf<JsonObject>()

        if (waypoints.size >= 2) {
            features += feature(
                geometry = buildJsonObject {
                    put("type", "LineString")
                    put("coordinates", buildJsonArray {
                        waypoints.forEach { add(coordinate(it.lon, it.lat)) }
                    })
                },
                properties = buildJsonObject {},
            )
        }

        for (wp in waypoints) {
            features += feature(
                geometry = point(wp.lon, wp.lat),
                properties = buildJsonObject {
                    put("ident", wp.ident)
                },
            )
        }

        return featureCollection(features)
    }

    fun track(points: List<ws.freeflight.data.RecordedPoint>): String {
        if (points.size < 2) return empty
        val coordinates = points.map { coordinate(it.lon, it.lat) }
        val feature = feature(
            geometry = buildJsonObject {
                put("type", "LineString")
                put("coordinates", JsonArray(coordinates))
            },
            properties = buildJsonObject {},
        )
        return featureCollection(listOf(feature))
    }

    fun gairmets(records: List<ws.freeflight.data.GAirmet>): String =
        featureCollection(
            records.mapNotNull { r ->
                val coords = r.coords.mapNotNull {
                    val lat = it.lat.toDoubleOrNull() ?: return@mapNotNull null
                    val lon = it.lon.toDoubleOrNull() ?: return@mapNotNull null
                    coordinate(lon, lat)
                }
                if (coords.isEmpty()) return@mapNotNull null

                val geometry = if (r.geometryType.equals("LINE", ignoreCase = true)) {
                    buildJsonObject {
                        put("type", "LineString")
                        put("coordinates", JsonArray(coords))
                    }
                } else {
                    val ring = if (coords.first() != coords.last()) coords + coords.first() else coords
                    buildJsonObject {
                        put("type", "Polygon")
                        put("coordinates", buildJsonArray { add(JsonArray(ring)) })
                    }
                }

                feature(
                    geometry = geometry,
                    properties = buildJsonObject {
                        put("hazard", r.hazard)
                        put("tag", r.tag)
                        r.severity?.let { put("severity", it) }
                        r.base?.let { put("base", it) }
                        r.top?.let { put("top", it) }
                        r.fzlbase?.let { put("fzlbase", it) }
                        r.fzltop?.let { put("fzltop", it) }
                        put("validTime", r.validTime)
                        put("product", r.product)
                    },
                )
            }
        )

    fun sigmets(records: List<ws.freeflight.data.Sigmet>): String =
        featureCollection(
            records.mapNotNull { r ->
                val coords = r.coords.map { coordinate(it.lon, it.lat) }
                if (coords.isEmpty()) return@mapNotNull null
                val ring = if (coords.first() != coords.last()) coords + coords.first() else coords
                val geometry = buildJsonObject {
                    put("type", "Polygon")
                    put("coordinates", buildJsonArray { add(JsonArray(ring)) })
                }
                feature(
                    geometry = geometry,
                    properties = buildJsonObject {
                        put("hazard", r.hazard)
                        put("seriesId", r.seriesId)
                        put("icaoId", r.icaoId)
                        put("alphaChar", r.alphaChar)
                        r.altitudeLow1?.let { put("altitudeLow1", it) }
                        r.altitudeHi1?.let { put("altitudeHi1", it) }
                        put("rawAirSigmet", r.rawAirSigmet)
                    },
                )
            }
        )

    fun cwas(records: List<ws.freeflight.data.Cwa>): String =
        featureCollection(
            records.mapNotNull { r ->
                val coords = r.coords.mapNotNull {
                    val lat = it.lat.toDoubleOrNull() ?: return@mapNotNull null
                    val lon = it.lon.toDoubleOrNull() ?: return@mapNotNull null
                    coordinate(lon, lat)
                }
                if (coords.isEmpty()) return@mapNotNull null
                val ring = if (coords.first() != coords.last()) coords + coords.first() else coords
                val geometry = buildJsonObject {
                    put("type", "Polygon")
                    put("coordinates", buildJsonArray { add(JsonArray(ring)) })
                }
                feature(
                    geometry = geometry,
                    properties = buildJsonObject {
                        put("hazard", r.hazard)
                        put("cwsu", r.cwsu)
                        put("name", r.name)
                        put("seriesId", r.seriesId)
                        r.base?.let { put("base", it) }
                        r.top?.let { put("top", it) }
                        put("rawText", r.rawText)
                    },
                )
            }
        )

    fun pireps(records: List<ws.freeflight.data.Pirep>): String =
        featureCollection(
            records.map { r ->
                feature(
                    geometry = point(r.lon, r.lat),
                    properties = buildJsonObject {
                        put("severity", ws.freeflight.data.PirepSeverity.valueOf(r.severity().name).name)
                        put("urgent", r.pirepType.equals("Urgent PIREP", ignoreCase = true))
                        put("summary", r.summary())
                        put("rawOb", r.rawOb)
                        r.acType?.let { put("acType", it) }
                        r.fltLvl?.let { put("fltLvl", it) }
                        put("obsTime", r.obsTime)
                    },
                )
            }
        )

    val empty: String = featureCollection(emptyList())


    private fun featureCollection(features: List<JsonObject>): String =
        json.encodeToString(
            JsonObject.serializer(),
            buildJsonObject {
                put("type", "FeatureCollection")
                put("features", JsonArray(features))
            },
        )

    private fun feature(geometry: JsonElement, properties: JsonObject) = buildJsonObject {
        put("type", "Feature")
        put("geometry", geometry)
        put("properties", properties)
    }

    private fun point(lon: Double, lat: Double) = buildJsonObject {
        put("type", "Point")
        put("coordinates", coordinate(lon, lat))
    }

    private fun coordinate(lon: Double, lat: Double) =
        JsonArray(listOf(JsonPrimitive(lon), JsonPrimitive(lat)))
}
