package ws.freeflight.data

import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.Instant

/**
 * The pilot's fleet, on this device.
 *
 * Local-only by design for now. The web client keeps aircraft behind an
 * account so a fleet follows you between devices; this client has no auth
 * at all, and bolting a login onto the one client whose premise is
 * working without a network is a bigger decision than adding a second
 * aeroplane. So the rows live here — but shaped exactly like the server's
 * (`remote_id` included), so joining the two later is a sync, not a
 * migration.
 *
 * Everything is nullable except the registration, because a pilot who
 * knows their cruise numbers but has not yet weighed the aircraft should
 * still be able to plan a nav log. What that costs is tracked by
 * [Aircraft.verifiedAt], which the planning UI surfaces rather than
 * quietly treating book figures as measured ones.
 */
class AircraftRepository(
    context: Context,
    private val scope: CoroutineScope,
) {
    private val helper = RouteDatabaseHelper(context.applicationContext)

    private val _fleet = MutableStateFlow<List<Aircraft>>(emptyList())
    val fleet: StateFlow<List<Aircraft>> = _fleet.asStateFlow()

    private val _selectedId = MutableStateFlow<Long?>(null)
    val selectedId: StateFlow<Long?> = _selectedId.asStateFlow()

    /** The aircraft planning should use, or null before the first load. */
    val selected: Aircraft? get() = _fleet.value.firstOrNull { it.id == _selectedId.value }

    init {
        scope.launch { refresh() }
    }

    suspend fun refresh() = withContext(Dispatchers.IO) {
        val db = helper.readableDatabase
        val aircraft = db.query("aircraft", null, null, null, null, null, "registration").use { c ->
            buildList { while (c.moveToNext()) add(readAircraft(c, db)) }
        }
        // A first run with nothing in the fleet gets a starter aircraft
        // rather than an empty screen and a dead planning sheet. It is
        // deliberately unverified — book figures, not this airframe's.
        val fleet = aircraft.ifEmpty { listOf(insert(Aircraft.starter())) }
        _fleet.value = fleet

        val stored = db.query(
            "selected_aircraft", arrayOf("aircraft_id"), "id = 1", null, null, null, null,
        ).use { c -> if (c.moveToFirst() && !c.isNull(0)) c.getLong(0) else null }
        _selectedId.value = stored?.takeIf { id -> fleet.any { it.id == id } } ?: fleet.first().id
    }

    fun select(id: Long) {
        scope.launch {
            withContext(Dispatchers.IO) {
                helper.writableDatabase.replace(
                    "selected_aircraft", null,
                    ContentValues().apply {
                        put("id", 1)
                        put("aircraft_id", id)
                    },
                )
            }
            _selectedId.value = id
        }
    }

    suspend fun save(aircraft: Aircraft): Aircraft = withContext(Dispatchers.IO) {
        val saved = if (aircraft.id == 0L) insert(aircraft) else update(aircraft)
        refresh()
        saved
    }

    /**
     * Delete an aircraft. The last one is kept: planning has no meaning
     * without an aircraft, and an empty fleet would leave the nav log
     * silently using defaults nobody chose.
     */
    suspend fun delete(id: Long): Boolean = withContext(Dispatchers.IO) {
        if (_fleet.value.size <= 1) return@withContext false
        helper.writableDatabase.delete("aircraft", "id = ?", arrayOf(id.toString()))
        refresh()
        true
    }

    /** Replace one phase's table wholesale — the editor edits a whole grid. */
    suspend fun setPerformance(
        aircraftId: Long,
        phase: PerformancePhase,
        rows: List<PerformancePoint>,
    ) = withContext(Dispatchers.IO) {
        val db = helper.writableDatabase
        db.beginTransaction()
        try {
            db.delete(
                "aircraft_performance", "aircraft_id = ? AND phase = ?",
                arrayOf(aircraftId.toString(), phase.wire),
            )
            for (row in rows) {
                db.insert(
                    "aircraft_performance", null,
                    ContentValues().apply {
                        put("aircraft_id", aircraftId)
                        put("phase", phase.wire)
                        put("pressure_altitude_ft", row.pressureAltitudeFt)
                        put("power_setting", row.powerSetting)
                        row.verticalSpeedFpm?.let { put("vertical_speed_fpm", it) }
                        put("tas_kt", row.tasKt)
                        put("fuel_gph", row.fuelGph)
                    },
                )
            }
            db.setTransactionSuccessful()
        } finally {
            db.endTransaction()
        }
        refresh()
    }

    /** Record that the pilot has checked these figures against their POH. */
    suspend fun markVerified(id: Long, verified: Boolean) = withContext(Dispatchers.IO) {
        helper.writableDatabase.update(
            "aircraft",
            ContentValues().apply {
                if (verified) put("verified_at", Instant.now().toString()) else putNull("verified_at")
                put("updated_at", Instant.now().toString())
            },
            "id = ?", arrayOf(id.toString()),
        )
        refresh()
    }

    private fun insert(aircraft: Aircraft): Aircraft {
        val now = Instant.now().toString()
        val values = aircraft.toContentValues().apply {
            put("created_at", now)
            put("updated_at", now)
        }
        val id = helper.writableDatabase.insert("aircraft", null, values)
        return aircraft.copy(id = id)
    }

    private fun update(aircraft: Aircraft): Aircraft {
        helper.writableDatabase.update(
            "aircraft",
            aircraft.toContentValues().apply { put("updated_at", Instant.now().toString()) },
            "id = ?", arrayOf(aircraft.id.toString()),
        )
        return aircraft
    }

    private fun readAircraft(c: Cursor, db: android.database.sqlite.SQLiteDatabase): Aircraft {
        val id = c.getLong(c.getColumnIndexOrThrow("id"))
        return Aircraft(
            id = id,
            registration = c.str("registration") ?: "",
            name = c.str("name"),
            icaoType = c.str("icao_type"),
            serialNumber = c.str("serial_number"),
            cruiseTasKt = c.dbl("cruise_tas_kt"),
            cruiseFuelGph = c.dbl("cruise_fuel_gph"),
            climbRateFpm = c.dbl("climb_rate_fpm"),
            climbTasKt = c.dbl("climb_tas_kt"),
            climbFuelGph = c.dbl("climb_fuel_gph"),
            descentRateFpm = c.dbl("descent_rate_fpm"),
            descentTasKt = c.dbl("descent_tas_kt"),
            descentFuelGph = c.dbl("descent_fuel_gph"),
            taxiFuelGal = c.dbl("taxi_fuel_gal"),
            fuelCapacityGal = c.dbl("fuel_capacity_gal"),
            reserveMinutes = c.dbl("reserve_minutes")?.toInt(),
            maxGrossWeightLb = c.dbl("max_gross_weight_lb"),
            forwardCgLimitIn = c.dbl("forward_cg_limit_in"),
            aftCgLimitIn = c.dbl("aft_cg_limit_in"),
            emptyWeightLb = c.dbl("empty_weight_lb"),
            emptyCgIn = c.dbl("empty_cg_in"),
            cruiseAltitudeFt = c.dbl("cruise_altitude_ft"),
            cruisePowerSetting = c.str("cruise_power_setting"),
            verifiedAt = c.str("verified_at"),
            performance = readPerformance(db, id),
        )
    }

    private fun readPerformance(
        db: android.database.sqlite.SQLiteDatabase,
        aircraftId: Long,
    ): AircraftPerformance {
        val byPhase = mutableMapOf<String, MutableList<PerformancePoint>>()
        db.query(
            "aircraft_performance", null, "aircraft_id = ?", arrayOf(aircraftId.toString()),
            null, null, "phase, pressure_altitude_ft",
        ).use { c ->
            while (c.moveToNext()) {
                val phase = c.str("phase") ?: continue
                byPhase.getOrPut(phase) { mutableListOf() } += PerformancePoint(
                    pressureAltitudeFt = c.dbl("pressure_altitude_ft") ?: 0.0,
                    tasKt = c.dbl("tas_kt") ?: 0.0,
                    fuelGph = c.dbl("fuel_gph") ?: 0.0,
                    verticalSpeedFpm = c.dbl("vertical_speed_fpm"),
                    powerSetting = c.str("power_setting") ?: "",
                )
            }
        }
        return AircraftPerformance(
            climb = byPhase[PerformancePhase.CLIMB.wire].orEmpty(),
            cruise = byPhase[PerformancePhase.CRUISE.wire].orEmpty(),
            descent = byPhase[PerformancePhase.DESCENT.wire].orEmpty(),
        )
    }
}

private fun Cursor.str(column: String): String? =
    getColumnIndex(column).takeIf { it >= 0 }?.let { if (isNull(it)) null else getString(it) }

private fun Cursor.dbl(column: String): Double? =
    getColumnIndex(column).takeIf { it >= 0 }?.let { if (isNull(it)) null else getDouble(it) }

private fun Aircraft.toContentValues(): ContentValues = ContentValues().apply {
    put("registration", registration)
    putOrNull("name", name)
    putOrNull("icao_type", icaoType)
    putOrNull("serial_number", serialNumber)
    putOrNull("cruise_tas_kt", cruiseTasKt)
    putOrNull("cruise_fuel_gph", cruiseFuelGph)
    putOrNull("climb_rate_fpm", climbRateFpm)
    putOrNull("climb_tas_kt", climbTasKt)
    putOrNull("climb_fuel_gph", climbFuelGph)
    putOrNull("descent_rate_fpm", descentRateFpm)
    putOrNull("descent_tas_kt", descentTasKt)
    putOrNull("descent_fuel_gph", descentFuelGph)
    putOrNull("taxi_fuel_gal", taxiFuelGal)
    putOrNull("fuel_capacity_gal", fuelCapacityGal)
    putOrNull("reserve_minutes", reserveMinutes?.toDouble())
    putOrNull("max_gross_weight_lb", maxGrossWeightLb)
    putOrNull("forward_cg_limit_in", forwardCgLimitIn)
    putOrNull("aft_cg_limit_in", aftCgLimitIn)
    putOrNull("empty_weight_lb", emptyWeightLb)
    putOrNull("empty_cg_in", emptyCgIn)
    putOrNull("cruise_altitude_ft", cruiseAltitudeFt)
    putOrNull("cruise_power_setting", cruisePowerSetting)
    putOrNull("verified_at", verifiedAt)
}

private fun ContentValues.putOrNull(key: String, value: String?) {
    if (value.isNullOrBlank()) putNull(key) else put(key, value)
}

private fun ContentValues.putOrNull(key: String, value: Double?) {
    if (value == null) putNull(key) else put(key, value)
}
