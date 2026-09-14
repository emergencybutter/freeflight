package ws.freeflight.map

import android.util.Log
import uniffi.ff_uniffi.Freeflight
import java.io.BufferedOutputStream
import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.Executors

/**
 * A loopback HTTP server that hands MapLibre the raster tiles inside the
 * device's own PMTiles archives.
 *
 * On web, MapLibre GL JS has a `pmtiles://` protocol handler and reads the
 * archive over HTTP range requests (DESIGN.md §4). The Android SDK has no
 * such hook — a raster source is a URL template and nothing else — and the
 * archive here is a local file anyway, which is the entire point of the
 * offline client (§8). So the app puts an HTTP server where MapLibre
 * expects one and answers out of the Rust core, which already knows how to
 * find a tile inside a PMTiles file.
 *
 * Bound to loopback on an ephemeral port, so it is reachable only from
 * inside this app's own sandbox and cannot collide with anything else on
 * the device. It serves exactly one shape of URL and 404s everything else;
 * there is no filesystem path anywhere in a request, only a chart id the
 * core validates.
 */
class TileServer(private val core: Freeflight) {

    @Volatile
    private var socket: ServerSocket? = null
    private val workers = Executors.newFixedThreadPool(WORKER_THREADS)

    /** Port the server is listening on; 0 until [start] has run. */
    @Volatile
    var port: Int = 0
        private set

    /**
     * The MapLibre raster-source template for one catalogued chart.
     * `{z}/{x}/{y}` are left for MapLibre to substitute.
     */
    fun tileUrlTemplate(chartId: String): String =
        "http://127.0.0.1:$port/chart/$chartId/{z}/{x}/{y}.png"

    fun start() {
        if (socket != null) return
        val server = ServerSocket(0, BACKLOG, InetAddress.getByName("127.0.0.1"))
        socket = server
        port = server.localPort
        Thread({ acceptLoop(server) }, "ff-tile-server").apply { isDaemon = true }.start()
    }

    fun stop() {
        socket?.let { runCatching { it.close() } }
        socket = null
        workers.shutdownNow()
    }

    private fun acceptLoop(server: ServerSocket) {
        while (!server.isClosed) {
            val client = try {
                server.accept()
            } catch (e: IOException) {
                if (!server.isClosed) Log.w(TAG, "accept failed", e)
                return
            }
            workers.execute { serve(client) }
        }
    }

    private fun serve(client: Socket) {
        client.use { connection ->
            connection.soTimeout = SOCKET_TIMEOUT_MS
            val output = BufferedOutputStream(connection.getOutputStream())
            try {
                val requestLine = connection.getInputStream().bufferedReader()
                    .readLine() ?: return
                val target = requestLine.split(' ').getOrNull(1) ?: return
                val tile = parse(target)
                if (tile == null) {
                    respond(output, 404, "text/plain", "not a tile request".toByteArray())
                    return
                }
                val bytes = core.chartTile(tile.chartId, tile.z, tile.x, tile.y)
                if (bytes == null) {
                    // A chart covers a quadrilateral; the map asks for the
                    // whole square around it. A hole is normal, and 404 is
                    // what MapLibre expects for one.
                    respond(output, 404, "text/plain", ByteArray(0))
                } else {
                    respond(output, 200, "image/png", bytes)
                }
            } catch (e: Exception) {
                Log.w(TAG, "tile request failed", e)
                runCatching { respond(output, 500, "text/plain", ByteArray(0)) }
            }
        }
    }

    private data class TileRequest(val chartId: String, val z: UByte, val x: UInt, val y: UInt)

    /** `/chart/<chart id>/<z>/<x>/<y>.png`, and nothing else. */
    private fun parse(target: String): TileRequest? {
        val path = target.substringBefore('?').trim('/').split('/')
        if (path.size != 5 || path[0] != "chart") return null
        val z = path[2].toUByteOrNull() ?: return null
        val x = path[3].toUIntOrNull() ?: return null
        val y = path[4].removeSuffix(".png").toUIntOrNull() ?: return null
        return TileRequest(path[1], z, x, y)
    }

    private fun respond(out: BufferedOutputStream, status: Int, contentType: String, body: ByteArray) {
        val reason = when (status) {
            200 -> "OK"
            404 -> "Not Found"
            else -> "Internal Server Error"
        }
        val header = buildString {
            append("HTTP/1.1 $status $reason\r\n")
            append("Content-Type: $contentType\r\n")
            append("Content-Length: ${body.size}\r\n")
            // Tiles are immutable for the life of a cycle, and the whole
            // archive is already local — MapLibre's own memory cache is the
            // only one worth having in front of this.
            append("Cache-Control: no-store\r\n")
            // One request per connection: simpler than keep-alive, and the
            // connection is a loopback socket, so setup costs nothing.
            append("Connection: close\r\n\r\n")
        }
        out.write(header.toByteArray())
        out.write(body)
        out.flush()
    }

    private companion object {
        const val TAG = "ff.TileServer"
        const val BACKLOG = 32
        // MapLibre fetches a screenful of tiles at once; enough threads to
        // keep a pan fluid, few enough not to thrash the core's one mutex.
        const val WORKER_THREADS = 4
        const val SOCKET_TIMEOUT_MS = 10_000
    }
}
