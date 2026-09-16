package ws.freeflight.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ws.freeflight.data.WeatherHazardTap

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WeatherHazardSheet(
    hazard: WeatherHazardTap,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        modifier = modifier,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 20.dp)
                .padding(bottom = 32.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            when (hazard) {
                is WeatherHazardTap.AirmetTap -> AirmetContent(hazard, onDismiss)
                is WeatherHazardTap.SigmetTap -> SigmetContent(hazard, onDismiss)
                is WeatherHazardTap.CwaTap -> CwaContent(hazard, onDismiss)
                is WeatherHazardTap.PirepTap -> PirepContent(hazard, onDismiss)
            }
        }
    }
}

@Composable
private fun AirmetContent(airmet: WeatherHazardTap.AirmetTap, onDismiss: () -> Unit) {
    val color = when (airmet.hazard) {
        "TURB" -> Color(0xFFFFB020)
        "ICE" -> Color(0xFF4FC3F7)
        "IFR" -> Color(0xFF9B6BD6)
        "MT_OBSC" -> Color(0xFF8A8A8A)
        "FZLVL" -> Color(0xFF7FD0FF)
        "SFC_WND" -> Color(0xFFE0C341)
        else -> Color(0xFFFFB020)
    }

    HeaderRow(
        title = "G-AIRMET ${airmet.hazard}",
        subtitle = if (airmet.product.isNotEmpty()) "Product: ${airmet.product}" else "Tag: ${airmet.tag}",
        badgeText = airmet.hazard,
        badgeColor = color,
        onDismiss = onDismiss,
    )

    HorizontalDivider()

    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        DetailRow("Valid Time", airmet.validTime)
        val altRange = buildString {
            if (airmet.base != null || airmet.top != null) {
                append("${airmet.base ?: "SFC"} to ${airmet.top ?: "UNLTD"}")
            }
            if (airmet.fzlbase != null || airmet.fzltop != null) {
                if (isNotEmpty()) append(" • ")
                append("FZLVL: ${airmet.fzlbase ?: "SFC"}-${airmet.fzltop ?: "UNLTD"}")
            }
        }
        if (altRange.isNotEmpty()) {
            DetailRow("Altitude", altRange)
        }
        airmet.severity?.let { DetailRow("Severity", it) }
        DetailRow("Tag / Identifier", airmet.tag)
    }
}

@Composable
private fun SigmetContent(sigmet: WeatherHazardTap.SigmetTap, onDismiss: () -> Unit) {
    HeaderRow(
        title = "SIGMET ${sigmet.seriesId}",
        subtitle = "Station: ${sigmet.icaoId} • Series ${sigmet.alphaChar}",
        badgeText = sigmet.hazard.ifEmpty { "HAZARD" },
        badgeColor = Color(0xFFE5484D),
        onDismiss = onDismiss,
    )

    HorizontalDivider()

    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        val alts = buildString {
            if (sigmet.altitudeLow1 != null || sigmet.altitudeHi1 != null) {
                val low = sigmet.altitudeLow1?.let { "$it ft" } ?: "SFC"
                val hi = sigmet.altitudeHi1?.let { "$it ft" } ?: "UNLTD"
                append("$low - $hi")
            }
        }
        if (alts.isNotEmpty()) {
            DetailRow("Altitude Range", alts)
        }
        DetailRow("Hazard", sigmet.hazard)
    }

    if (sigmet.rawAirSigmet.isNotEmpty()) {
        Text("Raw Advisory Text", style = MaterialTheme.typography.labelLarge)
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            shape = RoundedCornerShape(8.dp),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(
                sigmet.rawAirSigmet,
                style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                modifier = Modifier.padding(12.dp),
            )
        }
    }
}

@Composable
private fun CwaContent(cwa: WeatherHazardTap.CwaTap, onDismiss: () -> Unit) {
    HeaderRow(
        title = "Center Weather Advisory ${cwa.seriesId}",
        subtitle = "ARTCC / CWSU: ${cwa.cwsu} ${cwa.name}",
        badgeText = cwa.hazard.ifEmpty { "CWA" },
        badgeColor = Color(0xFFFF8C42),
        onDismiss = onDismiss,
    )

    HorizontalDivider()

    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        val alts = buildString {
            if (cwa.base != null || cwa.top != null) {
                val low = cwa.base?.let { "$it ft" } ?: "SFC"
                val hi = cwa.top?.let { "$it ft" } ?: "UNLTD"
                append("$low - $hi")
            }
        }
        if (alts.isNotEmpty()) {
            DetailRow("Altitude Range", alts)
        }
        DetailRow("CWSU", "${cwa.cwsu} - ${cwa.name}")
    }

    if (cwa.rawText.isNotEmpty()) {
        Text("Raw CWA Advisory", style = MaterialTheme.typography.labelLarge)
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            shape = RoundedCornerShape(8.dp),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(
                cwa.rawText,
                style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                modifier = Modifier.padding(12.dp),
            )
        }
    }
}

@Composable
private fun PirepContent(pirep: WeatherHazardTap.PirepTap, onDismiss: () -> Unit) {
    val badgeColor = when (pirep.severity.uppercase()) {
        "SEVERE" -> Color(0xFFE5484D)
        "MODERATE" -> Color(0xFFE0973F)
        "LIGHT" -> Color(0xFFE0C341)
        else -> Color(0xFF7FA8D9)
    }

    HeaderRow(
        title = "Pilot Report (PIREP)",
        subtitle = pirep.summary.ifEmpty { pirep.acType ?: "Report" },
        badgeText = pirep.severity.ifEmpty { "REPORT" },
        badgeColor = badgeColor,
        onDismiss = onDismiss,
    )

    HorizontalDivider()

    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        pirep.acType?.let { DetailRow("Aircraft Type", it) }
        pirep.fltLvl?.let { DetailRow("Flight Level", "FL$it (${it * 100} ft)") }
        DetailRow("Report Severity", pirep.severity)
    }

    if (pirep.rawOb.isNotEmpty()) {
        Text("Raw PIREP Text", style = MaterialTheme.typography.labelLarge)
        Surface(
            color = MaterialTheme.colorScheme.surfaceVariant,
            shape = RoundedCornerShape(8.dp),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(
                pirep.rawOb,
                style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                modifier = Modifier.padding(12.dp),
            )
        }
    }
}

@Composable
private fun HeaderRow(
    title: String,
    subtitle: String,
    badgeText: String,
    badgeColor: Color,
    onDismiss: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Surface(
                    color = badgeColor,
                    shape = RoundedCornerShape(4.dp),
                ) {
                    Text(
                        badgeText,
                        style = MaterialTheme.typography.labelSmall,
                        fontWeight = FontWeight.Bold,
                        color = Color.Black,
                        modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                    )
                }
                Spacer(Modifier.width(8.dp))
                Text(
                    title,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                )
            }
            Spacer(Modifier.height(4.dp))
            Text(
                subtitle,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        IconButton(onClick = onDismiss) {
            Icon(Icons.Default.Close, contentDescription = "Close")
        }
    }
}

@Composable
private fun DetailRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            label,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            value,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.SemiBold,
        )
    }
}
