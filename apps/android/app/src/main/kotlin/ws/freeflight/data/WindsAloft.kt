package ws.freeflight.data

import kotlinx.serialization.Serializable
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.hypot

sealed class WindReading {
    object LightAndVariable : WindReading()
    data class Directional(val directionDeg: Int, val speedKt: Int) : WindReading()
}

data class WindsAloftLevel(
    val altitudeFt: Int,
    val wind: WindReading,
    val tempC: Int?,
)

data class StationWindsAloft(
    val stationId: String,
    val levels: List<WindsAloftLevel>,
    val lat: Double?,
    val lon: Double?,
)

data class WindsAloftBulletin(
    val dataBasedOn: String,
    val validTime: String,
    val forUse: String,
    val stations: List<StationWindsAloft>,
)

@Serializable
data class PlanningWind(
    val direction_true_deg: Double,
    val speed_kt: Double,
)

object WindsAloftParser {
    fun parse(jsonStr: String): WindsAloftBulletin {
        val root = JSONObject(jsonStr)
        val stationsArray = root.optJSONArray("stations") ?: JSONArray()
        val stations = mutableListOf<StationWindsAloft>()

        for (i in 0 until stationsArray.length()) {
            val st = stationsArray.getJSONObject(i)
            val stationId = st.getString("station_id")
            val lat = if (st.has("lat") && !st.isNull("lat")) st.getDouble("lat") else null
            val lon = if (st.has("lon") && !st.isNull("lon")) st.getDouble("lon") else null
            val levelsArray = st.optJSONArray("levels") ?: JSONArray()
            val levels = mutableListOf<WindsAloftLevel>()

            for (j in 0 until levelsArray.length()) {
                val lvl = levelsArray.getJSONObject(j)
                val alt = lvl.getInt("altitude_ft")
                val temp = if (lvl.has("temp_c") && !lvl.isNull("temp_c")) lvl.getInt("temp_c") else null
                val windVal = lvl.opt("wind")
                val wind: WindReading? = when {
                    windVal is String && windVal == "LightAndVariable" -> WindReading.LightAndVariable
                    windVal is JSONObject && windVal.has("Directional") -> {
                        val dirObj = windVal.getJSONObject("Directional")
                        WindReading.Directional(
                            dirObj.getInt("direction_deg"),
                            dirObj.getInt("speed_kt"),
                        )
                    }
                    else -> null
                }
                if (wind != null) {
                    levels.add(WindsAloftLevel(alt, wind, temp))
                }
            }
            stations.add(StationWindsAloft(stationId, levels, lat, lon))
        }

        return WindsAloftBulletin(
            dataBasedOn = root.optString("data_based_on"),
            validTime = root.optString("valid_time"),
            forUse = root.optString("for_use"),
            stations = stations,
        )
    }
}

object WindsAloftResolver {
    fun windsForRoute(
        points: List<PlannedWaypoint>,
        cruiseAltitudeFt: Double,
        bulletin: WindsAloftBulletin?,
    ): List<PlanningWind?> {
        if (bulletin == null || points.size < 2) return emptyList()
        return (0 until points.size - 1).map { i ->
            legWind(
                from = points[i],
                to = points[i + 1],
                cruiseAltFt = cruiseAltitudeFt,
                bulletin = bulletin,
            )
        }
    }

    private fun legWind(
        from: PlannedWaypoint,
        to: PlannedWaypoint,
        cruiseAltFt: Double,
        bulletin: WindsAloftBulletin,
    ): PlanningWind? {
        val midLat = (from.lat + to.lat) / 2.0
        val midLon = (from.lon + to.lon) / 2.0

        var bestStation: StationWindsAloft? = null
        var bestDist = Double.MAX_VALUE
        for (st in bulletin.stations) {
            val sLat = st.lat ?: continue
            val sLon = st.lon ?: continue
            val dLat = (midLat - sLat) * 60.0
            val dLon = (midLon - sLon) * 60.0 * cos(Math.toRadians(midLat))
            val dist = hypot(dLat, dLon)
            if (dist < bestDist) {
                bestDist = dist
                bestStation = st
            }
        }

        val station = bestStation ?: return null
        if (station.levels.isEmpty()) return null

        var nearestLevel = station.levels[0]
        for (lvl in station.levels) {
            if (abs(lvl.altitudeFt - cruiseAltFt) < abs(nearestLevel.altitudeFt - cruiseAltFt)) {
                nearestLevel = lvl
            }
        }

        return when (val w = nearestLevel.wind) {
            is WindReading.Directional -> PlanningWind(w.directionDeg.toDouble(), w.speedKt.toDouble())
            is WindReading.LightAndVariable -> null
        }
    }
}
