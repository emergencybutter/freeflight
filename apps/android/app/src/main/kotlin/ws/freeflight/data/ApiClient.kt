package ws.freeflight.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
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
        http.newCall(request(path).build()).execute().use { response ->
            if (!response.isSuccessful) {
                throw response.toException(path)
            }
            response.body?.string().orEmpty()
        }
    }

    /**
     * Every request declares the API contract this build was written
     * against (DESIGN.md §4.1).
     *
     * An app on a phone cannot be updated in lockstep with the server, and
     * the failure that matters is the silent one: without this, a server
     * that changed a response shape would hand an old APK something it
     * misparses, and the app would show a pilot something plausible and
     * wrong. Declaring the version turns that into a refusal the app can
     * report — see [ApiException.OutdatedClient].
     */
    private fun request(pathOrUrl: String): Request.Builder =
        Request.Builder()
            .url(url(pathOrUrl))
            .header(API_VERSION_HEADER, API_VERSION)

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

        val request = request(path)
            .apply { if (alreadyHave > 0) header("Range", "bytes=$alreadyHave-") }
            .build()

        http.newCall(request).execute().use { response ->
            if (!response.isSuccessful) {
                throw response.toException(path)
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

/**
 * The API contract this build speaks (DESIGN.md §4.1). Bump only when this
 * client is updated to a new one.
 */
private const val API_VERSION = "1"
private const val API_VERSION_HEADER = "X-Freeflight-Api-Version"

/** Errors this client distinguishes, because the UI says different things. */
sealed class ApiException(message: String) : IOException(message) {
    /**
     * The server has moved on and no longer serves this build's contract
     * (HTTP 426). Not a network blip and not retryable: nothing improves
     * until the app is updated, so the UI has to say that rather than
     * offer to try again.
     */
    class OutdatedClient(val detail: String) : ApiException(
        "this version of the app is too old for the server: $detail"
    )

    class Failed(message: String) : ApiException(message)
}

private fun Response.toException(path: String): ApiException = when (code) {
    // 426 Upgrade Required — version negotiation refusing this build.
    426 -> ApiException.OutdatedClient(
        body?.string()?.trim().orEmpty().ifEmpty { message }
    )

    else -> ApiException.Failed("$code from $path: $message")
}
