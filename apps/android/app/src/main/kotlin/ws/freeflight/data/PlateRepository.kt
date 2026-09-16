package ws.freeflight.data

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.ff_uniffi.Freeflight
import uniffi.ff_uniffi.Plate
import java.io.File
import java.net.URLEncoder

/** Progress of a run that takes an airport's plates along, for one progress bar. */
data class PlateDownload(
    val icao: String,
    val completed: Int,
    val total: Int,
    val currentPlate: String?,
    val failures: List<String>,
) {
    val isFinished: Boolean get() = completed >= total
}

/**
 * The d-TPP plates on this device, and the act of putting them there.
 *
 * The cycle bundle carries the plate *index* — every chart's name and URL,
 * offline, from the moment the cycle is installed — but never the PDFs,
 * which run to tens of gigabytes nationwide (§8, same reasoning that keeps
 * chart archives out of the bundle). So a plate is only genuinely
 * available once its PDF has been fetched, and the point of this class is
 * to let a pilot decide *on the ground* which ones to carry.
 *
 * That decision is the difference between a plate viewer and an offline
 * plate viewer. Fetching a plate the moment it is tapped works on a desk
 * and fails in the one place the Android client exists for.
 */
class PlateRepository(
    private val core: Freeflight,
    private val api: ApiClient,
    private val scope: CoroutineScope,
) {
    private val _download = MutableStateFlow<PlateDownload?>(null)
    val download: StateFlow<PlateDownload?> = _download.asStateFlow()

    /** Bumped whenever the store changes, so screens re-read installed state. */
    private val _revision = MutableStateFlow(0)
    val revision: StateFlow<Int> = _revision.asStateFlow()

    private var job: Job? = null

    suspend fun plates(icao: String): List<Plate> = withContext(Dispatchers.IO) {
        runCatching { core.airportPlates(icao) }.getOrDefault(emptyList())
    }

    /** Local path for a plate already here, or null — never a network call. */
    suspend fun localPath(pdfUrl: String): String? = withContext(Dispatchers.IO) {
        core.platePath(pdfUrl)
    }

    suspend fun storedBytes(): Long = withContext(Dispatchers.IO) { core.platesBytes().toLong() }

    suspend fun clear(): Long = withContext(Dispatchers.IO) {
        core.clearPlates().toLong().also { _revision.update { rev -> rev + 1 } }
    }

    /**
     * Fetch one plate and install it, returning its local path.
     *
     * Tries the publisher's URL directly first and falls back to the API's
     * `/dtpp/plate` proxy, which exists because the FAA host is reached
     * live rather than re-hosted (§7) and is not always reachable from a
     * phone's network.
     */
    suspend fun fetch(pdfUrl: String, onProgress: (Long, Long) -> Unit = { _, _ -> }): String {
        core.platePath(pdfUrl)?.let { return it }

        val staged = File(core.plateTargetPath(pdfUrl) + ".partial")
        try {
            api.download(pdfUrl, staged, onProgress)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            staged.delete()
            val proxied = "/dtpp/plate?url=" + URLEncoder.encode(pdfUrl, "UTF-8")
            api.download(proxied, staged, onProgress)
        }
        withContext(Dispatchers.IO) { core.installPlate(pdfUrl, staged.absolutePath) }
        _revision.update { it + 1 }
        return core.platePath(pdfUrl)
            ?: throw IllegalStateException("plate vanished immediately after install")
    }

    /**
     * Take every plate this airport publishes that isn't already here.
     *
     * Sequential, and a failure is recorded rather than fatal — the same
     * shape as a chart-set download, and for the same reason: one
     * unreachable plate out of forty shouldn't cost a pilot the other
     * thirty-nine.
     */
    fun downloadAll(icao: String, plates: List<Plate>) {
        if (job?.isActive == true) return
        val pending = plates.filterNot { it.installed }
        if (pending.isEmpty()) return

        job = scope.launch {
            var done = 0
            val failures = mutableListOf<String>()

            fun publish(current: String?) {
                _download.value = PlateDownload(icao, done, pending.size, current, failures.toList())
            }
            publish(pending.first().chartName)

            for (plate in pending) {
                publish(plate.chartName)
                try {
                    fetch(plate.pdfUrl)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    failures += "${plate.chartName}: ${e.readableMessage()}"
                }
                done++
                publish(null)
            }
            publish(null)
        }
    }

    fun cancel() {
        job?.cancel()
        job = null
        _download.value = null
    }

    fun dismiss() {
        if (job?.isActive == true) return
        _download.value = null
    }
}
