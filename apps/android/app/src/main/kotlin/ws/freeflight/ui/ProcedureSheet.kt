package ws.freeflight.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import uniffi.ff_uniffi.ProcedureDetail
import uniffi.ff_uniffi.ProcedureTransition

/**
 * One procedure's leg table, transition by transition, with the path drawn
 * on the map behind the sheet.
 *
 * The legs are shown as ARINC 424 records — path/terminator, fix, course,
 * constraints — rather than prose. That is what the procedure *is*, and a
 * pilot cross-checking the app against a plate needs the same columns the
 * plate's coding table has.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProcedureSheet(detail: ProcedureDetail, onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState()) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp)
                .padding(bottom = 32.dp),
        ) {
            Text(
                detail.procedure.ident,
                style = MaterialTheme.typography.headlineSmall,
                fontFamily = FontFamily.Monospace,
            )
            Text(
                listOfNotNull(
                    detail.procedure.airportIcao,
                    detail.procedure.kind,
                    detail.procedure.runwayIdent?.let { "RWY $it" },
                ).joinToString(" · "),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            detail.chartName?.let { name ->
                Text(
                    "Plate: $name",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 4.dp),
                )
            }

            detail.transitions.forEach { transition ->
                SectionSpacer()
                TransitionTable(transition)
            }
        }
    }
}

@Composable
private fun TransitionTable(transition: ProcedureTransition) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        // The two labels are different type sizes; without this they sit on
        // their own baselines and read as one overlapping string.
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(transition.ident, style = MaterialTheme.typography.titleSmall)
        Text(
            transition.kind,
            style = MaterialTheme.typography.labelMedium,
            color = if (transition.kind.equals("MISSED", ignoreCase = true)) {
                MaterialTheme.colorScheme.secondary
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
        )
    }

    Row(Modifier.fillMaxWidth().padding(top = 6.dp)) {
        HeaderCell("FIX", 90.dp)
        HeaderCell("P/T", 46.dp)
        HeaderCell("CRS", 52.dp)
        Text(
            "ALT / SPD",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    HorizontalDivider()

    transition.legs.forEach { leg ->
        Row(Modifier.fillMaxWidth().padding(vertical = 5.dp)) {
            Text(
                leg.fixIdent ?: "—",
                style = MonoTextStyle,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.width(90.dp),
            )
            Text(leg.pathAndTerm, style = MonoTextStyle, modifier = Modifier.width(46.dp))
            Text(
                leg.courseDeg?.let { "${it.toInt().toString().padStart(3, '0')}°" } ?: "—",
                style = MonoTextStyle,
                modifier = Modifier.width(52.dp),
            )
            Text(
                listOfNotNull(leg.altitudeConstraint, leg.speedConstraint)
                    .joinToString(" / ")
                    .ifBlank { "—" },
                style = MonoTextStyle,
            )
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
