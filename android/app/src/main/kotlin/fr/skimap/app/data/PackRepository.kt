package fr.skimap.app.data

import fr.skimap.core.LegendEntry
import fr.skimap.core.MapStyle
import fr.skimap.core.MapSymbol
import fr.skimap.core.PackEntry
import fr.skimap.core.PackException
import fr.skimap.core.PackInfo
import fr.skimap.core.PackSet
import fr.skimap.core.listPacks
import fr.skimap.core.openPacks
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File

/** A pack opened by the Rust core, with the style of one of its basemaps. */
data class OpenedPack(
    val info: PackInfo,
    val style: MapStyle,
    val legend: List<LegendEntry>,
    /** Images the style refers to (drawn by the app, defined by the core). */
    val symbols: List<MapSymbol>,
    /** Version of the Rust core that opened the pack. */
    val coreVersion: String,
)

/** Packs stored on the phone. Every read goes through the Rust reader (crates/ffi). */
interface PackRepository {
    /** Directory holding one sub-directory per pack (`<dir>/<site>/manifest.json`). */
    val packsDir: File

    /** Packs found in [packsDir], sorted by site name; unreadable ones carry their error. */
    suspend fun list(): List<PackEntry>

    /**
     * Opens the packs in [dirs] together (free mode: all of them; area mode: one) with
     * [basemap] (`null`: the first basemap of the first pack).
     */
    suspend fun open(
        dirs: List<String>,
        basemap: String?,
    ): OpenedPack

    /** Style of the currently opened pack with another basemap. */
    suspend fun style(basemap: String): MapStyle
}

/**
 * [PackRepository] backed by the Rust core through UniFFI. Calls run on [io]: the core
 * reads files. One FFI call per pack opened and per basemap switch. Keeps one pack handle
 * open and frees the previous one.
 */
class RustPackRepository(
    packsDir: () -> File,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : PackRepository {
    /** Resolved on first use, on [io] (`getExternalFilesDir` touches the disk). */
    override val packsDir: File by lazy(packsDir)

    private val mutex = Mutex()
    private var current: PackSet? = null

    override suspend fun list(): List<PackEntry> =
        withContext(io) {
            // Created by the app, so that packs copied into it by adb stay readable.
            packsDir.mkdirs()
            listPacks(packsDir.absolutePath)
        }

    override suspend fun open(
        dirs: List<String>,
        basemap: String?,
    ): OpenedPack =
        withContext(io) {
            mutex.withLock {
                // One call across the FFI: summary, style, legend and symbols together.
                val view = openPacks(dirs, basemap)
                current?.destroy()
                current = view.set
                OpenedPack(view.info, view.style, view.legend, view.symbols, view.coreVersion)
            }
        }

    override suspend fun style(basemap: String): MapStyle =
        withContext(io) {
            mutex.withLock {
                val handle = checkNotNull(current) { "no pack opened" }
                handle.mapStyle(basemap)
            }
        }
}

/** French message of a core error, for the screen. */
fun PackException.describe(): String =
    when (this) {
        is PackException.NotFound -> "Aucun pack dans $path"
        is PackException.Invalid -> "Pack invalide : $detail"
        is PackException.UnknownBasemap -> "Fond de carte « $id » absent du pack"
    }
