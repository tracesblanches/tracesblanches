package fr.skimap.app.ui

import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import fr.skimap.app.data.CameraSpot
import fr.skimap.app.data.DownloadState
import fr.skimap.app.data.Downloads
import fr.skimap.app.data.OpenedPack
import fr.skimap.app.data.PackProblem
import fr.skimap.app.data.PackRepository
import fr.skimap.app.data.Practice
import fr.skimap.app.data.SettingsStore
import fr.skimap.app.data.StyleChoice
import fr.skimap.app.data.StyleSource
import fr.skimap.app.data.problem
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
        val problem: PackProblem,
        val packs: List<PackEntry>,
    ) : MapUiState

    /** Packs are open and their map style ready. */
    data class Ready(
        val packs: List<PackEntry>,
        val opened: OpenedPack,
        /** Time to list the packs, open them and instantiate the style, in milliseconds. */
        val openMillis: Long,
        /** Last error of a style or pack switch, shown until the next action. */
        val error: PackProblem? = null,
        /** Layer groups ([fr.skimap.core.LayerGroup.id]) turned off for this style. */
        val hidden: Set<String> = emptySet(),
        /** Free mode: every installed pack in the frame; else one area. */
        val free: Boolean = false,
        /** Style shown: practice, guidance, theme. */
        val choice: StyleChoice = StyleChoice(),
    ) : MapUiState
}

/**
 * State of the main screen: which packs and which validated style are shown. Opens
 * [preferredPack] (a directory name under the packs directory, from the launch intent)
 * if given, else the last area shown, else every installed pack (free mode, the default).
 *
 * Nothing runs in the background: work happens only on a user action, at start, or when
 * a sync installed a new pack version, on [io] for disk reads.
 */
class MapViewModel(
    private val repository: PackRepository,
    private val settings: SettingsStore,
    private val styles: StyleSource,
    private val preferredPack: String? = null,
    /** Dark theme of the system at start (the style follows it without a second load). */
    dark: Boolean = false,
    /** Downloads (K2): a pack installed or updated reopens the map with it. */
    downloads: Downloads? = null,
    private val nanoTime: () -> Long = System::nanoTime,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {
    private val mutableState = MutableStateFlow<MapUiState>(MapUiState.Loading)
    val state: StateFlow<MapUiState> = mutableState.asStateFlow()

    /** Style wanted; the map follows it once loaded. */
    private var choice = StyleChoice(dark = dark)

    /** Where the map was when the app was last left (read once, at start). */
    var lastCamera: CameraSpot? = null
        private set

    init {
        reload()
        if (downloads != null) {
            viewModelScope.launch {
                var done: Set<String>? = null
                downloads.states.collect { states ->
                    val now = states.filterValues { it == DownloadState.Done }.keys
                    // Only a transition to "installed" reloads, not the works done before.
                    val before = done
                    if (before != null && (now - before).isNotEmpty() && mutableState.value !is MapUiState.Loading) reload()
                    done = now
                }
            }
        }
    }

    /** Lists the packs again and opens the preferred ones (also after a download). */
    fun reload() {
        val previous = mutableState.value as? MapUiState.Ready
        if (previous == null) mutableState.value = MapUiState.Loading
        viewModelScope.launch {
            val start = nanoTime()
            val packs = repository.list()
            val readable = packs.filter { it.error == null }
            if (readable.isEmpty()) {
                mutableState.value = MapUiState.NoPack(repository.packsDir.absolutePath, packs)
                return@launch
            }
            val (remembered, practice) = withContext(io) { settings.get() to settings.practice() }
            if (lastCamera == null) lastCamera = withContext(io) { settings.camera() }
            choice = choice.copy(practice = Practice.of(practice))
            // The launch intent names an area; else the last choice; free mode by default.
            val preferredDir = preferredPack?.let { File(repository.packsDir, it).absolutePath }
            val area =
                if (preferredPack == FREE) {
                    null
                } else {
                    listOfNotNull(preferredDir, remembered.takeIf { it != FREE })
                        .firstNotNullOfOrNull { w -> readable.firstOrNull { it.dir == w } }
                }
            if (area != null) {
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

    /** Ski de fond (pistes) or SRN (off piste): the style of the practice, remembered. */
    fun selectPractice(practice: Practice) {
        if (choice.practice == practice) return
        viewModelScope.launch { withContext(io) { settings.setPractice(practice.style) } }
        restyle(choice.copy(practice = practice))
    }

    /** Guidance on or off (a day plan followed): the guidance style, without a gesture. */
    fun setGuiding(guiding: Boolean) {
        if (choice.guiding != guiding) restyle(choice.copy(guiding = guiding))
    }

    /** Light or dark theme of the system. */
    fun setDark(dark: Boolean) {
        if (choice.dark != dark) restyle(choice.copy(dark = dark))
    }

    private fun restyle(wanted: StyleChoice) {
        choice = wanted
        val current = mutableState.value as? MapUiState.Ready ?: return
        // The practice shows at once in the menu, the map follows when the style is ready.
        mutableState.value = current.copy(choice = wanted)
        viewModelScope.launch {
            try {
                val style = repository.style(styles.read(wanted))
                val hidden = hiddenGroups(settings, style, io)
                mutableState.update {
                    if (it is MapUiState.Ready && choice == wanted) {
                        it.copy(opened = it.opened.copy(style = style), error = null, hidden = hidden, choice = wanted)
                    } else {
                        it
                    }
                }
            } catch (e: PackException) {
                mutableState.update { if (it is MapUiState.Ready) it.copy(error = e.problem()) else it }
            }
        }
    }

    /**
     * Shows or hides a layer group of the style (panel "Couches"). The map switches layer
     * visibility: no style reload. The choice is kept per style family.
     */
    fun setLayerVisible(
        group: String,
        visible: Boolean,
    ) {
        val current = mutableState.value as? MapUiState.Ready ?: return
        val toggleable =
            current.opened.style.groups
                .any { it.id == group && it.toggleable }
        val hidden = if (visible) current.hidden - group else current.hidden + group
        if (!toggleable || hidden == current.hidden) return
        mutableState.value = current.copy(hidden = hidden)
        val family = current.opened.style.styleId
        viewModelScope.launch { withContext(io) { settings.setHiddenLayers(family, hidden) } }
    }

    /** Keeps the camera for the next launch (once, when the app goes to the background). */
    fun saveCamera(spot: CameraSpot) {
        lastCamera = spot
        viewModelScope.launch { withContext(io) { settings.setCamera(spot) } }
    }

    private suspend fun open(
        dirs: List<String>,
        free: Boolean,
        packs: List<PackEntry>,
        start: Long,
    ) {
        val wanted = choice
        try {
            val opened =
                repository
                    .open(dirs, styles.read(wanted), free)
                    .copy(
                        packBounds = dirs.mapNotNull { d -> packs.firstOrNull { it.dir == d }?.bounds },
                        packNames = dirs.map { d -> packs.firstOrNull { it.dir == d }?.siteName.orEmpty() },
                    )
            val millis = (nanoTime() - start) / NANOS_PER_MILLI
            withContext(io) { settings.set(if (free) FREE else dirs.first()) }
            Log.i(TAG, "packs ${opened.info.siteId} opened in $millis ms (list, open, style ${opened.style.name})")
            mutableState.value =
                MapUiState.Ready(packs, opened, millis, hidden = hiddenGroups(settings, opened.style, io), free = free, choice = wanted)
            if (choice != wanted) restyle(choice)
        } catch (e: PackException) {
            val previous = mutableState.value
            mutableState.value =
                if (previous is MapUiState.Ready) {
                    previous.copy(error = e.problem())
                } else {
                    MapUiState.Failed(dirs.first(), e.problem(), packs)
                }
        }
    }

    companion object {
        private const val TAG = "skimap"
        private const val NANOS_PER_MILLI = 1_000_000L

        /** The free mode: stored as the last choice, accepted as the launch extra `pack`. */
        const val FREE = "libre"
    }
}

/** Groups turned off for [style]: the user's choice for its family, else the defaults of the style. */
private suspend fun hiddenGroups(
    settings: SettingsStore,
    style: MapStyle,
    io: CoroutineDispatcher,
): Set<String> {
    val stored = withContext(io) { settings.hiddenLayers(style.styleId) }
    val toggleable = style.groups.filter { it.toggleable }
    return stored?.intersect(toggleable.map { it.id }.toSet())
        ?: toggleable.filterNot { it.visibleByDefault }.map { it.id }.toSet()
}
