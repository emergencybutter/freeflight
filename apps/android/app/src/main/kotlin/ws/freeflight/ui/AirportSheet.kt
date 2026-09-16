package ws.freeflight.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import uniffi.ff_uniffi.Procedure

import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Map
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon

/**
 * Everything the bundle knows about one airport, plus its current weather
 * if the network could be reached.
 *
 * The two are visually separated on purpose: the runways, frequencies and
 * procedures came from the installed cycle and are as current as that cycle
 * is, while the METAR came from the network seconds ago or not at all. A
 * pilot has to be able to tell those apart without thinking about it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AirportSheet(
    state: AirportUiState,
    onDismiss: () -> Unit,
    onRefreshWeather: () -> Unit,
    onProcedureSelected: (String) -> Unit,
    onShowOnMap: () -> Unit,
    onViewPlate: ((url: String, title: String, subtitle: String?) -> Unit)? = null,
) {
    val airport = state.detail.airport
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState()) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(bottom = 32.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(
                        airport.icao,
                        style = MaterialTheme.typography.headlineSmall,
                        fontFamily = FontFamily.Monospace,
                    )
                    Text(airport.name, style = MaterialTheme.typography.bodyMedium)
                    Text(
                        listOfNotNull(
                            airport.airportType,
                            "${airport.elevationFt} ft MSL",
                            airport.iata?.let { "IATA $it" },
                        ).joinToString(" · "),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                TextButton(onClick = onShowOnMap) { Text("Centre") }
            }

            state.detail.airportDiagramUrl?.let { diagramUrl ->
                Spacer(Modifier.height(4.dp))
                FilledTonalButton(
                    onClick = {
                        onViewPlate?.invoke(
                            diagramUrl,
                            "${airport.icao} Airport Diagram",
                            airport.name,
                        )
                    }
                ) {
                    Icon(Icons.Default.Map, contentDescription = null)
                    Spacer(Modifier.width(8.dp))
                    Text("View Airport Diagram")
                }
            }

            SectionSpacer()
            WeatherSection(state, onRefreshWeather)

            if (state.detail.runways.isNotEmpty()) {
                SectionSpacer()
                SectionHeader("Runways")
                state.detail.runways.forEach { runway ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                        Text(
                            runway.ident,
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.weight(1f),
                        )
                        Column(horizontalAlignment = Alignment.End) {
                            Text(
                                formatRunwayDimensions(runway.lengthFt, runway.widthFt),
                                style = MaterialTheme.typography.bodyMedium,
                            )
                            Text(
                                runway.surface,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }

            if (state.detail.frequencies.isNotEmpty()) {
                SectionSpacer()
                SectionHeader("Frequencies")
                state.detail.frequencies.forEach { frequency ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
                        Text(frequency.kind, Modifier.weight(1f))
                        Text(
                            formatFrequency(frequency.freqMhz),
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.SemiBold,
                        )
                    }
                    frequency.remarks?.takeIf { it.isNotBlank() }?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }

            if (state.procedures.isNotEmpty()) {
                SectionSpacer()
                SectionHeader("Procedures")
                ProcedureGroup("Approaches", state.procedures, "APPROACH", onProcedureSelected)
                ProcedureGroup("Departures", state.procedures, "SID", onProcedureSelected)
                ProcedureGroup("Arrivals", state.procedures, "STAR", onProcedureSelected)
            }
        }
    }
}

@Composable
private fun WeatherSection(state: AirportUiState, onRefresh: () -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        SectionHeader("Weather", Modifier.weight(1f))
        when {
            state.weatherLoading -> CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
            else -> TextButton(onClick = onRefresh) { Text("Refresh") }
        }
    }

    val metar = state.metar
    when {
        metar != null -> {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                FlightCategoryDot(metar.flightCategory)
                Text(
                    metar.flightCategory ?: "Unknown",
                    style = MaterialTheme.typography.titleSmall,
                )
                state.fetchedAtMillis?.let {
                    Text(
                        "fetched ${formatAge(it)}",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Row(Modifier.fillMaxWidth()) {
                LabelledValue("Wind", metar.windLabel, Modifier.weight(1f))
                LabelledValue("Vis", metar.visibilityLabel, Modifier.weight(1f))
                LabelledValue("Alt", metar.altimeterLabel, Modifier.weight(1f))
            }
            Text(metar.rawText, style = MonoTextStyle, modifier = Modifier.padding(top = 6.dp))
            state.taf?.rawText?.takeIf { it.isNotBlank() }?.let { taf ->
                Text(
                    "TAF",
                    style = MaterialTheme.typography.labelMedium,
                    modifier = Modifier.padding(top = 10.dp),
                )
                Text(taf, style = MonoTextStyle)
            }
        }

        // The distinction that matters: no data because the fetch failed,
        // versus no data because the station has none.
        state.weatherError != null -> Text(
            "Couldn't fetch weather — ${state.weatherError}. " +
                "Everything else on this sheet is from the installed cycle and is unaffected.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.error,
        )

        !state.weatherLoading -> Text(
            "No observation published for this airport.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ProcedureGroup(
    title: String,
    procedures: List<Procedure>,
    kind: String,
    onSelected: (String) -> Unit,
) {
    val matching = procedures.filter { it.kind.equals(kind, ignoreCase = true) }
    if (matching.isEmpty()) return
    Text(
        title,
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(top = 8.dp),
    )
    matching.forEach { procedure ->
        Row(
            Modifier
                .fillMaxWidth()
                .clickable { onSelected(procedure.id) }
                .padding(vertical = 8.dp),
        ) {
            Text(procedure.ident, fontFamily = FontFamily.Monospace, modifier = Modifier.weight(1f))
            procedure.runwayIdent?.let {
                Text(
                    "RWY $it",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        HorizontalDivider()
    }
}

@Composable
private fun LabelledValue(label: String, value: String, modifier: Modifier = Modifier) {
    Column(modifier) {
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(value, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
internal fun SectionHeader(text: String, modifier: Modifier = Modifier) {
    Text(text, style = MaterialTheme.typography.titleSmall, modifier = modifier)
}

@Composable
internal fun SectionSpacer() {
    Spacer(Modifier.height(10.dp))
    HorizontalDivider()
    Spacer(Modifier.height(6.dp))
}
