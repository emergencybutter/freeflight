package ws.freeflight.data

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.time.Instant
import java.time.format.DateTimeFormatter

data class SavedRoutePlan(
    val id: Long,
    val name: String,
    val createdAt: String,
    val waypoints: List<PlannedWaypoint>,
) {
    val summaryText: String
        get() = if (waypoints.isNotEmpty()) waypoints.joinToString(" → ") { it.ident } else "Empty"
}

class RouteDatabaseHelper(context: Context) : SQLiteOpenHelper(context, "freeflight_routes.db", null, DB_VERSION) {

    override fun onConfigure(db: SQLiteDatabase) {
        db.setForeignKeyConstraintsEnabled(true)
    }

    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL(
            """
            CREATE TABLE IF NOT EXISTS route_plan (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            """.trimIndent()
        )
        db.execSQL(
            """
            CREATE TABLE IF NOT EXISTS route_leg (
                route_plan_id INTEGER NOT NULL,
                seq INTEGER NOT NULL,
                ident TEXT NOT NULL,
                name TEXT,
                lat REAL NOT NULL,
                lon REAL NOT NULL,
                PRIMARY KEY (route_plan_id, seq),
                FOREIGN KEY (route_plan_id) REFERENCES route_plan(id) ON DELETE CASCADE
            );
            """.trimIndent()
        )
        db.execSQL(
            """
            CREATE TABLE IF NOT EXISTS active_route (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                profile_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            """.trimIndent()
        )
        db.execSQL(
            """
            CREATE TABLE IF NOT EXISTS active_leg (
                seq INTEGER PRIMARY KEY,
                ident TEXT NOT NULL,
                name TEXT,
                lat REAL NOT NULL,
                lon REAL NOT NULL
            );
            """.trimIndent()
        )
        // A fresh install must end up with exactly what an upgraded one
        // has, so the fleet tables are created from the same function
        // onUpgrade uses rather than a second copy of the DDL.
        createFleetTables(db)
    }

    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        // Additive only, and applied in order, so a device that skipped a
        // version still lands in the same place. A saved route is a
        // pilot's own work — dropping and recreating would be the easy
        // path and the wrong one.
        if (oldVersion < 2) createFleetTables(db)
    }

    /**
     * The on-device fleet.
     *
     * Columns mirror `ff-accounts`' `aircraft` table one-to-one (see
     * `apps/web/src/aircraft.ts`) even though nothing syncs yet: the web
     * client keeps a fleet behind an account, this client has no auth, and
     * a differently-shaped local table would guarantee a migration on the
     * day those are joined up. `remote_id` is the hook for that day —
     * null for everything created here.
     */
    private fun createFleetTables(db: SQLiteDatabase) {
        db.execSQL(
            """
            CREATE TABLE IF NOT EXISTS aircraft (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                remote_id            INTEGER,
                registration         TEXT NOT NULL,
                name                 TEXT,
                icao_type            TEXT,
                serial_number        TEXT,
                cruise_tas_kt        REAL,
                cruise_fuel_gph      REAL,
                climb_rate_fpm       REAL,
                climb_tas_kt         REAL,
                climb_fuel_gph       REAL,
                descent_rate_fpm     REAL,
                descent_tas_kt       REAL,
                descent_fuel_gph     REAL,
                taxi_fuel_gal        REAL,
                fuel_capacity_gal    REAL,
                reserve_minutes      INTEGER,
                max_gross_weight_lb  REAL,
                forward_cg_limit_in  REAL,
                aft_cg_limit_in      REAL,
                empty_weight_lb      REAL,
                empty_cg_in          REAL,
                cruise_altitude_ft   REAL,
                cruise_power_setting TEXT,
                verified_at          TEXT,
                created_at           TEXT NOT NULL,
                updated_at           TEXT NOT NULL
            )
            """.trimIndent()
        )
        db.execSQL(
            """
            CREATE TABLE IF NOT EXISTS aircraft_performance (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                aircraft_id          INTEGER NOT NULL REFERENCES aircraft(id) ON DELETE CASCADE,
                phase                TEXT NOT NULL,
                pressure_altitude_ft REAL NOT NULL,
                power_setting        TEXT NOT NULL DEFAULT '',
                vertical_speed_fpm   REAL,
                tas_kt               REAL NOT NULL,
                fuel_gph             REAL NOT NULL
            )
            """.trimIndent()
        )
        db.execSQL(
            "CREATE INDEX IF NOT EXISTS idx_aircraft_performance ON " +
                "aircraft_performance(aircraft_id, phase, pressure_altitude_ft)"
        )
        // One aircraft is selected for planning at a time.
        db.execSQL(
            """
            CREATE TABLE IF NOT EXISTS selected_aircraft (
                id          INTEGER PRIMARY KEY CHECK (id = 1),
                aircraft_id INTEGER REFERENCES aircraft(id) ON DELETE SET NULL
            )
            """.trimIndent()
        )
    }

    companion object {
        /** 2 added the on-device fleet (`aircraft`, performance tables). */
        const val DB_VERSION = 2
    }
}

class RoutePlanRepository(
    context: Context,
    private val scope: CoroutineScope,
) {
    private val dbHelper = RouteDatabaseHelper(context.applicationContext)
    private val json = Json { ignoreUnknownKeys = true; isLenient = true }

    private val _savedPlans = MutableStateFlow<List<SavedRoutePlan>>(emptyList())
    val savedPlans: StateFlow<List<SavedRoutePlan>> = _savedPlans.asStateFlow()

    init {
        scope.launch(Dispatchers.IO) {
            refreshSavedPlans()
        }
    }

    suspend fun loadActiveRoute(): Pair<List<PlannedWaypoint>, AircraftProfileData>? = withContext(Dispatchers.IO) {
        val db = dbHelper.readableDatabase
        var profile: AircraftProfileData? = null

        db.query(
            "active_route",
            arrayOf("profile_json"),
            "id = 1",
            null,
            null,
            null,
            null,
        ).use { cursor ->
            if (cursor.moveToFirst()) {
                val profileJson = cursor.getString(0)
                profile = runCatching { json.decodeFromString<AircraftProfileData>(profileJson) }.getOrNull()
            }
        }

        val waypoints = mutableListOf<PlannedWaypoint>()
        db.query(
            "active_leg",
            arrayOf("seq", "ident", "name", "lat", "lon"),
            null,
            null,
            null,
            null,
            "seq ASC",
        ).use { cursor ->
            while (cursor.moveToNext()) {
                val ident = cursor.getString(1)
                val name = cursor.getString(2)
                val lat = cursor.getDouble(3)
                val lon = cursor.getDouble(4)
                waypoints.add(PlannedWaypoint(ident, name, lat, lon))
            }
        }

        if (waypoints.isEmpty() && profile == null) {
            null
        } else {
            Pair(waypoints, profile ?: AircraftProfileData())
        }
    }

    suspend fun saveActiveRoute(waypoints: List<PlannedWaypoint>, profile: AircraftProfileData) = withContext(Dispatchers.IO) {
        val db = dbHelper.writableDatabase
        db.beginTransaction()
        try {
            val cv = ContentValues().apply {
                put("id", 1)
                put("profile_json", json.encodeToString(profile))
                put("updated_at", DateTimeFormatter.ISO_INSTANT.format(Instant.now()))
            }
            db.insertWithOnConflict("active_route", null, cv, SQLiteDatabase.CONFLICT_REPLACE)

            db.delete("active_leg", null, null)
            waypoints.forEachIndexed { seq, wp ->
                val legCv = ContentValues().apply {
                    put("seq", seq)
                    put("ident", wp.ident)
                    put("name", wp.name)
                    put("lat", wp.lat)
                    put("lon", wp.lon)
                }
                db.insert("active_leg", null, legCv)
            }
            db.setTransactionSuccessful()
        } finally {
            db.endTransaction()
        }
    }

    suspend fun clearActiveRoute() = withContext(Dispatchers.IO) {
        val db = dbHelper.writableDatabase
        db.beginTransaction()
        try {
            db.delete("active_route", null, null)
            db.delete("active_leg", null, null)
            db.setTransactionSuccessful()
        } finally {
            db.endTransaction()
        }
    }

    suspend fun saveNamedPlan(name: String, waypoints: List<PlannedWaypoint>): SavedRoutePlan = withContext(Dispatchers.IO) {
        val db = dbHelper.writableDatabase
        val now = DateTimeFormatter.ISO_INSTANT.format(Instant.now())
        db.beginTransaction()
        val planId = try {
            val planCv = ContentValues().apply {
                put("name", name)
                put("created_at", now)
            }
            val id = db.insert("route_plan", null, planCv)

            waypoints.forEachIndexed { seq, wp ->
                val legCv = ContentValues().apply {
                    put("route_plan_id", id)
                    put("seq", seq)
                    put("ident", wp.ident)
                    put("name", wp.name)
                    put("lat", wp.lat)
                    put("lon", wp.lon)
                }
                db.insert("route_leg", null, legCv)
            }
            db.setTransactionSuccessful()
            id
        } finally {
            db.endTransaction()
        }

        refreshSavedPlans()
        SavedRoutePlan(planId, name, now, waypoints)
    }

    suspend fun deleteSavedPlan(id: Long) = withContext(Dispatchers.IO) {
        val db = dbHelper.writableDatabase
        db.delete("route_plan", "id = ?", arrayOf(id.toString()))
        refreshSavedPlans()
    }

    private fun refreshSavedPlans() {
        val db = dbHelper.readableDatabase
        val plans = mutableListOf<SavedRoutePlan>()

        db.query(
            "route_plan",
            arrayOf("id", "name", "created_at"),
            null,
            null,
            null,
            null,
            "id DESC",
        ).use { planCursor ->
            while (planCursor.moveToNext()) {
                val planId = planCursor.getLong(0)
                val planName = planCursor.getString(1)
                val createdAt = planCursor.getString(2)

                val waypoints = mutableListOf<PlannedWaypoint>()
                db.query(
                    "route_leg",
                    arrayOf("seq", "ident", "name", "lat", "lon"),
                    "route_plan_id = ?",
                    arrayOf(planId.toString()),
                    null,
                    null,
                    "seq ASC",
                ).use { legCursor ->
                    while (legCursor.moveToNext()) {
                        waypoints.add(
                            PlannedWaypoint(
                                ident = legCursor.getString(1),
                                name = legCursor.getString(2),
                                lat = legCursor.getDouble(3),
                                lon = legCursor.getDouble(4),
                            )
                        )
                    }
                }
                plans.add(SavedRoutePlan(planId, planName, createdAt, waypoints))
            }
        }
        _savedPlans.value = plans
    }
}
