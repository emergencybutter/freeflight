package ws.freeflight.ui

import android.graphics.Bitmap
import android.graphics.pdf.PdfRenderer
import android.os.ParcelFileDescriptor
import androidx.compose.foundation.Image
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronLeft
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.DarkMode
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.ColorMatrix
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ws.freeflight.data.PlateRepository
import java.io.File

import kotlin.math.max
import kotlin.math.min

/** The plate or diagram to render in the full-screen viewer. */
data class PlateTarget(
    val url: String,
    val title: String,
    val subtitle: String? = null,
)

/**
 * Native PDF Plate Viewer for FAA d-TPP procedure plates and airport diagrams.
 *
 * Reads the PDF from the on-device plate store, falling back to fetching
 * it only when it isn't there — so a plate carried along on the ground
 * opens with no signal. Renders pages with Android's native [PdfRenderer],
 * and supports pinch-to-zoom, panning, double-tap zoom, page navigation,
 * and a night-mode colour inversion for cockpit use.
 */
@Composable
fun PlateViewer(
    target: PlateTarget,
    plates: PlateRepository,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val coroutineScope = rememberCoroutineScope()

    var fileState by remember { mutableStateOf<File?>(null) }
    var isLoading by remember { mutableStateOf(true) }
    var errorMsg by remember { mutableStateOf<String?>(null) }
    var progressText by remember { mutableStateOf("Downloading plate...") }

    var currentPage by remember { mutableIntStateOf(0) }
    var pageCount by remember { mutableIntStateOf(1) }
    var bitmapState by remember { mutableStateOf<Bitmap?>(null) }

    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    var nightMode by remember { mutableStateOf(false) }

    fun loadPlateFile() {
        coroutineScope.launch {
            isLoading = true
            errorMsg = null
            try {
                // Disk first, and without touching the network: a plate
                // taken along on the ground has to open with no signal,
                // which is the whole reason this client exists (§8).
                val local = plates.localPath(target.url)
                if (local != null) {
                    fileState = File(local)
                } else {
                    progressText = "Downloading plate..."
                    val fetched = plates.fetch(target.url) { bytes, total ->
                        if (total > 0) {
                            val pct = (bytes * 100 / total).toInt()
                            progressText = "Downloading plate ($pct%)..."
                        }
                    }
                    fileState = File(fetched)
                }
            } catch (e: Exception) {
                // Naming the remedy matters more than naming the fault:
                // in the air the only useful thing to say is that this one
                // wasn't carried along.
                errorMsg = "This plate isn't on the device and can't be reached right now. " +
                    "Download an airport's plates before you fly to have them without a signal."
            } finally {
                isLoading = false
            }
        }
    }

    LaunchedEffect(target.url) {
        loadPlateFile()
    }

    LaunchedEffect(fileState, currentPage) {
        val file = fileState ?: return@LaunchedEffect
        withContext(Dispatchers.IO) {
            try {
                val pfd = ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
                val renderer = PdfRenderer(pfd)
                pageCount = renderer.pageCount
                if (currentPage >= pageCount) currentPage = 0

                val page = renderer.openPage(currentPage)
                val displayMetrics = context.resources.displayMetrics
                val scaleFactor = min(3f, max(1.5f, displayMetrics.density))
                val targetW = (page.width * scaleFactor).toInt().coerceAtLeast(100)
                val targetH = (page.height * scaleFactor).toInt().coerceAtLeast(100)

                val bitmap = Bitmap.createBitmap(targetW, targetH, Bitmap.Config.ARGB_8888)
                val canvas = android.graphics.Canvas(bitmap)
                canvas.drawColor(android.graphics.Color.WHITE)
                page.render(bitmap, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)

                page.close()
                renderer.close()
                pfd.close()

                withContext(Dispatchers.Main) {
                    bitmapState = bitmap
                }
            } catch (e: Exception) {
                withContext(Dispatchers.Main) {
                    errorMsg = "Error rendering PDF page: ${e.message}"
                }
            }
        }
    }

    Surface(
        color = MaterialTheme.colorScheme.background,
        modifier = modifier.fillMaxSize(),
    ) {
        Box(Modifier.fillMaxSize()) {
            val currentBitmap = bitmapState
            if (currentBitmap != null && !isLoading && errorMsg == null) {
                val nightColorMatrix = remember {
                    ColorMatrix(
                        floatArrayOf(
                            -1f, 0f, 0f, 0f, 255f,
                            0f, -1f, 0f, 0f, 255f,
                            0f, 0f, -1f, 0f, 255f,
                            0f, 0f, 0f, 1f, 0f
                        )
                    )
                }

                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .pointerInput(Unit) {
                            detectTapGestures(
                                onDoubleTap = {
                                    if (scale > 1.2f) {
                                        scale = 1f
                                        offset = Offset.Zero
                                    } else {
                                        scale = 2.5f
                                    }
                                }
                            )
                        }
                        .pointerInput(Unit) {
                            detectTransformGestures { _, pan, zoom, _ ->
                                scale = (scale * zoom).coerceIn(1f, 5f)
                                if (scale > 1f) {
                                    offset += pan
                                } else {
                                    offset = Offset.Zero
                                }
                            }
                        },
                    contentAlignment = Alignment.Center
                ) {
                    Image(
                        bitmap = currentBitmap.asImageBitmap(),
                        contentDescription = target.title,
                        colorFilter = if (nightMode) ColorFilter.colorMatrix(nightColorMatrix) else null,
                        modifier = Modifier
                            .graphicsLayer(
                                scaleX = scale,
                                scaleY = scale,
                                translationX = offset.x,
                                translationY = offset.y
                            )
                    )
                }
            }

            if (isLoading) {
                Column(
                    Modifier.align(Alignment.Center),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    CircularProgressIndicator()
                    Text(progressText, style = MaterialTheme.typography.bodyMedium)
                }
            }

            errorMsg?.let { err ->
                Column(
                    Modifier
                        .align(Alignment.Center)
                        .padding(24.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Text("Could not view instrument plate", style = MaterialTheme.typography.titleMedium)
                    Text(
                        err,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error
                    )
                    TextButton(onClick = { loadPlateFile() }) {
                        Icon(Icons.Default.Refresh, contentDescription = null)
                        Spacer(Modifier.width(4.dp))
                        Text("Retry")
                    }
                }
            }

            Surface(
                color = MaterialTheme.colorScheme.surface.copy(alpha = 0.92f),
                modifier = Modifier
                    .fillMaxWidth()
                    .align(Alignment.TopCenter)
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 8.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    IconButton(onClick = onDismiss) {
                        Icon(Icons.Default.Close, contentDescription = "Close plate")
                    }
                    Column(Modifier.weight(1f).padding(horizontal = 8.dp)) {
                        Text(
                            target.title,
                            style = MaterialTheme.typography.titleSmall,
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.Bold,
                            maxLines = 1,
                        )
                        target.subtitle?.let {
                            Text(
                                it,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                            )
                        }
                    }

                    FilledTonalIconButton(
                        onClick = { nightMode = !nightMode }
                    ) {
                        Icon(
                            Icons.Default.DarkMode,
                            contentDescription = "Toggle night mode",
                            tint = if (nightMode) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }

                    if (pageCount > 1) {
                        Spacer(Modifier.width(4.dp))
                        IconButton(
                            onClick = { if (currentPage > 0) currentPage-- },
                            enabled = currentPage > 0
                        ) {
                            Icon(Icons.Default.ChevronLeft, contentDescription = "Previous page")
                        }
                        Text(
                            "${currentPage + 1}/$pageCount",
                            style = MaterialTheme.typography.labelMedium,
                            fontFamily = FontFamily.Monospace
                        )
                        IconButton(
                            onClick = { if (currentPage < pageCount - 1) currentPage++ },
                            enabled = currentPage < pageCount - 1
                        ) {
                            Icon(Icons.Default.ChevronRight, contentDescription = "Next page")
                        }
                    }
                }
            }
        }
    }
}
