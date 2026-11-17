package fr.skimap.app.data

import fr.skimap.app.appLang
import fr.skimap.core.Bounds
import fr.skimap.core.MapStyle
import fr.skimap.core.PackEntry
import fr.skimap.core.PackException
import fr.skimap.core.PackInfo
import fr.skimap.core.PackSet
import fr.skimap.core.listPacks
import fr.skimap.core.openMap
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File

/** Packs opened by the Rust core, with a validated style instantiated on them. */
data class OpenedPack(
    val info: PackInfo,
    val style: MapStyle,
    /** Version of the Rust core that opened the packs. */
    val coreVersion: String,
    /** Bounds of each pack shown, in order (free mode: the automatic view is one of them). */
    val packBounds: List<Bounds> = emptyList(),
    /** Name of each pack shown, in order (the style suffix `~p<k>` is pack `k`). */
    val packNames: List<String> = emptyList(),
    /** Packs the core opened (0: free mode over the France basemap alone, no pack installed). */
    val packCount: Int = 1,
)

/** Packs stored on the phone. Every read goes through the Rust reader (crates/ffi). */
interface PackRepository {
    /** Directory holding one sub-directory per pack (`<dir>/<site>/manifest.json`). */
    val packsDir: File

    /** Packs found in [packsDir], sorted by site name; unreadable ones carry their error. */
    suspend fun list(): List<PackEntry>

    /**
     * Opens the packs in [dirs] together (free mode: all of them, or none over the France
     * basemap; area mode: one) and instantiates [styleJson] (a style of `styles/`) on them.
     */
    suspend fun open(
        dirs: List<String>,
        styleJson: String,
        free: Boolean,
    ): OpenedPack

    /** [styleJson] instantiated on the packs opened last. */
    suspend fun style(styleJson: String): MapStyle

    /** Whether the phone has the France basemap (X20): the free mode opens without any pack. */
    suspend fun hasFrance(): Boolean = false
}

/**
 * [PackRepository] backed by the Rust core through UniFFI. Calls run on [io]: the core
 * reads files. One FFI call per opening and per style switch. Keeps one set of packs
 * open and frees the previous one.
 */
class RustPackRepository(
    packsDir: () -> File,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    /** The France basemap (X20), drawn under the packs when the phone has it. */
    franceDir: () -> File? = { null },
) : PackRepository {
    /** Resolved on first use, on [io] (`getExternalFilesDir` touches the disk). */
    override val packsDir: File by lazy(packsDir)

    private val franceDir: File? by lazy(franceDir)

    /** The France basemap directory, when its description is there (installed last). */
    private fun france(): File? = franceDir?.takeIf { File(it, "france.json").isFile }

    override suspend fun hasFrance(): Boolean = withContext(io) { france() != null }

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
        styleJson: String,
        free: Boolean,
    ): OpenedPack =
        withContext(io) {
            mutex.withLock {
                // One call across the FFI: summary and style together.
                val view = openMap(dirs, france()?.absolutePath, styleJson, free, appLang())
                current?.destroy()
                current = view.set
                OpenedPack(view.info, view.style, view.coreVersion, packCount = view.set.len().toInt())
            }
        }

    override suspend fun style(styleJson: String): MapStyle =
        withContext(io) {
            mutex.withLock {
                val set = checkNotNull(current) { "no pack opened" }
                set.mapStyle(styleJson)
            }
        }
}

/** An error of the core, worded on screen in the language of the app (UI layer). */
data class PackProblem(
    val kind: Kind,
    /** Path or technical detail, as the core gives it. */
    val detail: String,
) {
    enum class Kind { NOT_FOUND, INVALID }
}

fun PackException.problem(): PackProblem =
    when (this) {
        is PackException.NotFound -> PackProblem(PackProblem.Kind.NOT_FOUND, path)
        is PackException.Invalid -> PackProblem(PackProblem.Kind.INVALID, detail)
    }
