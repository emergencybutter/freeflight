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
import uniffi.ff_uniffi.Chart
import uniffi.ff_uniffi.CycleInfo
import uniffi.ff_uniffi.Freeflight
import java.io.File

/** What the cycle sync is doing, for a UI that must never imply progress it isn't making. */
sealed interface SyncState {
    data object Idle : SyncState
    data object Checking : SyncState
    data object UpToDate : SyncState
    data class UpdateAvailable(val cycleId: String) : SyncState
    data class Downloading(val cycleId: String, val downloaded: Long, val total: Long) : SyncState
    /** Checksumming and swapping in — brief, but not instant for a 145MB file. */
    data class Applying(val cycleId: String) : SyncState
    data class Failed(val message: String) : SyncState
}

/** Per-chart download progress, keyed by `chart_catalog.id`. */
data class ChartDownload(val downloaded: Long, val total: Long, val failure: String? = null)

/**
 * A queued run over several charts, reported as one job.
 *
 * Downloading a set is still N separate transfers — that is what the
 * server publishes — but a pilot who asked for "everything" wants one
 * answer to "how far along is it", not 111. [completed] counts finished
 * charts; [downloadedBytes] is cumulative across the whole run so a single
 * progress bar can be honest about the ~20GB case.
 */
data class ChartSetDownload(
    val setName: String,
    val completed: Int,
    val total: Int,
    val currentChart: String?,
    val downloadedBytes: Long,
    val totalBytes: Long?,
    val failures: List<String> = emptyList(),
) {
    val finished: Boolean get() = completed >= total
}

/**
 * Owns getting aeronautical data onto the device and keeping the UI honest
 * about what is there (DESIGN.md §8, §11).
 *
 * The division of labour with the Rust core is the one `ff-sync`'s `apply`
 * module describes: this class moves bytes, the core verifies and installs
 * them. Nothing here decides whether a bundle is valid, and nothing in the
 * core opens a socket.
 */
class CycleRepository(
    private val core: Freeflight,
    private val api: ApiClient,
    private val scope: CoroutineScope,
) {
    private val _cycle = MutableStateFlow<CycleInfo?>(null)

    /** The installed cycle, or null before the first sync. */
    val cycle: StateFlow<CycleInfo?> = _cycle.asStateFlow()

    private val _sync = MutableStateFlow<SyncState>(SyncState.Idle)
    val sync: StateFlow<SyncState> = _sync.asStateFlow()

    private val _charts = MutableStateFlow<List<Chart>>(emptyList())
    val charts: StateFlow<List<Chart>> = _charts.asStateFlow()

    private val _chartDownloads = MutableStateFlow<Map<String, ChartDownload>>(emptyMap())
    val chartDownloads: StateFlow<Map<String, ChartDownload>> = _chartDownloads.asStateFlow()

    private val _setDownload = MutableStateFlow<ChartSetDownload?>(null)
    val setDownload: StateFlow<ChartSetDownload?> = _setDownload.asStateFlow()

    private var setJob: Job? = null

    private var syncJob: Job? = null
    private val chartJobs = mutableMapOf<String, Job>()

    init {
        scope.launch { refresh() }
    }

    /** Re-read what is installed. Cheap, and the only way the UI learns a swap happened. */
    suspend fun refresh() = withContext(Dispatchers.IO) {
        try {
            _cycle.value = core.currentCycle()
        } catch (e: Exception) {
            // An installed-but-unreadable bundle. Saying so beats showing
            // the first-run empty state, which would claim the device is
            // carrying nothing when it is carrying something broken.
            _cycle.value = null
            _sync.value = SyncState.Failed(
                "The installed cycle can't be read (${e.readableMessage()}). " +
                    "Downloading it again should replace it."
            )
        }
        _charts.value = runCatching { core.charts() }.getOrDefault(emptyList())
    }

    /**
     * Ask `ff-api` whether there is a newer cycle. Only reports what it
     * found — downloading is a separate, explicit act, because it is
     * hundreds of megabytes of someone's mobile data.
     */
    fun checkForUpdate() {
        if (syncJob?.isActive == true) return
        syncJob = scope.launch {
            _sync.value = SyncState.Checking
            _sync.value = try {
                val manifest = core.parseManifest(api.latestCycleManifestJson())
                if (core.isUpdateAvailable(manifest.cycleId)) {
                    SyncState.UpdateAvailable(manifest.cycleId)
                } else {
                    SyncState.UpToDate
                }
            } catch (e: Exception) {
                SyncState.Failed(e.readableMessage())
            }
        }
    }

    /**
     * Download the latest cycle and swap it in.
     *
     * The manifest is re-fetched rather than carried over from
     * [checkForUpdate]: a cycle can be published between the two, and the
     * checksum verified at the end has to be the one that came with the
     * bytes actually downloaded.
     */
    fun downloadLatestCycle() {
        if (syncJob?.isActive == true) return
        syncJob = scope.launch {
            try {
                _sync.value = SyncState.Checking
                val manifest = core.parseManifest(api.latestCycleManifestJson())
                if (!core.isUpdateAvailable(manifest.cycleId)) {
                    _sync.value = SyncState.UpToDate
                    return@launch
                }

                val staged = File(core.downloadsDir(), "${manifest.cycleId}.sqlite.partial")
                _sync.value = SyncState.Downloading(manifest.cycleId, 0, -1)
                api.download(manifest.sqliteUrl, staged) { got, total ->
                    _sync.value = SyncState.Downloading(manifest.cycleId, got, total)
                }

                _sync.value = SyncState.Applying(manifest.cycleId)
                withContext(Dispatchers.IO) {
                    core.applyCycle(manifest.cycleId, staged.absolutePath, manifest.sqliteSha256)
                }
                refresh()
                _sync.value = SyncState.UpToDate
            } catch (e: Exception) {
                _sync.value = SyncState.Failed(e.readableMessage())
            }
        }
    }

    /** Throw away the previous cycle and its charts; returns bytes reclaimed. */
    suspend fun pruneOldCycles(): ULong = withContext(Dispatchers.IO) {
        val freed = core.pruneOldCycles()
        refresh()
        freed
    }

    fun downloadChart(chart: Chart) {
        if (chartJobs[chart.id]?.isActive == true) return
        chartJobs[chart.id] = scope.launch {
            _chartDownloads.update { it + (chart.id to ChartDownload(0, -1)) }
            try {
                val staged = File(core.downloadsDir(), "${chart.id}.pmtiles.partial")
                api.download(chart.tileUrl, staged) { got, total ->
                    _chartDownloads.update { it + (chart.id to ChartDownload(got, total)) }
                }
                withContext(Dispatchers.IO) { core.installChart(chart.id, staged.absolutePath) }
                _chartDownloads.update { it - chart.id }
                refresh()
            } catch (e: Exception) {
                _chartDownloads.update {
                    it + (chart.id to ChartDownload(0, -1, e.readableMessage()))
                }
            }
        }
    }

    /**
     * Download every chart in `set` that isn't already here, one at a
     * time, reported as a single job.
     *
     * Sequential on purpose: these are hundreds of megabytes each, and
     * running them in parallel on a phone buys nothing but contention and
     * a progress bar that lurches. A chart that fails is recorded and the
     * run continues — one bad archive out of a hundred shouldn't abandon
     * the other ninety-nine.
     *
     * Charts already installed are skipped, which after a cycle update is
     * usually most of them: archives are addressed by content, so anything
     * unchanged is already on disk.
     */
    fun downloadChartSet(set: ChartSet) {
        if (setJob?.isActive == true) return
        val pending = set.missing
        if (pending.isEmpty()) return

        setJob = scope.launch {
            var done = 0
            var bytesSoFar = 0L
            val failures = mutableListOf<String>()
            val totalBytes = set.remainingBytes

            fun publish(current: String?, inFlight: Long) {
                _setDownload.value = ChartSetDownload(
                    setName = set.name,
                    completed = done,
                    total = pending.size,
                    currentChart = current,
                    downloadedBytes = bytesSoFar + inFlight,
                    totalBytes = totalBytes,
                    failures = failures.toList(),
                )
            }
            publish(pending.first().name, 0)

            for (chart in pending) {
                try {
                    val staged = File(core.downloadsDir(), "${chart.id}.pmtiles.partial")
                    api.download(chart.tileUrl, staged) { got, _ -> publish(chart.name, got) }
                    withContext(Dispatchers.IO) {
                        core.installChart(chart.id, staged.absolutePath)
                    }
                    bytesSoFar += chart.downloadBytes?.toLong() ?: staged.length()
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    failures += "${chart.name}: ${e.readableMessage()}"
                }
                done++
                publish(null, 0)
                refresh()
            }
            publish(null, 0)
        }
    }

    /** Stop a set download. Partial files stay, so resuming re-uses them. */
    fun cancelChartSetDownload() {
        setJob?.cancel()
        setJob = null
        _setDownload.value = null
    }

    fun dismissChartSetDownload() {
        if (setJob?.isActive == true) return
        _setDownload.value = null
    }

    fun cancelChartDownload(chartId: String) {
        chartJobs.remove(chartId)?.cancel()
        // The partial file is deliberately left behind: the next attempt
        // resumes from it rather than re-fetching what is already there.
        _chartDownloads.update { it - chartId }
    }

    suspend fun removeChart(chartId: String) = withContext(Dispatchers.IO) {
        core.removeChart(chartId)
        refresh()
    }

}

private fun Exception.readableMessage(): String =
    message?.takeIf { it.isNotBlank() } ?: this::class.simpleName ?: "unknown error"
