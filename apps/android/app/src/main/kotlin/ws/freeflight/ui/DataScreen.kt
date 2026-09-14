package ws.freeflight.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import uniffi.ff_uniffi.Chart
import ws.freeflight.data.ChartDownload
import ws.freeflight.data.ChartSet
import ws.freeflight.data.ChartSetDownload
import ws.freeflight.data.SyncState

/**
 * Where the pilot decides what this device carries.
 *
 * Downloading is always an explicit act here — a cycle bundle is hundreds
 * of megabytes and a chart is tens more (DESIGN.md §11), so nothing on this
 * screen starts a transfer on its own.
 */
@Composable
fun DataScreen(viewModel: FreeflightViewModel, modifier: Modifier = Modifier) {
    val cycle by viewModel.cycle.collectAsState()
    val sync by viewModel.sync.collectAsState()
    val charts by viewModel.charts.collectAsState()
    val downloads by viewModel.chartDownloads.collectAsState()
    val chartSets by viewModel.chartSets.collectAsState()
    val setDownload by viewModel.setDownload.collectAsState()

    LazyColumn(
        modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Card {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text("Data cycle", style = MaterialTheme.typography.titleMedium)
                    if (cycle == null) {
                        Text(
                            "Nothing downloaded. The map, airports and procedures all come " +
                                "from a cycle bundle — until one is installed there is nothing " +
                                "to show, online or off.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    } else {
                        val info = cycle!!
                        Text(
                            "Effective ${info.effectiveDate ?: info.cycleId}",
                            style = MaterialTheme.typography.bodyLarge,
                        )
                        Text(
                            "${info.airportCount} airports · ${info.procedureCount} procedures · " +
                                formatBytes(info.bundleBytes.toLong()),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }

                    SyncStatus(sync)

                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        val busy = sync is SyncState.Downloading ||
                            sync is SyncState.Applying ||
                            sync is SyncState.Checking
                        OutlinedButton(onClick = viewModel::checkForUpdate, enabled = !busy) {
                            Text("Check for update")
                        }
                        Button(onClick = viewModel::downloadLatestCycle, enabled = !busy) {
                            Text(if (cycle == null) "Download cycle" else "Update")
                        }
                    }
                    if (cycle != null) {
                        TextButton(onClick = viewModel::pruneOldCycles) {
                            Text("Free space from older cycles")
                        }
                    }
                }
            }
        }

        setDownload?.let { job ->
            item {
                ChartSetProgress(
                    job = job,
                    onCancel = viewModel::cancelChartSetDownload,
                    onDismiss = viewModel::dismissChartSetDownload,
                )
            }
        }

        if (chartSets.isNotEmpty() && setDownload == null) {
            item {
                Text("Chart sets", style = MaterialTheme.typography.titleMedium)
                Text(
                    "Take a whole group in one go. Charts already on the device are skipped, " +
                        "so after a cycle update this is usually far smaller than it looks.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            items(chartSets, key = { it.name }) { set ->
                ChartSetRow(set = set, onDownload = { viewModel.downloadChartSet(set) })
            }
        }

        item {
            Text(
                "Individual charts",
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(top = 8.dp),
            )
            Text(
                "Each chart works with no network once it is here. Pick which one the map " +
                    "draws from the layers button.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        if (charts.isEmpty()) {
            item {
                Text(
                    if (cycle == null) {
                        "Download a cycle to see the charts it publishes."
                    } else {
                        "This cycle publishes no charts."
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        items(charts, key = { it.id }) { chart ->
            ChartRow(
                chart = chart,
                download = downloads[chart.id],
                onDownload = { viewModel.downloadChart(chart) },
                onCancel = { viewModel.cancelChartDownload(chart.id) },
                onRemove = { viewModel.removeChart(chart.id) },
            )
        }
    }
}

@Composable
private fun SyncStatus(sync: SyncState) {
    when (sync) {
        is SyncState.Idle -> Unit
        is SyncState.Checking -> Text("Checking…", style = MaterialTheme.typography.bodySmall)
        is SyncState.UpToDate -> Text(
            "Up to date.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        is SyncState.UpdateAvailable -> Text(
            "Cycle ${sync.cycleId} is available.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.primary,
        )

        is SyncState.Downloading -> Column {
            Text(
                "Downloading ${sync.cycleId} — ${formatBytes(sync.downloaded)}" +
                    if (sync.total > 0) " of ${formatBytes(sync.total)}" else "",
                style = MaterialTheme.typography.bodySmall,
            )
            Progress(sync.downloaded, sync.total)
        }

        is SyncState.Applying -> Column {
            // Not instant: this is a SHA-256 over the whole bundle before
            // anything is allowed to replace the live cycle.
            Text("Verifying and installing ${sync.cycleId}…", style = MaterialTheme.typography.bodySmall)
            LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 6.dp))
        }

        is SyncState.Failed -> Text(
            sync.message,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.error,
        )
    }
}

/**
 * One bulk option: what it contains, what it still costs, and a single
 * button. The size shown is what is *missing*, not the set total — after a
 * cycle update most of a set is usually already on disk, and quoting the
 * full 20GB would be a lie that stops someone downloading.
 */
@Composable
private fun ChartSetRow(set: ChartSet, onDownload: () -> Unit) {
    Card {
        Row(
            Modifier.padding(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(set.name, style = MaterialTheme.typography.bodyLarge)
                Text(
                    set.description,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    when {
                        set.isComplete -> "All ${set.charts.size} already downloaded"
                        else -> buildString {
                            append("${set.missing.size} to download")
                            // Null means the bundle predates published
                            // sizes; say so rather than imply free.
                            append(
                                set.remainingBytes
                                    ?.let { " · ${formatBytes(it)}" }
                                    ?: " · size unknown"
                            )
                            if (set.installedCount > 0) {
                                append(" · ${set.installedCount} already here")
                            }
                        }
                    },
                    style = MaterialTheme.typography.labelMedium,
                    color = if (set.isComplete) {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    } else {
                        MaterialTheme.colorScheme.primary
                    },
                )
            }
            if (!set.isComplete) {
                Button(onClick = onDownload) { Text("Get") }
            }
        }
    }
}

/** The one progress bar a bulk download reports through. */
@Composable
private fun ChartSetProgress(
    job: ChartSetDownload,
    onCancel: () -> Unit,
    onDismiss: () -> Unit,
) {
    Card {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(job.setName, style = MaterialTheme.typography.titleMedium)
            Text(
                if (job.finished) {
                    "Finished — ${job.completed} of ${job.total}"
                } else {
                    "Chart ${job.completed + 1} of ${job.total}" +
                        (job.currentChart?.let { " · $it" } ?: "")
                },
                style = MaterialTheme.typography.bodySmall,
            )
            Text(
                formatBytes(job.downloadedBytes) +
                    (job.totalBytes?.let { " of ${formatBytes(it)}" } ?: ""),
                style = MaterialTheme.typography.labelMedium,
                fontFamily = FontFamily.Monospace,
            )
            if (!job.finished) {
                Progress(job.downloadedBytes, job.totalBytes ?: -1L)
            }
            // Failures don't stop the run, so they accumulate rather than
            // replacing it: one bad archive out of a hundred should not
            // look like total failure. Only the first few are shown — a
            // run that fails widely would otherwise push the progress bar
            // and the Cancel button off the screen entirely, which is
            // exactly when you want them.
            if (job.failures.isNotEmpty()) {
                Text(
                    "${job.failures.size} failed",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(top = 4.dp),
                )
                job.failures.take(MAX_SHOWN_FAILURES).forEach { failure ->
                    Text(
                        failure,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.error,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                val hidden = job.failures.size - MAX_SHOWN_FAILURES
                if (hidden > 0) {
                    Text(
                        "…and $hidden more",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            TextButton(onClick = if (job.finished) onDismiss else onCancel) {
                Text(if (job.finished) "Done" else "Cancel")
            }
        }
    }
}

@Composable
private fun ChartRow(
    chart: Chart,
    download: ChartDownload?,
    onDownload: () -> Unit,
    onCancel: () -> Unit,
    onRemove: () -> Unit,
) {
    Card {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(chart.name, style = MaterialTheme.typography.bodyLarge)
                    Text(
                        buildString {
                            append(chart.kind)
                            if (chart.installed) append(" · ${formatBytes(chart.installedBytes.toLong())}")
                        },
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                when {
                    download != null && download.failure == null ->
                        TextButton(onClick = onCancel) { Text("Cancel") }

                    chart.installed -> TextButton(onClick = onRemove) { Text("Remove") }
                    else -> Button(onClick = onDownload) { Text("Download") }
                }
            }

            download?.let { progress ->
                if (progress.failure != null) {
                    Text(
                        progress.failure,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                    TextButton(onClick = onDownload) { Text("Retry") }
                } else {
                    Text(
                        formatBytes(progress.downloaded) +
                            if (progress.total > 0) " of ${formatBytes(progress.total)}" else "",
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = FontFamily.Monospace,
                    )
                    Progress(progress.downloaded, progress.total)
                }
            }
        }
    }
}

/** Enough to see the shape of a problem, not enough to bury the controls. */
private const val MAX_SHOWN_FAILURES = 3

/**
 * Determinate when the server sent a length, indeterminate when it didn't —
 * never a fake bar, since a stalled download that looks like it is moving
 * is worse than one that plainly says it doesn't know.
 */
@Composable
private fun Progress(downloaded: Long, total: Long) {
    if (total > 0) {
        LinearProgressIndicator(
            progress = { (downloaded.toFloat() / total.toFloat()).coerceIn(0f, 1f) },
            modifier = Modifier.fillMaxWidth().padding(top = 6.dp),
        )
    } else {
        LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 6.dp))
    }
}
