package fr.skimap.app.ui

import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import fr.skimap.app.data.OpenedPack
import fr.skimap.app.data.PackRepository
import fr.skimap.app.data.SettingsStore
import fr.skimap.app.data.describe
import fr.skimap.core.MapStyle
import fr.skimap.core.PackEntry
import fr.skimap.core.PackException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/** What the main screen shows. */
sealed interface MapUiState {
    /** Listing and opening the packs. */
    data object Loading : MapUiState

    /** No readable pack in [packsDir]; [unreadable] lists the packs that do not open. */
    data class NoPack(
        val packsDir: String,
        val unreadable: List<PackEntry>,
    ) : MapUiState

    /** The pack in [dir] does not open; the other [packs] can still be chosen. */
    data class Failed(
        val dir: String,
        val message: String,
        val packs: List<PackEntry>,
    ) : MapUiState

    /** A pack is open and its map style ready. */
    data class Ready(
        val packs: List<PackEntry>,
        val opened: OpenedPack,
        /** Time to list the packs, open this one and build its style, in milliseconds. */
        val openMillis: Long,
        /** Last error of a basemap or pack switch, shown until the next action. */
        val error: String? = null,
        /** Layer switches ([fr.skimap.core.LayerToggle.id]) turned off for this style. */
        val hidden: Set<String> = emptySet(),
        /** Free mode: every installed pack in the frame; else one area. */
        val free: Boolean = false,
    ) : MapUiState
}

/**
 * State of the main screen: which pack and basemap are shown. Opens [preferredPack] (a
 * directory name under the packs directory, from the launch intent) if given, else the
 * last pack shown, else the first readable one.
 *
 * Nothing runs in the background: work happens only on a user action or at start, on
 * [io] for disk reads (the repository moves its own calls off the main thread too).
 */
class MapViewModel(
    private val repository: PackRepository,
    private val settings: SettingsStore,
    private val preferredPack: String? = null,
    private val nanoTime: () -> Long = System::nanoTime,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {
    private val mutableState = MutableStateFlow<MapUiState>(MapUiState.Loading)
    val state: StateFlow<MapUiState> = mutableState.asStateFlow()

    init {
        reload()
    }

    /** Lists the packs again and opens the preferred one. */
    fun reload() {
        mutableState.value = MapUiState.Loading
        viewModelScope.launch {
            val start = nanoTime()
            val packs = repository.list()
            val readable = packs.filter { it.error == null }
            if (readable.isEmpty()) {
                mutableState.value = MapUiState.NoPack(repository.packsDir.absolutePath, packs)
                return@launch
            }
            // The launch intent names an area; else the last choice; free mode by default.
            val remembered = withContext(io) { settings.get() }
            val preferredDir = preferredPack?.let { File(repository.packsDir, it).absolutePath }
            val area =
                if (preferredPack == FREE) {
                    null
                } else {
                    listOfNotNull(preferredDir, remembered.takeIf { it != FREE })
                        .firstNotNullOfOrNull { w -> readable.firstOrNull { it.dir == w } }
                }
            if (area !=
                null
            ) {
                open(listOf(area.dir), free = false, packs, start)
            } else {
                open(readable.map { it.dir }, free = true, packs, start)
            }
        }
    }

    /** Area mode: shows the pack in [dir] only. */
    fun selectPack(dir: String) {
        val current = mutableState.value
        val packs =
            when (current) {
                is MapUiState.Ready -> if (!current.free && current.opened.info.dir == dir) return else current.packs
                is MapUiState.Failed -> current.packs
                else -> return
            }
        viewModelScope.launch { open(listOf(dir), free = false, packs, nanoTime()) }
    }

    /** Free mode: every installed pack in the frame. */
    fun selectFree() {
        val current = mutableState.value as? MapUiState.Ready ?: return
        if (current.free) return
        val dirs = current.packs.filter { it.error == null }.map { it.dir }
        viewModelScope.launch { open(dirs, free = true, current.packs, nanoTime()) }
    }

    /** Shows the current pack on basemap [id]. */
    fun selectBasemap(id: String) {
        val current = mutableState.value as? MapUiState.Ready ?: return
        if (current.opened.style.basemap == id) return
        viewModelScope.launch {
            try {
                val style = repository.style(id)
                val hidden = hiddenFor(style)
                mutableState.update {
                    if (it is MapUiState.Ready) {
                        it.copy(opened = it.opened.copy(style = style), error = null, hidden = hidden)
                    } else {
                        it
                    }
                }
            } catch (e: PackException) {
                mutableState.update { if (it is MapUiState.Ready) it.copy(error = e.describe()) else it }
            }
        }
    }

    /**
     * Shows or hides a category of secondary elements (panel "Couches"). The map switches
     * layer visibility: no style reload. The choice is kept per style.
     */
    fun setLayerVisible(
        toggle: String,
        visible: Boolean,
    ) {
        val current = mutableState.value as? MapUiState.Ready ?: return
        val known =
            current.opened.style.toggles
                .any { it.id == toggle }
        val hidden = if (visible) current.hidden - toggle else current.hidden + toggle
        if (!known || hidden == current.hidden) return
        mutableState.value = current.copy(hidden = hidden)
        val style = styleKey(current.opened.style)
        viewModelScope.launch { withContext(io) { settings.setHiddenLayers(style, hidden) } }
    }

    private fun styleKey(style: MapStyle) = style.basemap ?: "blank"

    /** Switches turned off for [style]: the user's choice, else the defaults of the core. */
    private suspend fun hiddenFor(style: MapStyle): Set<String> {
        val stored = withContext(io) { settings.hiddenLayers(styleKey(style)) }
        val ids = style.toggles.map { it.id }.toSet()
        return stored?.intersect(ids) ?: style.toggles
            .filterNot { it.visibleByDefault }
            .map { it.id }
            .toSet()
    }

    private suspend fun open(
        dirs: List<String>,
        free: Boolean,
        packs: List<PackEntry>,
        start: Long,
    ) {
        try {
            val opened = repository.open(dirs, null)
            val millis = (nanoTime() - start) / NANOS_PER_MILLI
            withContext(io) { settings.set(if (free) FREE else dirs.first()) }
            Log.i(TAG, "pack ${opened.info.siteId} opened in $millis ms (list, open, style)")
            mutableState.value = MapUiState.Ready(packs, opened, millis, hidden = hiddenFor(opened.style), free = free)
        } catch (e: PackException) {
            val previous = mutableState.value
            mutableState.value =
                if (previous is MapUiState.Ready) {
                    previous.copy(error = e.describe())
                } else {
                    MapUiState.Failed(dirs.first(), e.describe(), packs)
                }
        }
    }

    private companion object {
        const val TAG = "skimap"
        const val NANOS_PER_MILLI = 1_000_000L

        /** The free mode: stored as the last choice, accepted as the launch extra `pack`. */
        const val FREE = "libre"
    }
}
