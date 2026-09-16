package ws.freeflight.data

import org.json.JSONObject
import kotlin.math.max
import kotlin.math.min

data class AirspaceCrossingWarning(
    val id: String,
    val name: String,
    val airspaceClass: String,
    val floor: String,
    val ceiling: String,
) {
    val formattedClass: String
        get() = when (airspaceClass.uppercase()) {
            "B", "C", "D" -> "Class $airspaceClass"
            "MOA" -> "MOA"
            "RESTRICTED", "R" -> "Restricted"
            "PROHIBITED", "P" -> "Prohibited"
            "WARNING", "W" -> "Warning Area"
            "ALERT", "A" -> "Alert Area"
            else -> airspaceClass
        }
}

/**
 * Checks route legs against airspace boundaries to detect intersections and crossings.
 * Matches the web EFB implementation in apps/web/src/planning/airspaceCrossing.ts.
 */
object AirspaceCrossingDetector {

    private data class Point(val lon: Double, val lat: Double)

    fun findCrossedAirspace(
        points: List<PlannedWaypoint>,
        volumes: List<uniffi.ff_uniffi.Airspace>,
    ): List<AirspaceCrossingWarning> {
        if (points.size < 2 || volumes.isEmpty()) return emptyList()
        val crossed = mutableListOf<AirspaceCrossingWarning>()
        val seenIds = mutableSetOf<String>()

        for (i in 0 until points.size - 1) {
            val from = Point(points[i].lon, points[i].lat)
            val to = Point(points[i + 1].lon, points[i + 1].lat)

            for (vol in volumes) {
                if (vol.id in seenIds) continue
                if (legCrossesAirspace(from, to, vol.boundaryGeojson)) {
                    seenIds.add(vol.id)
                    crossed.add(
                        AirspaceCrossingWarning(
                            id = vol.id,
                            name = vol.name,
                            airspaceClass = vol.`class`,
                            floor = vol.floor,
                            ceiling = vol.ceiling,
                        )
                    )
                }
            }
        }
        return crossed
    }

    private fun legCrossesAirspace(from: Point, to: Point, boundaryGeojson: String): Boolean {
        return try {
            val root = JSONObject(boundaryGeojson)
            val type = root.optString("type")
            val coords = root.optJSONArray("coordinates") ?: return false

            when (type) {
                "Polygon" -> {
                    val rings = parsePolygonRings(coords)
                    checkPolygonCrossed(from, to, rings)
                }
                "MultiPolygon" -> {
                    for (p in 0 until coords.length()) {
                        val polyArray = coords.optJSONArray(p) ?: continue
                        val rings = parsePolygonRings(polyArray)
                        if (checkPolygonCrossed(from, to, rings)) return true
                    }
                    false
                }
                else -> false
            }
        } catch (_: Exception) {
            false
        }
    }

    private fun parsePolygonRings(polyJson: org.json.JSONArray): List<List<Point>> {
        val rings = mutableListOf<List<Point>>()
        for (r in 0 until polyJson.length()) {
            val ringArray = polyJson.optJSONArray(r) ?: continue
            val ring = mutableListOf<Point>()
            for (pt in 0 until ringArray.length()) {
                val coord = ringArray.optJSONArray(pt) ?: continue
                if (coord.length() >= 2) {
                    ring.add(Point(coord.getDouble(0), coord.getDouble(1)))
                }
            }
            if (ring.isNotEmpty()) {
                rings.add(ring)
            }
        }
        return rings
    }

    private fun checkPolygonCrossed(from: Point, to: Point, rings: List<List<Point>>): Boolean {
        val exterior = rings.firstOrNull() ?: return false

        // Endpoint inside check
        if (pointInPolygon(from, rings) || pointInPolygon(to, rings)) {
            return true
        }

        // Segment crossing exterior ring edges
        for (i in 0 until exterior.size - 1) {
            if (segmentsIntersect(from, to, exterior[i], exterior[i + 1])) {
                return true
            }
        }
        return false
    }

    private fun pointInPolygon(p: Point, rings: List<List<Point>>): Boolean {
        val exterior = rings.firstOrNull() ?: return false
        if (!pointInRing(p, exterior)) return false
        // Ensure not inside any holes
        for (i in 1 until rings.size) {
            if (pointInRing(p, rings[i])) return false
        }
        return true
    }

    private fun pointInRing(p: Point, ring: List<Point>): Boolean {
        var inside = false
        val x = p.lon
        val y = p.lat
        var j = ring.size - 1
        for (i in ring.indices) {
            val xi = ring[i].lon
            val yi = ring[i].lat
            val xj = ring[j].lon
            val yj = ring[j].lat

            val crosses = ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi)
            if (crosses) {
                inside = !inside
            }
            j = i
        }
        return inside
    }

    private fun orientation(a: Point, b: Point, c: Point): Double {
        return (b.lon - a.lon) * (c.lat - a.lat) - (b.lat - a.lat) * (c.lon - a.lon)
    }

    private fun onSegment(a: Point, b: Point, p: Point): Boolean {
        return (min(a.lon, b.lon) <= p.lon && p.lon <= max(a.lon, b.lon) &&
                min(a.lat, b.lat) <= p.lat && p.lat <= max(a.lat, b.lat))
    }

    private fun segmentsIntersect(a1: Point, a2: Point, b1: Point, b2: Point): Boolean {
        val d1 = orientation(b1, b2, a1)
        val d2 = orientation(b1, b2, a2)
        val d3 = orientation(a1, a2, b1)
        val d4 = orientation(a1, a2, b2)

        if (((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) &&
            ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))
        ) {
            return true
        }

        // Collinear cases
        if (d1 == 0.0 && onSegment(b1, b2, a1)) return true
        if (d2 == 0.0 && onSegment(b1, b2, a2)) return true
        if (d3 == 0.0 && onSegment(a1, a2, b1)) return true
        if (d4 == 0.0 && onSegment(a1, a2, b2)) return true

        return false
    }
}
