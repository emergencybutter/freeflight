package ws.freeflight.data

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.io.File
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.UUID

class FlightRecordingRepository(
    private val context: Context,
    private val scope: CoroutineScope,
) {
    private val json = Json { ignoreUnknownKeys = true; isLenient = true; prettyPrint = true }
    private val flightsDir: File = File(context.filesDir, "flights").apply { mkdirs() }

    private val _isRecording = MutableStateFlow(false)
    val isRecording: StateFlow<Boolean> = _isRecording.asStateFlow()

    private val _activePoints = MutableStateFlow<List<RecordedPoint>>(emptyList())
    val activePoints: StateFlow<List<RecordedPoint>> = _activePoints.asStateFlow()

    private val _elapsedSeconds = MutableStateFlow(0L)
    val elapsedSeconds: StateFlow<Long> = _elapsedSeconds.asStateFlow()

    private val _latestPoint = MutableStateFlow<RecordedPoint?>(null)
    val latestPoint: StateFlow<RecordedPoint?> = _latestPoint.asStateFlow()

    private val _latestSpeedKt = MutableStateFlow<Double?>(null)
    val latestSpeedKt: StateFlow<Double?> = _latestSpeedKt.asStateFlow()

    private val _savedFlights = MutableStateFlow<List<RecordedFlight>>(emptyList())
    val savedFlights: StateFlow<List<RecordedFlight>> = _savedFlights.asStateFlow()

    private val _reviewFlight = MutableStateFlow<RecordedFlight?>(null)
    val reviewFlight: StateFlow<RecordedFlight?> = _reviewFlight.asStateFlow()

    private var timerJob: Job? = null
    private var recordStartTime: Long = 0L

    init {
        loadSavedFlights()
    }

    fun startRecording() {
        if (_isRecording.value) return
        _activePoints.value = emptyList()
        _elapsedSeconds.value = 0L
        _latestPoint.value = null
        _latestSpeedKt.value = null
        recordStartTime = System.currentTimeMillis()
        _isRecording.value = true

        timerJob?.cancel()
        timerJob = scope.launch(Dispatchers.Default) {
            while (isActive && _isRecording.value) {
                delay(1000L)
                _elapsedSeconds.value = (System.currentTimeMillis() - recordStartTime) / 1000L
            }
        }
    }

    fun addPoint(lat: Double, lon: Double, altFt: Double, timestampMs: Long, speedKt: Double? = null) {
        if (!_isRecording.value) return
        val ts = DateTimeFormatter.ISO_INSTANT.format(Instant.ofEpochMilli(timestampMs))
        val point = RecordedPoint(ts = ts, lat = lat, lon = lon, alt_ft = altFt)

        val updated = _activePoints.value.toMutableList()
        updated.add(point)
        _activePoints.value = updated
        _latestPoint.value = point
        if (speedKt != null && speedKt >= 0.0) {
            _latestSpeedKt.value = speedKt
        }
    }

    fun stopRecording(): RecordedFlight? {
        if (!_isRecording.value) return null
        _isRecording.value = false
        timerJob?.cancel()
        timerJob = null

        val points = _activePoints.value
        val endedAt = DateTimeFormatter.ISO_INSTANT.format(Instant.now())
        val startedAt = points.firstOrNull()?.ts ?: DateTimeFormatter.ISO_INSTANT.format(Instant.ofEpochMilli(recordStartTime))

        val flightId = UUID.randomUUID().toString()
        val flightName = "Flight " + DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm")
            .withZone(ZoneOffset.systemDefault())
            .format(Instant.ofEpochMilli(recordStartTime))

        var analysis: AnalyzedFlightSummary? = null
        if (points.size >= 2) {
            try {
                val pointsJson = json.encodeToString(points)
                val analyzedJson = uniffi.ff_uniffi.analyzeTrackJson(pointsJson)
                analysis = json.decodeFromString<AnalyzedFlightSummary>(analyzedJson)
            } catch (e: Exception) {
                // Analysis error fallback
            }
        }

        val flight = RecordedFlight(
            id = flightId,
            name = flightName,
            startedAt = startedAt,
            endedAt = endedAt,
            points = points,
            analysis = analysis,
        )

        saveFlightToDisk(flight)
        loadSavedFlights()
        _reviewFlight.value = flight
        _activePoints.value = emptyList()
        return flight
    }

    fun selectFlightForReview(flight: RecordedFlight?) {
        _reviewFlight.value = flight
    }

    fun loadSavedFlights() {
        scope.launch(Dispatchers.IO) {
            val files = flightsDir.listFiles { f -> f.extension == "json" } ?: emptyArray()
            val list = files.mapNotNull { file ->
                runCatching {
                    json.decodeFromString<RecordedFlight>(file.readText())
                }.getOrNull()
            }.sortedByDescending { it.startedAt }
            _savedFlights.value = list
        }
    }

    fun deleteFlight(id: String) {
        scope.launch(Dispatchers.IO) {
            val file = File(flightsDir, "$id.json")
            if (file.exists()) {
                file.delete()
            }
            if (_reviewFlight.value?.id == id) {
                _reviewFlight.value = null
            }
            loadSavedFlights()
        }
    }

    private fun saveFlightToDisk(flight: RecordedFlight) {
        scope.launch(Dispatchers.IO) {
            try {
                val file = File(flightsDir, "${flight.id}.json")
                file.writeText(json.encodeToString(flight))
            } catch (e: Exception) {
                // Ignore file write errors
            }
        }
    }

    fun exportGpx(flight: RecordedFlight): String {
        val pointsJson = json.encodeToString(flight.points)
        return uniffi.ff_uniffi.exportTrackGpx(pointsJson, flight.name)
    }

    fun exportCsv(flight: RecordedFlight): String {
        val analysis = flight.analysis ?: return ""
        val analysisJson = json.encodeToString(analysis)
        return uniffi.ff_uniffi.exportFlightCsv(analysisJson, flight.name)
    }
}
