package ws.freeflight.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** A single waypoint selected in a flight plan. */
@Serializable
data class PlannedWaypoint(
    val ident: String,
    val name: String? = null,
    val lat: Double,
    val lon: Double,
)

/** Aircraft performance & profile data for calculation. */
@Serializable
data class AircraftProfileData(
    val name: String = "C172 Skyhawk",
    @SerialName("cruise_tas_kt") val cruiseTasKt: Double = 110.0,
    @SerialName("fuel_burn_gph") val fuelBurnGph: Double = 8.5,
    @SerialName("cruise_altitude_ft") val cruiseAltitudeFt: Double? = 5500.0,
    @SerialName("climb_rate_fpm") val climbRateFpm: Double? = 700.0,
    @SerialName("climb_tas_kt") val climbTasKt: Double? = 80.0,
    @SerialName("climb_fuel_gph") val climbFuelGph: Double? = 11.5,
    @SerialName("descent_rate_fpm") val descentRateFpm: Double? = 500.0,
    @SerialName("descent_tas_kt") val descentTasKt: Double? = 120.0,
    @SerialName("descent_fuel_gph") val descentFuelGph: Double? = 6.0,
    @SerialName("taxi_fuel_gal") val taxiFuelGal: Double? = 1.5,
    @SerialName("fuel_capacity_gal") val fuelCapacityGal: Double? = 40.0,
    @SerialName("reserve_minutes") val reserveMinutes: Int? = 45,

    // Weight & Balance
    @SerialName("empty_weight_lb") val emptyWeightLb: Double = 1680.0,
    @SerialName("empty_cg_in") val emptyCgIn: Double = 38.5,
    @SerialName("max_gross_weight_lb") val maxGrossWeightLb: Double = 2550.0,
    @SerialName("arm_pilot_in") val armPilotIn: Double = 37.0,
    @SerialName("arm_passenger_in") val armPassengerIn: Double = 73.0,
    @SerialName("arm_baggage_in") val armBaggageIn: Double = 95.0,
    @SerialName("arm_fuel_in") val armFuelIn: Double = 48.0,

    val weightPilotLb: Double = 170.0,
    val weightPassengerLb: Double = 170.0,
    val weightBaggageLb: Double = 30.0,
    val gallonsFuel: Double = 40.0,
) {
    val totalWeightLb: Double
        get() = emptyWeightLb + weightPilotLb + weightPassengerLb + weightBaggageLb + (gallonsFuel * 6.0)

    val totalMomentInLb: Double
        get() = (emptyWeightLb * emptyCgIn) +
            (weightPilotLb * armPilotIn) +
            (weightPassengerLb * armPassengerIn) +
            (weightBaggageLb * armBaggageIn) +
            (gallonsFuel * 6.0 * armFuelIn)

    val centerOfGravityIn: Double
        get() = if (totalWeightLb > 0) totalMomentInLb / totalWeightLb else 0.0

    val isOverweight: Boolean
        get() = totalWeightLb > maxGrossWeightLb
}

/** Nav log calculation result for one leg. */
@Serializable
data class LegPlanSummary(
    @SerialName("distance_nm") val distanceNm: Double = 0.0,
    @SerialName("true_course_deg") val trueCourseDeg: Double = 0.0,
    @SerialName("true_heading_deg") val trueHeadingDeg: Double = 0.0,
    @SerialName("magnetic_variation_deg") val magneticVariationDeg: Double = 0.0,
    @SerialName("magnetic_course_deg") val magneticCourseDeg: Double = 0.0,
    @SerialName("magnetic_heading_deg") val magneticHeadingDeg: Double = 0.0,
    @SerialName("ground_speed_kt") val groundSpeedKt: Double = 0.0,
    @SerialName("leg_minutes") val legMinutes: Double = 0.0,
    @SerialName("fuel_gal") val fuelGal: Double = 0.0,
)

/** Fuel requirement calculation summary. */
@Serializable
data class FuelSummaryData(
    @SerialName("taxi_gal") val taxiGal: Double = 0.0,
    @SerialName("climb_gal") val climbGal: Double = 0.0,
    @SerialName("cruise_gal") val cruiseGal: Double = 0.0,
    @SerialName("descent_gal") val descentGal: Double = 0.0,
    @SerialName("trip_gal") val tripGal: Double = 0.0,
    @SerialName("reserve_gal") val reserveGal: Double = 0.0,
    @SerialName("required_gal") val requiredGal: Double = 0.0,
    @SerialName("capacity_gal") val capacityGal: Double? = null,
    @SerialName("within_capacity") val withinCapacity: Boolean? = true,
)

/** Output of the Rust planning engine. */
@Serializable
data class FlightPlanSummaryData(
    val legs: List<LegPlanSummary> = emptyList(),
    @SerialName("total_distance_nm") val totalDistanceNm: Double = 0.0,
    @SerialName("total_ete_hours") val totalEteHours: Double = 0.0,
    val fuel: FuelSummaryData = FuelSummaryData(),
)
