package ws.freeflight.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive

/**
 * The weather records `ff-api` hands back, which are `ff-weather`'s own
 * validated structs (DESIGN.md §4.1) — already decoded server-side, so the
 * client's job is display, not parsing.
 *
 * Only the fields the UI shows are modelled; the JSON parser is configured
 * to ignore the rest, so a new upstream field never breaks a briefing.
 */
@Serializable
data class Metar(
    @SerialName("icaoId") val icaoId: String,
    @SerialName("obsTime") val obsTime: Long,
    @SerialName("rawOb") val rawText: String,
    val temp: Double? = null,
    val dewp: Double? = null,
    /** Degrees, or the string "VRB" — hence [JsonElement] rather than a number. */
    val wdir: JsonElement? = null,
    val wspd: Int? = null,
    val wgst: Int? = null,
    /** Statute miles, but sometimes a string like "10+". */
    val visib: JsonElement? = null,
    val altim: Double? = null,
    @SerialName("wxString") val wxString: String? = null,
    /** "VFR" | "MVFR" | "IFR" | "LIFR", computed upstream. */
    @SerialName("fltCat") val flightCategory: String? = null,
) {
    val windLabel: String
        get() {
            val direction = wdir?.plainString()
            val speed = wspd
            if (direction == null || speed == null) return "—"
            if (speed == 0) return "Calm"
            val gust = wgst?.let { "G$it" } ?: ""
            val from = if (direction == "VRB") "VRB" else direction.padStart(3, '0')
            return "$from° at $speed$gust kt"
        }

    val visibilityLabel: String get() = visib?.plainString()?.let { "$it sm" } ?: "—"

    /** `altim` arrives in hectopascals; US altimeter settings read in inches. */
    val altimeterLabel: String
        get() = altim?.let { String.format("%.2f inHg", it / 33.8639) } ?: "—"
}

@Serializable
data class TafPeriod(
    @SerialName("timeFrom") val timeFrom: Long = 0,
    @SerialName("timeTo") val timeTo: Long = 0,
    @SerialName("fcstChange") val change: String? = null,
)

@Serializable
data class Taf(
    @SerialName("icaoId") val icaoId: String,
    @SerialName("rawTAF") val rawText: String = "",
    @SerialName("issueTime") val issueTime: String? = null,
    @SerialName("fcsts") val periods: List<TafPeriod> = emptyList(),
)

/** The scalar inside a field that may be a number or a string ("VRB", "10+"). */
private fun JsonElement.plainString(): String? =
    (this as? JsonPrimitive)?.content?.takeIf { it.isNotBlank() && it != "null" }
