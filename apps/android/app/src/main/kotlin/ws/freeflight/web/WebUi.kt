package ws.freeflight.web

import android.annotation.SuppressLint
import android.content.res.AssetManager
import android.net.Uri
import android.view.ViewGroup
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import uniffi.ff_uniffi.Freeflight
import ws.freeflight.BuildConfig
import ws.freeflight.container
import java.io.ByteArrayInputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileNotFoundException
import java.io.InputStream
import java.io.RandomAccessFile

/**
 * The app's own origin for the web UI. Every request to it is answered by
 * [AppWebViewClient] — it never reaches DNS or the network — so it only
 * has to be a name nobody else will serve us content under. A subdomain of
 * our own domain guarantees that, and gives the server's CORS allowlist
 * (`FF_WEB_ORIGINS`) one unambiguous entry for "the Android app".
 */
const val WEB_UI_ORIGIN = "https://app.freeflight.butr.cc"
private const val WEB_UI_HOST = "app.freeflight.butr.cc"

/**
 * The website's React UI (apps/web, bundled into the APK's assets at build
 * time) running in a WebView, with its cycle-data requests answered on the
 * device so it works offline.
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun WebUiScreen(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    var webView by remember { mutableStateOf<WebView?>(null) }
    var canGoBack by remember { mutableStateOf(false) }

    BackHandler(enabled = canGoBack) { webView?.goBack() }

    AndroidView(
        modifier = modifier.fillMaxSize(),
        factory = { ctx ->
            WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG)
            WebView(ctx).apply {
                // AndroidView's default is WRAP_CONTENT, and a WebView laid
                // out that way reports a zero-height viewport to CSS: the
                // UI's `height: 100vh` layout collapses to nothing.
                layoutParams = ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT,
                )
                settings.javaScriptEnabled = true
                // The UI keeps the route, map layers, etc. in localStorage.
                settings.domStorageEnabled = true
                webViewClient = AppWebViewClient(
                    core = context.container.core,
                    assets = ctx.assets,
                    remoteBase = BuildConfig.DEFAULT_API_BASE_URL,
                    onHistoryChanged = { view -> canGoBack = view.canGoBack() },
                )
                loadUrl("$WEB_UI_ORIGIN/")
                webView = this
            }
        },
    )
}

/**
 * Serves [WEB_UI_ORIGIN]:
 *  - `/data/…`, `/cycles/latest`: the Rust core, from the installed cycle
 *    (the same `ff-data-api` router the server uses);
 *  - `/local-charts/<id>.pmtiles`: byte ranges of an installed chart;
 *  - everything else: the bundled web build, with SPA fallback to
 *    `index.html`.
 * Requests to any other host (the live API, map basemaps, d-TPP) go to the
 * network as normal.
 */
internal class AppWebViewClient(
    private val core: Freeflight,
    private val assets: AssetManager,
    private val remoteBase: String,
    private val onHistoryChanged: (WebView) -> Unit,
) : WebViewClient() {

    override fun shouldInterceptRequest(
        view: WebView,
        request: WebResourceRequest,
    ): WebResourceResponse? {
        val url = request.url
        if (url.scheme != "https" || url.host != WEB_UI_HOST) return null
        val path = url.path ?: "/"
        return try {
            when {
                path.startsWith("/data/") || path == "/cycles/latest" -> data(url)
                path.startsWith(LOCAL_CHARTS) -> chart(path, request.requestHeaders)
                else -> asset(path)
            }
        } catch (e: Exception) {
            text(500, "${e.javaClass.simpleName}: ${e.message}")
        }
    }

    override fun doUpdateVisitedHistory(view: WebView, url: String?, isReload: Boolean) {
        onHistoryChanged(view)
    }

    private fun data(url: Uri): WebResourceResponse {
        val pathAndQuery = url.encodedPath + (url.encodedQuery?.let { "?$it" } ?: "")
        val response = core.handleWebRequest(pathAndQuery, remoteBase)
        val mime = response.contentType.substringBefore(';').trim()
        return respond(response.status.toInt(), mime, ByteArrayInputStream(response.body))
    }

    /**
     * PMTiles is read with HTTP Range requests (a header, then directory
     * and tile slices), so this answers `Range: bytes=a-b` with a 206 —
     * without it the whole multi-hundred-MB archive would be read per tile.
     */
    private fun chart(path: String, headers: Map<String, String>): WebResourceResponse {
        val chartId = path.removePrefix(LOCAL_CHARTS).removeSuffix(".pmtiles")
        val file = core.chartFile(chartId)?.let(::File)
            ?: return text(404, "chart $chartId is not installed")
        val total = file.length()
        val range = headers.entries
            .firstOrNull { it.key.equals("Range", ignoreCase = true) }
            ?.value
            ?.let { parseRange(it, total) }
            ?: return respond(200, "application/octet-stream", FileInputStream(file), mapOf(
                "Accept-Ranges" to "bytes",
                "Content-Length" to total.toString(),
            ))
        val (start, end) = range
        val bytes = ByteArray((end - start + 1).toInt())
        RandomAccessFile(file, "r").use { raf ->
            raf.seek(start)
            raf.readFully(bytes)
        }
        return respond(206, "application/octet-stream", ByteArrayInputStream(bytes), mapOf(
            "Accept-Ranges" to "bytes",
            "Content-Range" to "bytes $start-$end/$total",
            "Content-Length" to bytes.size.toString(),
        ))
    }

    private fun asset(path: String): WebResourceResponse {
        val requested = path.trimStart('/').ifEmpty { "index.html" }
        // Client-side routes (/settings, /about) have no file; the SPA
        // handles them. Anything with an extension really is missing.
        val isRoute = !requested.substringAfterLast('/').contains('.')
        openAsset(requested)?.let { return respond(200, mimeFor(requested), it) }
        if (isRoute) openAsset("index.html")?.let { return respond(200, "text/html", it) }
        return text(404, "$path not found")
    }

    private fun openAsset(relative: String): InputStream? =
        try {
            assets.open("web/$relative")
        } catch (_: FileNotFoundException) {
            null
        }

    private fun respond(
        status: Int,
        mime: String,
        body: InputStream,
        headers: Map<String, String> = emptyMap(),
    ): WebResourceResponse {
        val textual = mime.startsWith("text/") || mime == "application/json" ||
            mime == "application/javascript"
        return WebResourceResponse(
            mime,
            if (textual) "utf-8" else null,
            status,
            reasonPhrase(status),
            headers,
            body,
        )
    }

    private fun text(status: Int, message: String) =
        respond(status, "text/plain", ByteArrayInputStream(message.toByteArray()))

    companion object {
        private const val LOCAL_CHARTS = "/local-charts/"

        /** `bytes=a-b`, `bytes=a-`, or `bytes=-n`, clamped to the file. */
        fun parseRange(header: String, total: Long): Pair<Long, Long>? {
            val spec = header.trim().removePrefix("bytes=").substringBefore(',').trim()
            val (a, b) = spec.split('-', limit = 2).let { it[0] to it.getOrElse(1) { "" } }
            val (start, end) = when {
                a.isEmpty() -> {
                    val suffix = b.toLongOrNull() ?: return null
                    (total - suffix).coerceAtLeast(0) to total - 1
                }
                else -> {
                    val s = a.toLongOrNull() ?: return null
                    s to (b.toLongOrNull() ?: (total - 1)).coerceAtMost(total - 1)
                }
            }
            return if (start in 0..end) start to end else null
        }

        // WebResourceResponse rejects an empty reason phrase.
        fun reasonPhrase(status: Int) = when (status) {
            200 -> "OK"
            206 -> "Partial Content"
            400 -> "Bad Request"
            404 -> "Not Found"
            501 -> "Not Implemented"
            else -> if (status >= 500) "Server Error" else "Status $status"
        }

        // nginx's stock types lack .mjs (pdf.js's worker) — same gap the
        // server's vhost papers over; .wasm must be exact for streaming
        // compilation of ff-wasm.
        fun mimeFor(name: String) = when (name.substringAfterLast('.', "").lowercase()) {
            "html" -> "text/html"
            "js", "mjs" -> "application/javascript"
            "css" -> "text/css"
            "json", "map" -> "application/json"
            "wasm" -> "application/wasm"
            "svg" -> "image/svg+xml"
            "png" -> "image/png"
            "jpg", "jpeg" -> "image/jpeg"
            "ico" -> "image/x-icon"
            "woff2" -> "font/woff2"
            "woff" -> "font/woff"
            "ttf" -> "font/ttf"
            "txt" -> "text/plain"
            "pdf" -> "application/pdf"
            else -> "application/octet-stream"
        }
    }
}
