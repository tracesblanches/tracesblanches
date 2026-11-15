package fr.skimap.app.ui.map

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.maplibre.android.LibraryLoader
import org.maplibre.android.MapLibre
import org.maplibre.android.maps.MapLibreMapOptions
import org.maplibre.android.storage.FileSource

/**
 * MapLibre start-up without blocking the main thread: the 13 MB native library loads and
 * the cache paths resolve on a background thread (about 120 ms on the emulator, flagged by
 * StrictMode when it ran on the main thread), in parallel with the pack opening; only the
 * light `getInstance` runs on the main thread. The map is composed once [ready] is true.
 */
object MapEngine {
    private val mutableReady = MutableStateFlow(false)
    val ready: StateFlow<Boolean> = mutableReady.asStateFlow()

    @Volatile
    private var started = false

    @Volatile
    private var options: MapLibreMapOptions? = null

    /** Options of every map view: north up, no rotation or tilt, no tiny logo buttons. */
    fun mapOptions(): MapLibreMapOptions = checkNotNull(options) { "MapEngine not ready" }

    private const val MIN_ZOOM = 8.0
    private const val MAX_ZOOM = 18.0

    /** Starts MapLibre once per process; [scope] runs on the main thread. */
    fun start(
        context: Context,
        scope: CoroutineScope,
    ) {
        if (started) return
        started = true
        val app = context.applicationContext
        scope.launch {
            withContext(Dispatchers.IO) {
                LibraryLoader.load()
                // Disk reads MapLibre would otherwise do on the main thread (StrictMode):
                // cache paths (preferences, external storage state) and the files directory
                // read by MapLibreMapOptions. Both are cached once resolved.
                FileSource.getResourcesCachePath(app)
                FileSource.getInternalCachePath(app)
                // Map options read from resources here rather than by `MapView(context)`
                // on the main thread (it reads the files directory).
                options =
                    MapLibreMapOptions
                        .createFromAttributes(app)
                        .rotateGesturesEnabled(false)
                        .tiltGesturesEnabled(false)
                        .compassEnabled(false)
                        .logoEnabled(false)
                        .attributionEnabled(false)
                        .minZoomPreference(MIN_ZOOM)
                        .maxZoomPreference(MAX_ZOOM)
            }
            MapLibre.getInstance(app)
            // Every tile, glyph and sprite comes from the pack: never try the network.
            MapLibre.setConnected(false)
            mutableReady.value = true
        }
    }
}
