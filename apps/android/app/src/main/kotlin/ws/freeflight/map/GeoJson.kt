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
 * Every *value* goes through the JSON encoder, because airport and airspace
 * names are real data — "O'Hare", `"BIG BEAR CITY"`, names with quotes and
 * non-ASCII — and hand-quoting them gets that wrong in exactly the cases a
 * US-wide bundle contains. Only the fixed structural keys are literal text,
 * and they are written a feature at a time: see [FeatureSink].
 */
object GeoJson {

    private val json = Json

    fun airports(airports: List<Airport>, flightCategories: Map<String, String>): String =
        featureCollection {
            airports.forEach { airport ->
                feature(
                    geometry = point(airport.lon, airport.lat),
                    properties = buildJsonObject {
                        put("icao", airport.icao)
                        put("name", airport.name)
                        put("hasProcedures", airport.hasProcedures)
                        put("towered", airport.towered)
                        put("type", airport.airportType)
                        flightCategories[airport.icao]?.let { put("flightCategory", it) }
                    },
                )
            }
        }

    fun airspace(volumes: List<Airspace>): String =
        featureCollection {
            volumes.forEach { volume ->
                // `boundaryGeojson` is already GeoJSON geometry text, so it
                // goes out as it came in. It is still parsed first, because a
                // single malformed boundary spliced in raw would invalidate
                // the whole collection rather than drop one volume — but the
                // parsed tree is discarded immediately instead of being held
                // until every volume has been read.
                val valid = runCatching {
                    json.parseToJsonElement(volume.boundaryGeojson) is JsonObject
                }.getOrDefault(false)
                if (!valid) return@forEach
                rawFeature(
                    geometryJson = volume.boundaryGeojson,
                    properties = buildJsonObject {
                        put("id", volume.id)
                        put("name", volume.name)
                        put("class", volume.`class`)
                        put("floor", volume.floor)
                        put("ceiling", volume.ceiling)
                    },
                )
            }
        }

    fun procedure(detail: ProcedureDetail): String = featureCollection {
        for (transition in detail.transitions) {
            val coordinates = transition.legs.mapNotNull { leg ->
                val lat = leg.lat ?: return@mapNotNull null
                val lon = leg.lon ?: return@mapNotNull null
                lon to lat
            }
            if (coordinates.size >= 2) {
                feature(
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
                feature(
                    geometry = point(lon, lat),
                    properties = buildJsonObject {
                        put("fix", leg.fixIdent ?: "")
                        put("altitude", leg.altitudeConstraint ?: "")
                        put("missed", transition.kind.equals("MISSED", ignoreCase = true))
                    },
                )
            }
        }
    }

    fun route(waypoints: List<PlannedWaypoint>): String {
        if (waypoints.isEmpty()) return empty
        return featureCollection {
            if (waypoints.size >= 2) {
                feature(
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
                feature(
                    geometry = point(wp.lon, wp.lat),
                    properties = buildJsonObject {
                        put("ident", wp.ident)
                    },
                )
            }
        }
    }

    fun track(points: List<ws.freeflight.data.RecordedPoint>): String {
        if (points.size < 2) return empty
        return featureCollection {
            feature(
                geometry = buildJsonObject {
                    put("type", "LineString")
                    put("coordinates", JsonArray(points.map { coordinate(it.lon, it.lat) }))
                },
                properties = buildJsonObject {},
            )
        }
    }

    fun gairmets(records: List<ws.freeflight.data.GAirmet>): String =
        featureCollection {
            records.forEach { r ->
                val coords = r.coords.mapNotNull {
                    val lat = it.lat.toDoubleOrNull() ?: return@mapNotNull null
                    val lon = it.lon.toDoubleOrNull() ?: return@mapNotNull null
                    coordinate(lon, lat)
                }
                if (coords.isEmpty()) return@forEach

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
        }

    fun sigmets(records: List<ws.freeflight.data.Sigmet>): String =
        featureCollection {
            records.forEach { r ->
                val coords = r.coords.map { coordinate(it.lon, it.lat) }
                if (coords.isEmpty()) return@forEach
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
        }

    fun cwas(records: List<ws.freeflight.data.Cwa>): String =
        featureCollection {
            records.forEach { r ->
                val coords = r.coords.mapNotNull {
                    val lat = it.lat.toDoubleOrNull() ?: return@mapNotNull null
                    val lon = it.lon.toDoubleOrNull() ?: return@mapNotNull null
                    coordinate(lon, lat)
                }
                if (coords.isEmpty()) return@forEach
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
        }

    fun pireps(records: List<ws.freeflight.data.Pirep>): String =
        featureCollection {
            records.forEach { r ->
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
        }

    val empty: String = featureCollection { }

    private inline fun featureCollection(build: FeatureSink.() -> Unit): String =
        FeatureSink().apply(build).finish()

    /**
     * Writes features straight out as text, one at a time.
     *
     * The collection used to be assembled as one `JsonObject` tree and then
     * serialized, which meant the tree and the finished string were both live
     * at once — and the tree is much the larger of the two, since every
     * coordinate costs a boxed `JsonPrimitive` where the text costs a dozen
     * characters. A wide viewport over US airspace exhausted the 256 MB heap
     * that way, which is the crash this replaces. Here each feature's model
     * objects are garbage the moment it has been written, so only the output
     * grows with the feature count.
     */
    private class FeatureSink {
        private val out = StringBuilder(INITIAL_CAPACITY)
            .append("{\"type\":\"FeatureCollection\",\"features\":[")
        private var wroteOne = false

        fun feature(geometry: JsonElement, properties: JsonObject) =
            rawFeature(json.encodeToString(JsonElement.serializer(), geometry), properties)

        /** [geometryJson] must already be valid GeoJSON geometry text. */
        fun rawFeature(geometryJson: String, properties: JsonObject) {
            if (wroteOne) out.append(',') else wroteOne = true
            out.append("{\"type\":\"Feature\",\"geometry\":")
                .append(geometryJson)
                .append(",\"properties\":")
                .append(json.encodeToString(JsonObject.serializer(), properties))
                .append('}')
        }

        fun finish(): String = out.append("]}").toString()

        private companion object {
            const val INITIAL_CAPACITY = 8 * 1024
        }
    }

    private fun point(lon: Double, lat: Double) = buildJsonObject {
        put("type", "Point")
        put("coordinates", coordinate(lon, lat))
    }

    private fun coordinate(lon: Double, lat: Double) =
        JsonArray(listOf(JsonPrimitive(lon), JsonPrimitive(lat)))
}
