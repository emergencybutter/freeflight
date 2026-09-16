package ws.freeflight.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class GAirmetCoord(
    val lat: String,
    val lon: String,
)

@Serializable
data class GAirmet(
    val tag: String,
    @SerialName("forecastHour") val forecastHour: Int = 0,
    @SerialName("validTime") val validTime: String = "",
    val hazard: String = "",
    @SerialName("geometryType") val geometryType: String = "",
    val severity: String? = null,
    @SerialName("due_to") val dueTo: String? = null,
    val status: String = "",
    val top: String? = null,
    val base: String? = null,
    val fzltop: String? = null,
    val fzlbase: String? = null,
    val product: String = "",
    val coords: List<GAirmetCoord> = emptyList(),
)

@Serializable
data class SigmetCoord(
    val lat: Double,
    val lon: Double,
)

@Serializable
data class Sigmet(
    @SerialName("icaoId") val icaoId: String = "",
    @SerialName("alphaChar") val alphaChar: String = "",
    @SerialName("seriesId") val seriesId: String = "",
    @SerialName("validTimeFrom") val validTimeFrom: Long = 0,
    @SerialName("validTimeTo") val validTimeTo: Long = 0,
    @SerialName("airSigmetType") val airSigmetType: String = "",
    val hazard: String = "",
    @SerialName("altitudeHi1") val altitudeHi1: Int? = null,
    @SerialName("altitudeLow1") val altitudeLow1: Int? = null,
    @SerialName("rawAirSigmet") val rawAirSigmet: String = "",
    val coords: List<SigmetCoord> = emptyList(),
)

@Serializable
data class Cwa(
    val cwsu: String = "",
    val name: String = "",
    @SerialName("validTimeFrom") val validTimeFrom: Long = 0,
    @SerialName("validTimeTo") val validTimeTo: Long = 0,
    @SerialName("seriesId") val seriesId: String = "",
    val hazard: String = "",
    val qualifier: String? = null,
    val base: Int? = null,
    val top: Int? = null,
    @SerialName("rawText") val rawText: String = "",
    val coords: List<GAirmetCoord> = emptyList(),
)

@Serializable
data class Pirep(
    @SerialName("receiptTime") val receiptTime: String = "",
    @SerialName("obsTime") val obsTime: Long = 0,
    @SerialName("icaoId") val icaoId: String? = null,
    @SerialName("acType") val acType: String? = null,
    val lat: Double,
    val lon: Double,
    @SerialName("fltLvl") val fltLvl: Int? = null,
    @SerialName("wxString") val wxString: String? = null,
    val temp: Double? = null,
    @SerialName("icgBas1") val icgBase1: Int? = null,
    @SerialName("icgTop1") val icgTop1: Int? = null,
    @SerialName("icgInt1") val icgIntensity1: String? = null,
    @SerialName("icgType1") val icgType1: String? = null,
    @SerialName("tbBas1") val tbBase1: Int? = null,
    @SerialName("tbTop1") val tbTop1: Int? = null,
    @SerialName("tbInt1") val tbIntensity1: String? = null,
    @SerialName("tbType1") val tbType1: String? = null,
    @SerialName("pirepType") val pirepType: String = "PIREP",
    @SerialName("rawOb") val rawOb: String = "",
)

enum class PirepSeverity {
    SEVERE,
    MODERATE,
    LIGHT,
    NONE,
}

fun Pirep.severity(): PirepSeverity {
    if (pirepType == "Urgent PIREP") return PirepSeverity.SEVERE
    val text = listOfNotNull(icgIntensity1, tbIntensity1).joinToString(" ").uppercase()
    return when {
        text.contains("SEV") || text.contains("EXTM") -> PirepSeverity.SEVERE
        text.contains("MOD") -> PirepSeverity.MODERATE
        text.contains("LGT") || text.contains("LIGHT") -> PirepSeverity.LIGHT
        else -> PirepSeverity.NONE
    }
}

fun Pirep.summary(): String {
    val parts = mutableListOf<String>()
    acType?.let { parts.add(it) }
    fltLvl?.let { parts.add("FL$it") }
    val icing = listOfNotNull(icgIntensity1, icgType1).joinToString(" ").trim()
    if (icing.isNotEmpty()) parts.add("ICE $icing")
    val turb = listOfNotNull(tbIntensity1, tbType1).joinToString(" ").trim()
    if (turb.isNotEmpty()) parts.add("TURB $turb")
    return parts.joinToString(" · ")
}

sealed interface WeatherHazardDetail {
    data class Airmet(val airmet: GAirmet) : WeatherHazardDetail
    data class SigmetDetail(val sigmet: Sigmet) : WeatherHazardDetail
    data class CwaDetail(val cwa: Cwa) : WeatherHazardDetail
    data class PirepDetail(val pirep: Pirep) : WeatherHazardDetail
}

sealed interface WeatherHazardTap {
    data class AirmetTap(
        val hazard: String,
        val tag: String,
        val severity: String?,
        val base: String?,
        val top: String?,
        val fzlbase: String?,
        val fzltop: String?,
        val validTime: String,
        val product: String,
    ) : WeatherHazardTap

    data class SigmetTap(
        val hazard: String,
        val seriesId: String,
        val icaoId: String,
        val alphaChar: String,
        val altitudeLow1: Int?,
        val altitudeHi1: Int?,
        val rawAirSigmet: String,
    ) : WeatherHazardTap

    data class CwaTap(
        val hazard: String,
        val cwsu: String,
        val name: String,
        val seriesId: String,
        val base: Int?,
        val top: Int?,
        val rawText: String,
    ) : WeatherHazardTap

    data class PirepTap(
        val summary: String,
        val rawOb: String,
        val severity: String,
        val acType: String?,
        val fltLvl: Int?,
        val obsTime: Long = 0,
    ) : WeatherHazardTap
}

