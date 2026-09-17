package ws.freeflight.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ws.freeflight.data.Aircraft
import ws.freeflight.data.AircraftProfileData
import ws.freeflight.data.PerformancePhase
import ws.freeflight.data.PerformancePoint
import kotlin.math.roundToInt

/**
 * The Aircraft tab: the fleet, its performance figures, POH tables and the
 * loading that follows from them.
 *
 * Weight & balance sits here rather than with the route because it is a
 * property of the aeroplane and what is put in it, not of where it is going —
 * the same load is flown down many routes.
 */
@Composable
fun AircraftScreen(viewModel: FreeflightViewModel, modifier: Modifier = Modifier) {
    val profile by viewModel.aircraftProfile.collectAsState()
    val fleet by viewModel.fleet.collectAsState()
    val selectedAircraftId by viewModel.selectedAircraftId.collectAsState()

    Column(
        modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .padding(top = 16.dp, bottom = 32.dp),
    ) {
        Text(
            "Aircraft",
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(bottom = 12.dp),
        )

        AircraftAndWbBody(
            profile = profile,
            onProfileChange = viewModel::updateProfile,
            fleet = fleet,
            selectedAircraftId = selectedAircraftId,
            onSelectAircraft = viewModel::selectAircraft,
            onSaveAircraft = viewModel::saveAircraft,
            onDeleteAircraft = viewModel::deleteAircraft,
            onAircraftVerifiedChange = viewModel::markAircraftVerified,
            onPerformanceChange = viewModel::setAircraftPerformance,
        )
    }
}

@Composable
private fun AircraftAndWbBody(
    profile: AircraftProfileData,
    onProfileChange: (AircraftProfileData) -> Unit,
    fleet: List<Aircraft>,
    selectedAircraftId: Long?,
    onSelectAircraft: (Long) -> Unit,
    onSaveAircraft: (Aircraft) -> Unit,
    onDeleteAircraft: (Long) -> Unit,
    onAircraftVerifiedChange: (Long, Boolean) -> Unit,
    onPerformanceChange: (Long, PerformancePhase, List<PerformancePoint>) -> Unit,
) {
    val selected = fleet.firstOrNull { it.id == selectedAircraftId } ?: fleet.firstOrNull()

    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (fleet.isNotEmpty()) {
            AircraftPicker(
                fleet = fleet,
                selectedId = selectedAircraftId,
                onSelect = onSelectAircraft,
                onAdd = { onSaveAircraft(Aircraft(registration = "New aircraft")) },
                onDelete = onDeleteAircraft,
                onVerifiedChange = onAircraftVerifiedChange,
            )
            HorizontalDivider()
        }

        Text(
            "Aircraft Performance",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
        )

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            NumberInputField(
                label = "Cruise TAS (kt)",
                value = profile.cruiseTasKt,
                onValueChange = { onProfileChange(profile.copy(cruiseTasKt = it)) },
                modifier = Modifier.weight(1f),
            )
            NumberInputField(
                label = "Fuel Burn (gph)",
                value = profile.fuelBurnGph,
                onValueChange = { onProfileChange(profile.copy(fuelBurnGph = it)) },
                modifier = Modifier.weight(1f),
            )
        }

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            NumberInputField(
                label = "Cruise Altitude (ft)",
                value = profile.cruiseAltitudeFt ?: 5500.0,
                onValueChange = { onProfileChange(profile.copy(cruiseAltitudeFt = it)) },
                modifier = Modifier.weight(1f),
            )
            NumberInputField(
                label = "Reserve (min)",
                value = (profile.reserveMinutes ?: 45).toDouble(),
                onValueChange = { onProfileChange(profile.copy(reserveMinutes = it.toInt())) },
                modifier = Modifier.weight(1f),
            )
        }

        HorizontalDivider()

        Text(
            "Weight & Balance",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
        )

        // Status Card
        Card(
            colors = CardDefaults.cardColors(
                containerColor = if (profile.isOverweight) {
                    MaterialTheme.colorScheme.errorContainer
                } else {
                    MaterialTheme.colorScheme.surfaceContainerHigh
                }
            ),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(14.dp),
                horizontalArrangement = Arrangement.SpaceAround,
            ) {
                SummaryMetric(
                    label = "TOTAL WT",
                    value = "${profile.totalWeightLb.roundToInt()} LB",
                    color = if (profile.isOverweight) MaterialTheme.colorScheme.error else Color.Unspecified,
                )
                SummaryMetric(
                    label = "MAX GROSS",
                    value = "${profile.maxGrossWeightLb.roundToInt()} LB",
                )
                SummaryMetric(
                    label = "C.G.",
                    value = "%.1f\"".format(profile.centerOfGravityIn),
                )
            }
        }

        if (profile.isOverweight) {
            Text(
                "⚠ Aircraft exceeds maximum gross weight (${(profile.totalWeightLb - profile.maxGrossWeightLb).roundToInt()} lb over)",
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.labelMedium,
            )
        }

        selected?.let { aircraft ->
            HorizontalDivider()
            Text(
                "POH tables",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                "Optional. With a table, each leg is planned at its own altitude " +
                    "instead of one cruise number for the whole flight; without one, " +
                    "the figures above are used.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            PerformancePhase.entries.forEach { phase ->
                PerformanceTableEditor(
                    phase = phase,
                    rows = when (phase) {
                        PerformancePhase.CLIMB -> aircraft.performance.climb
                        PerformancePhase.CRUISE -> aircraft.performance.cruise
                        PerformancePhase.DESCENT -> aircraft.performance.descent
                    },
                    onChange = { rows -> onPerformanceChange(aircraft.id, phase, rows) },
                )
            }
            HorizontalDivider()
        }

        Text("Loading", style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.Bold)

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            NumberInputField(
                label = "Pilot (lb)",
                value = profile.weightPilotLb,
                onValueChange = { onProfileChange(profile.copy(weightPilotLb = it)) },
                modifier = Modifier.weight(1f),
            )
            NumberInputField(
                label = "Passenger (lb)",
                value = profile.weightPassengerLb,
                onValueChange = { onProfileChange(profile.copy(weightPassengerLb = it)) },
                modifier = Modifier.weight(1f),
            )
        }

        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            NumberInputField(
                label = "Baggage (lb)",
                value = profile.weightBaggageLb,
                onValueChange = { onProfileChange(profile.copy(weightBaggageLb = it)) },
                modifier = Modifier.weight(1f),
            )
            NumberInputField(
                label = "Fuel (gal)",
                value = profile.gallonsFuel,
                onValueChange = { onProfileChange(profile.copy(gallonsFuel = it)) },
                modifier = Modifier.weight(1f),
            )
        }
    }
}
