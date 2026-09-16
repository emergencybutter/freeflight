package ws.freeflight

import android.app.Application
import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.Dispatchers
import ws.freeflight.data.ApiClient
import ws.freeflight.data.CycleRepository
import ws.freeflight.data.Settings
import ws.freeflight.map.TileServer
import uniffi.ff_uniffi.Freeflight

/**
 * The process-wide objects, built once and handed down.
 *
 * Small enough not to want a DI framework, but explicit about lifetime:
 * [core] holds an open SQLite connection and any open chart archives, and
 * [tileServer] holds a listening socket, so there must be exactly one of
 * each for the life of the process — not one per Activity.
 */
class AppContainer(context: Context) {
    val settings = Settings(context)

    /**
     * The Rust core, rooted at the app's private files directory. Nothing
     * is opened here: a first run with no cycle downloaded is a normal
     * state that the UI has to render, not a startup failure.
     */
    val core: Freeflight = Freeflight(context.filesDir.absolutePath)

    val api = ApiClient(settings)

    /**
     * Survives the Activity, so a cycle download in progress is not
     * cancelled by a rotation or by the user switching apps. It does not
     * survive process death — the downloader resumes with a `Range`
     * request for that case.
     */
    val appScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    val cycles = CycleRepository(core, api, appScope)

    val flightRecording = ws.freeflight.data.FlightRecordingRepository(context, appScope)

    val tileServer = TileServer(core).also { it.start() }
}


class FreeflightApp : Application() {
    lateinit var container: AppContainer
        private set

    override fun onCreate() {
        super.onCreate()
        container = AppContainer(this)
    }
}

val Context.container: AppContainer
    get() = (applicationContext as FreeflightApp).container
