package ws.freeflight.data

import kotlinx.serialization.Serializable

@Serializable
data class RecordedPoint(
    val ts: String,
    val lat: Double,
    val lon: Double,
    val alt_ft: Double,
)

@Serializable
data class PointWithSpeed(
    val point: RecordedPoint,
    val ground_speed_kt: Double,
)

@Serializable
data class PhaseSegmentData(
    val kind: String, // "Ground", "Taxi", "Airborne"
    val start: String,
    val end: String,
)

@Serializable
data class AnalyzedFlightSummary(
    val total_time_seconds: Long,
    val taxi_time_seconds: Long,
    val airborne_time_seconds: Long,
    val landings: List<String> = emptyList(),
    val touch_and_go_count: Int = 0,
    val full_stop_count: Int = 0,
    val max_altitude_ft: Double = 0.0,
    val max_ground_speed_kt: Double = 0.0,
    val distance_flown_nm: Double = 0.0,
    val segments: List<PhaseSegmentData> = emptyList(),
    val points_with_speed: List<PointWithSpeed> = emptyList(),
)

@Serializable
data class RecordedFlight(
    val id: String,
    val name: String,
    val startedAt: String,
    val endedAt: String,
    val points: List<RecordedPoint>,
    val analysis: AnalyzedFlightSummary?,
)
