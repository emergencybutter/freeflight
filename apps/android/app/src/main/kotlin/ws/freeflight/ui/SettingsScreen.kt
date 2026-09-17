package ws.freeflight.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.ff_uniffi.DataSourceCredit

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(viewModel: FreeflightViewModel, modifier: Modifier = Modifier) {
    val apiBaseUrl by viewModel.settings.apiBaseUrl.collectAsState()
    var draft by remember(apiBaseUrl) { mutableStateOf(apiBaseUrl) }
    val cycle by viewModel.cycle.collectAsState()
    var dataOpen by rememberSaveable { mutableStateOf(false) }

    // Cycles and charts are set up rarely and then forgotten about, so they
    // sit one level in from Settings rather than taking a place in the bar
    // next to the things flown with every day.
    if (dataOpen) {
        Scaffold(
            modifier = modifier,
            topBar = {
                TopAppBar(
                    title = { Text("Data & downloads") },
                    navigationIcon = {
                        IconButton(onClick = { dataOpen = false }) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back to settings")
                        }
                    },
                )
            },
        ) { padding ->
            DataScreen(viewModel, Modifier.padding(padding))
        }
        return
    }

    Column(
        modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Disclaimer()

        Card(onClick = { dataOpen = true }) {
            Row(
                Modifier.padding(16.dp).fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text("Data & downloads", style = MaterialTheme.typography.titleMedium)
                    Text(
                        cycle?.let { info ->
                            "Cycle effective ${info.effectiveDate ?: info.cycleId} · " +
                                "${info.airportCount} airports"
                        } ?: "No cycle installed",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Icon(Icons.Default.ChevronRight, contentDescription = null)
            }
        }

        Card {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Server", style = MaterialTheme.typography.titleMedium)
                Text(
                    "Where ff-api runs. Cycle downloads, chart downloads and live weather all " +
                        "come from here; everything already on the device does not.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = draft,
                    onValueChange = { draft = it },
                    singleLine = true,
                    label = { Text("ff-api base URL") },
                    modifier = Modifier.fillMaxWidth(),
                )
                if (draft.trimEnd('/') != apiBaseUrl) {
                    TextButton(onClick = { viewModel.settings.setApiBaseUrl(draft) }) {
                        Text("Save")
                    }
                }
                Text(
                    "Defaults to freeflight.flyvoyager.net. Release builds allow plain HTTP " +
                        "only to the emulator's host loopback (10.0.2.2); anywhere else must " +
                        "be HTTPS.",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        Attributions(viewModel, cycleId = cycle?.cycleId)
    }
}

/**
 * Required, and deliberately not tucked away (DESIGN.md §11).
 */
@Composable
private fun Disclaimer() {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.errorContainer,
            contentColor = MaterialTheme.colorScheme.onErrorContainer,
        )
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("Not for navigation", style = MaterialTheme.typography.titleMedium)
            Text(
                "VFR/IFR supplemental use only. Do not use this application as a primary " +
                    "means of navigation. Verify all data against official sources.",
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

@Composable
private fun Attributions(viewModel: FreeflightViewModel, cycleId: String?) {
    // Re-read whenever the installed cycle changes: the credits ship inside
    // the bundle, so a new cycle can carry different ones.
    val credits by produceState(initialValue = emptyList<DataSourceCredit>(), cycleId) {
        value = withContext(Dispatchers.IO) {
            runCatching { viewModel.attributions() }.getOrDefault(emptyList())
        }
    }

    Card {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Data sources", style = MaterialTheme.typography.titleMedium)
            if (credits.isEmpty()) {
                Text(
                    "No credits recorded in the installed cycle.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            credits.forEach { credit ->
                Column {
                    Text(credit.attribution, style = MaterialTheme.typography.bodyMedium)
                    Text(
                        listOfNotNull(
                            credit.effectiveDate?.let { "effective $it" },
                            credit.licence,
                            credit.url,
                        ).joinToString(" · "),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}
