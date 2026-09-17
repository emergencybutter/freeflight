package ws.freeflight.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * One row of a POH performance table, at a given pressure altitude.
 *
 * Field names match `ff_planning::performance::PerformancePoint`, because
 * these are serialised straight into the planning call, and
 * `ff-accounts`' `/aircraft/:id/performance/:phase` rows, because this
 * table is shaped so it can be synced to an account later without a
 * migration.
 */
@Serializable
data class PerformancePoint(
    @SerialName("pressure_altitude_ft") val pressureAltitudeFt: Double,
    @SerialName("tas_kt") val tasKt: Double,
    @SerialName("fuel_gph") val fuelGph: Double,
    /** Positive magnitude; the phase supplies the sign. Null for cruise. */
    @SerialName("vertical_speed_fpm") val verticalSpeedFpm: Double? = null,
    /** `"65%"`, `"2400 RPM"`. Empty for climb/descent. */
    @SerialName("power_setting") val powerSetting: String = "",
)

/** The three tables belonging to one aircraft; any may be empty. */
@Serializable
data class AircraftPerformance(
    val climb: List<PerformancePoint> = emptyList(),
    val cruise: List<PerformancePoint> = emptyList(),
    val descent: List<PerformancePoint> = emptyList(),
) {
    val isEmpty: Boolean get() = climb.isEmpty() && cruise.isEmpty() && descent.isEmpty()
}

/** Which table a performance row belongs to. */
enum class PerformancePhase(val wire: String, val label: String) {
    CLIMB("climb", "Climb"),
    CRUISE("cruise", "Cruise"),
    DESCENT("descent", "Descent"),
}

/**
 * One aircraft in the pilot's fleet, held on this device.
 *
 * The columns mirror `ff-accounts`' `Aircraft` one-to-one (see
 * `apps/web/src/aircraft.ts`), even though nothing syncs yet: the web
 * client keeps a fleet behind an account, this client has no auth, and
 * shaping the local rows differently would guarantee a migration the day
 * the two are joined up. Everything nullable there is nullable here, for
 * the same reason — a pilot who knows their cruise numbers but not their
 * CG limits should still be able to plan a nav log.
 *
 * [verifiedAt] carries the same meaning it does on the server: null until
 * the pilot has checked these figures against their own POH. A planning
 * tool that presents unconfirmed numbers as though they were measured is
 * the kind of wrong that reaches the aircraft, so the UI says so wherever
 * this feeds a plan (DESIGN.md §9.5.3).
 */
@Serializable
data class Aircraft(
    val id: Long = 0,
    val registration: String,
    val name: String? = null,
    @SerialName("icao_type") val icaoType: String? = null,
    @SerialName("serial_number") val serialNumber: String? = null,

    @SerialName("cruise_tas_kt") val cruiseTasKt: Double? = null,
    @SerialName("cruise_fuel_gph") val cruiseFuelGph: Double? = null,
    @SerialName("climb_rate_fpm") val climbRateFpm: Double? = null,
    @SerialName("climb_tas_kt") val climbTasKt: Double? = null,
    @SerialName("climb_fuel_gph") val climbFuelGph: Double? = null,
    @SerialName("descent_rate_fpm") val descentRateFpm: Double? = null,
    @SerialName("descent_tas_kt") val descentTasKt: Double? = null,
    @SerialName("descent_fuel_gph") val descentFuelGph: Double? = null,
    @SerialName("taxi_fuel_gal") val taxiFuelGal: Double? = null,
    @SerialName("fuel_capacity_gal") val fuelCapacityGal: Double? = null,
    @SerialName("reserve_minutes") val reserveMinutes: Int? = null,

    @SerialName("max_gross_weight_lb") val maxGrossWeightLb: Double? = null,
    @SerialName("forward_cg_limit_in") val forwardCgLimitIn: Double? = null,
    @SerialName("aft_cg_limit_in") val aftCgLimitIn: Double? = null,
    @SerialName("empty_weight_lb") val emptyWeightLb: Double? = null,
    @SerialName("empty_cg_in") val emptyCgIn: Double? = null,

    @SerialName("cruise_altitude_ft") val cruiseAltitudeFt: Double? = null,
    @SerialName("cruise_power_setting") val cruisePowerSetting: String? = null,

    /** Null until the pilot confirms the figures against their POH. */
    @SerialName("verified_at") val verifiedAt: String? = null,

    val performance: AircraftPerformance = AircraftPerformance(),
) {
    /** What to call this aircraft in a list. */
    val displayName: String
        get() = listOfNotNull(
            registration.takeIf { it.isNotBlank() },
            name?.takeIf { it.isNotBlank() },
        ).joinToString(" · ").ifBlank { "Unnamed aircraft" }

    val isVerified: Boolean get() = !verifiedAt.isNullOrBlank()

    /**
     * Fold this aircraft into the shape the planning call takes.
     *
     * `base` supplies the loading figures the nav-log screen edits per
     * flight (who is on board, how much fuel) — those belong to the
     * flight, not the aircraft, so they are deliberately not stored here.
     * Anything this aircraft leaves null keeps `base`'s value, which is
     * how a partly-filled aircraft still plans.
     */
    fun toProfile(base: AircraftProfileData = AircraftProfileData()): AircraftProfileData =
        base.copy(
            name = displayName,
            cruiseTasKt = cruiseTasKt ?: base.cruiseTasKt,
            fuelBurnGph = cruiseFuelGph ?: base.fuelBurnGph,
            cruiseAltitudeFt = cruiseAltitudeFt ?: base.cruiseAltitudeFt,
            climbRateFpm = climbRateFpm ?: base.climbRateFpm,
            climbTasKt = climbTasKt ?: base.climbTasKt,
            climbFuelGph = climbFuelGph ?: base.climbFuelGph,
            descentRateFpm = descentRateFpm ?: base.descentRateFpm,
            descentTasKt = descentTasKt ?: base.descentTasKt,
            descentFuelGph = descentFuelGph ?: base.descentFuelGph,
            taxiFuelGal = taxiFuelGal ?: base.taxiFuelGal,
            fuelCapacityGal = fuelCapacityGal ?: base.fuelCapacityGal,
            reserveMinutes = reserveMinutes ?: base.reserveMinutes,
            emptyWeightLb = emptyWeightLb ?: base.emptyWeightLb,
            emptyCgIn = emptyCgIn ?: base.emptyCgIn,
            maxGrossWeightLb = maxGrossWeightLb ?: base.maxGrossWeightLb,
            forwardCgLimitIn = forwardCgLimitIn ?: base.forwardCgLimitIn,
            aftCgLimitIn = aftCgLimitIn ?: base.aftCgLimitIn,
            cruisePowerSetting = cruisePowerSetting ?: base.cruisePowerSetting,
            // Empty tables are sent as null so the core falls back to the
            // scalars above rather than interpolating an empty table.
            performance = performance.takeUnless { it.isEmpty },
        )

    /**
     * Take back the fields this aircraft owns after the planning sheet
     * edited them.
     *
     * The inverse of [toProfile], and deliberately partial: the loading
     * figures the same screen edits — who is aboard, how much fuel — are
     * properties of the flight, not the airframe, so they are not copied
     * back. Without this, editing cruise TAS would change the plan and be
     * lost the moment another aircraft was selected.
     */
    fun updatedFrom(profile: AircraftProfileData): Aircraft = copy(
        cruiseTasKt = profile.cruiseTasKt,
        cruiseFuelGph = profile.fuelBurnGph,
        cruiseAltitudeFt = profile.cruiseAltitudeFt,
        climbRateFpm = profile.climbRateFpm,
        climbTasKt = profile.climbTasKt,
        climbFuelGph = profile.climbFuelGph,
        descentRateFpm = profile.descentRateFpm,
        descentTasKt = profile.descentTasKt,
        descentFuelGph = profile.descentFuelGph,
        taxiFuelGal = profile.taxiFuelGal,
        fuelCapacityGal = profile.fuelCapacityGal,
        reserveMinutes = profile.reserveMinutes,
        emptyWeightLb = profile.emptyWeightLb,
        emptyCgIn = profile.emptyCgIn,
        maxGrossWeightLb = profile.maxGrossWeightLb,
        forwardCgLimitIn = profile.forwardCgLimitIn,
        aftCgLimitIn = profile.aftCgLimitIn,
        cruisePowerSetting = profile.cruisePowerSetting,
    )

    companion object {
        /**
         * The aircraft a first run starts with.
         *
         * A 172 with book figures, and deliberately *unverified*: these
         * are a starting point to edit, not this airframe's numbers, and
         * the UI has to keep saying so until the pilot confirms them.
         */
        fun starter(): Aircraft = Aircraft(
            registration = "N12345",
            name = "C172 Skyhawk",
            icaoType = "C172",
            cruiseTasKt = 110.0,
            cruiseFuelGph = 8.5,
            cruiseAltitudeFt = 5500.0,
            climbRateFpm = 700.0,
            climbTasKt = 80.0,
            climbFuelGph = 11.5,
            descentRateFpm = 500.0,
            descentTasKt = 120.0,
            descentFuelGph = 6.0,
            taxiFuelGal = 1.5,
            fuelCapacityGal = 40.0,
            reserveMinutes = 45,
            emptyWeightLb = 1680.0,
            emptyCgIn = 38.5,
            maxGrossWeightLb = 2550.0,
            forwardCgLimitIn = 35.0,
            aftCgLimitIn = 47.3,
            verifiedAt = null,
        )
    }
}
