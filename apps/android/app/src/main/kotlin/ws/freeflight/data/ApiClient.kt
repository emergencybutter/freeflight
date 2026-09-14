package ws.freeflight.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.io.RandomAccessFile
import java.util.concurrent.TimeUnit

/**
 * Everything this app fetches over the network.
 *
 * Deliberately the only place that talks HTTP — the Rust core takes file
 * paths, never URLs (see `ff-sync`'s `apply` module) — so the one class
 * here is also the one place that knows the app can be offline.
 */
class ApiClient(private val settings: Settings) {

    private val http = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        // Generous, not absent: a cycle bundle is ~145MB and a chart is a
        // similar order, so a read timeout tuned for JSON would abort every
        // real download over a weak connection.
        .readTimeout(60, TimeUnit.SECONDS)
        .callTimeout(0, TimeUnit.MILLISECONDS)
        .build()

    private val json = Json { ignoreUnknownKeys = true; isLenient = true }

    /**
     * Accepts either a path on the configured `ff-api` or an absolute URL.
     * Both occur: the app's own routes are paths, but a manifest's
     * `sqlite_url` may point somewhere else entirely — serving bundles
     * straight from a CDN with `ff-api` only minting manifests is an
     * anticipated deployment (DESIGN.md §11's availability note).
     */
    private fun url(pathOrUrl: String): String =
        if (pathOrUrl.startsWith("http://") || pathOrUrl.startsWith("https://")) pathOrUrl
        else settings.apiBaseUrl.value.trimEnd('/') + pathOrUrl

    /** Raw body of `GET /cycles/latest`, handed to the core to parse (§4.1). */
    suspend fun latestCycleManifestJson(): String = getString("/cycles/latest")

    suspend fun metars(icaoIds: List<String>): List<Metar> {
        if (icaoIds.isEmpty()) return emptyList()
        val body = getString("/weather/metar?ids=" + icaoIds.joinToString(","))
        return json.decodeFromString(body)
    }

    suspend fun tafs(icaoIds: List<String>): List<Taf> {
        if (icaoIds.isEmpty()) return emptyList()
        val body = getString("/weather/taf?ids=" + icaoIds.joinToString(","))
        return json.decodeFromString(body)
    }

    private suspend fun getString(path: String): String = withContext(Dispatchers.IO) {
        http.newCall(Request.Builder().url(url(path)).build()).execute().use { response ->
            if (!response.isSuccessful) {
                throw IOException("${response.code} from $path: ${response.message}")
            }
            response.body?.string().orEmpty()
        }
    }

    /**
     * Streams `path` into [target], resuming where an interrupted attempt
     * left off.
     *
     * Resume is what makes a ~145MB download survive a process death on a
     * phone: the partial file stays on disk, and the next attempt asks for
     * the rest with a `Range` header. A server that ignores the range (any
     * 200 rather than a 206) is handled by starting over rather than by
     * appending to bytes that are already there, which would corrupt the
     * file in a way only the final checksum would catch.
     *
     * [onProgress] is called with bytes-so-far and the total when known —
     * total is -1 when the server sends no length.
     */
    suspend fun download(
        path: String,
        target: File,
        onProgress: (downloaded: Long, total: Long) -> Unit,
    ) = withContext(Dispatchers.IO) {
        target.parentFile?.mkdirs()
        val alreadyHave = if (target.exists()) target.length() else 0L

        val request = Request.Builder()
            .url(url(path))
            .apply { if (alreadyHave > 0) header("Range", "bytes=$alreadyHave-") }
            .build()

        http.newCall(request).execute().use { response ->
            if (!response.isSuccessful) {
                throw IOException("${response.code} from $path: ${response.message}")
            }
            val resuming = response.code == 206
            if (!resuming && alreadyHave > 0) {
                target.delete()
            }
            val startAt = if (resuming) alreadyHave else 0L
            val body = response.body ?: throw IOException("empty response for $path")
            val total = body.contentLength().let { if (it < 0) -1L else it + startAt }

            RandomAccessFile(target, "rw").use { out ->
                out.seek(startAt)
                var written = startAt
                val buffer = ByteArray(64 * 1024)
                body.byteStream().use { input ->
                    while (true) {
                        val read = input.read(buffer)
                        if (read <= 0) break
                        out.write(buffer, 0, read)
                        written += read
                        onProgress(written, total)
                    }
                }
            }
        }
    }
}
