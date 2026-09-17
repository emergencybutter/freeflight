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
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Card
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import ws.freeflight.data.Aircraft
import ws.freeflight.data.PerformancePhase
import ws.freeflight.data.PerformancePoint

/**
 * Which aircraft the plan is for, and the state of its numbers.
 *
 * The web client manages a fleet behind an account; this one keeps it on
 * the device (see `AircraftRepository`). What both must do is the same:
 * never let a set of book figures pass for this airframe's measured ones.
 * An unverified aircraft is called out here and stays called out until the
 * pilot ticks it off against their POH — a nav log is only as good as the
 * numbers under it, and those arrive as a guess.
 */
@Composable
fun AircraftPicker(
    fleet: List<Aircraft>,
    selectedId: Long?,
    onSelect: (Long) -> Unit,
    onAdd: () -> Unit,
    onDelete: (Long) -> Unit,
    onVerifiedChange: (Long, Boolean) -> Unit,
) {
    val selected = fleet.firstOrNull { it.id == selectedId } ?: fleet.firstOrNull()
    var expanded by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "Aircraft",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.weight(1f),
            )
            IconButton(onClick = onAdd) {
                Icon(Icons.Default.Add, contentDescription = "Add an aircraft")
            }
            if (fleet.size > 1 && selected != null) {
                IconButton(onClick = { scope.launch { onDelete(selected.id) } }) {
                    Icon(Icons.Default.Delete, contentDescription = "Delete this aircraft")
                }
            }
        }

        Card(Modifier.fillMaxWidth().clickable { expanded = true }) {
            Row(
                Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        selected?.displayName ?: "No aircraft",
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    selected?.icaoType?.takeIf { it.isNotBlank() }?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                Icon(Icons.Default.ExpandMore, contentDescription = "Choose an aircraft")
            }
            DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                fleet.forEach { aircraft ->
                    DropdownMenuItem(
                        text = { Text(aircraft.displayName) },
                        onClick = {
                            expanded = false
                            onSelect(aircraft.id)
                        },
                    )
                }
            }
        }

        // The whole point of tracking verification: say it where the
        // numbers are about to be used, not on a settings page.
        selected?.let { aircraft ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    if (aircraft.isVerified) Icons.Default.CheckCircle else Icons.Default.Warning,
                    contentDescription = null,
                    tint = if (aircraft.isVerified) {
                        MaterialTheme.colorScheme.primary
                    } else {
                        MaterialTheme.colorScheme.error
                    },
                    modifier = Modifier.size(16.dp),
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    if (aircraft.isVerified) {
                        "Checked against the POH"
                    } else {
                        "Unverified — these are book figures, not this airframe's"
                    },
                    style = MaterialTheme.typography.labelMedium,
                    color = if (aircraft.isVerified) {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    } else {
                        MaterialTheme.colorScheme.error
                    },
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = { onVerifiedChange(aircraft.id, !aircraft.isVerified) }) {
                    Text(if (aircraft.isVerified) "Unmark" else "I checked it")
                }
            }
        }
    }
}

/**
 * A POH performance table for one phase of flight.
 *
 * `ff-planning` interpolates between the rows and clamps outside them
 * rather than extrapolating, so a short table is honest and a missing one
 * simply falls back to the single cruise figures above. That is why this
 * can be left empty: a pilot who has not typed their POH in still gets a
 * plan, just a coarser one.
 */
@Composable
fun PerformanceTableEditor(
    phase: PerformancePhase,
    rows: List<PerformancePoint>,
    onChange: (List<PerformancePoint>) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "${phase.label} table",
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.weight(1f),
            )
            Text(
                if (rows.isEmpty()) "using single figures" else "${rows.size} rows",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        if (rows.isNotEmpty()) {
            Row {
                HeaderCell("Alt ft", 70.dp)
                if (phase == PerformancePhase.CRUISE) HeaderCell("Power", 70.dp)
                if (phase != PerformancePhase.CRUISE) HeaderCell("FPM", 60.dp)
                HeaderCell("TAS", 60.dp)
                HeaderCell("GPH", 60.dp)
                Spacer(Modifier.width(40.dp))
            }
        }

        rows.forEachIndexed { index, row ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                NumberInputField(
                    label = "",
                    value = row.pressureAltitudeFt,
                    onValueChange = { v ->
                        onChange(rows.replaced(index, row.copy(pressureAltitudeFt = v)))
                    },
                    modifier = Modifier.width(70.dp),
                )
                if (phase == PerformancePhase.CRUISE) {
                    Spacer(Modifier.width(4.dp))
                    PowerSettingField(
                        value = row.powerSetting,
                        onValueChange = { v ->
                            onChange(rows.replaced(index, row.copy(powerSetting = v)))
                        },
                        modifier = Modifier.width(70.dp),
                    )
                } else {
                    Spacer(Modifier.width(4.dp))
                    NumberInputField(
                        label = "",
                        value = row.verticalSpeedFpm ?: 0.0,
                        onValueChange = { v ->
                            onChange(rows.replaced(index, row.copy(verticalSpeedFpm = v)))
                        },
                        modifier = Modifier.width(60.dp),
                    )
                }
                Spacer(Modifier.width(4.dp))
                NumberInputField(
                    label = "",
                    value = row.tasKt,
                    onValueChange = { v -> onChange(rows.replaced(index, row.copy(tasKt = v))) },
                    modifier = Modifier.width(60.dp),
                )
                Spacer(Modifier.width(4.dp))
                NumberInputField(
                    label = "",
                    value = row.fuelGph,
                    onValueChange = { v -> onChange(rows.replaced(index, row.copy(fuelGph = v))) },
                    modifier = Modifier.width(60.dp),
                )
                IconButton(onClick = { onChange(rows.filterIndexed { i, _ -> i != index }) }) {
                    Icon(
                        Icons.Default.Delete,
                        contentDescription = "Remove this row",
                        modifier = Modifier.size(18.dp),
                    )
                }
            }
        }

        OutlinedButton(onClick = {
            val last = rows.lastOrNull()
            onChange(
                rows + PerformancePoint(
                    pressureAltitudeFt = (last?.pressureAltitudeFt ?: 0.0) + 2000.0,
                    tasKt = last?.tasKt ?: 0.0,
                    fuelGph = last?.fuelGph ?: 0.0,
                    verticalSpeedFpm = if (phase == PerformancePhase.CRUISE) null else last?.verticalSpeedFpm ?: 0.0,
                    powerSetting = last?.powerSetting ?: "",
                )
            )
        }) {
            Text("Add ${phase.label.lowercase()} row")
        }
    }
}

@Composable
private fun HeaderCell(text: String, width: androidx.compose.ui.unit.Dp) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.width(width),
    )
}

private fun <T> List<T>.replaced(index: Int, value: T): List<T> =
    mapIndexed { i, existing -> if (i == index) value else existing }
