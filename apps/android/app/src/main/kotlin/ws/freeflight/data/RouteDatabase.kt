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

class RouteDatabaseHelper(context: Context) : SQLiteOpenHelper(context, "freeflight_routes.db", null, 1) {

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
    }

    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        // Future migrations
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
