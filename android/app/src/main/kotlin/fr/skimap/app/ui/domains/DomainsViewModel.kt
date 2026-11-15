package fr.skimap.app.ui.domains

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import fr.skimap.app.data.DomainCatalog
import fr.skimap.app.data.DownloadState
import fr.skimap.app.data.Downloads
import fr.skimap.app.data.Locator
import fr.skimap.app.data.PackRepository
import fr.skimap.core.Domain
import fr.skimap.core.PackEntry
import fr.skimap.core.ServerSite
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the area picker shows. */
data class DomainsState(
    val loading: Boolean = true,
    val query: String = "",
    /** Areas matching the query (all when blank), installed first. */
    val results: List<Domain> = emptyList(),
    /** Every area, for the dots of the map. */
    val all: List<Domain> = emptyList(),
    /** Distance in km of each area when sorted by distance to the user. */
    val distances: Map<String, Double> = emptyMap(),
    val outline: List<List<Double>> = emptyList(),
    val locating: Boolean = false,
    /** The position could not be read (no permission, no fix). */
    val noPosition: Boolean = false,
    /** Download of each server site (by site identifier). */
    val downloads: Map<String, DownloadState> = emptyMap(),
    /** Sites whose installed pack is older than the one published. */
    val updates: Set<String> = emptySet(),
    /** The server catalog could not be read now (offline): the last copy is shown. */
    val offline: Boolean = false,
    /** Area chosen on the map of France (its row is shown first). */
    val selected: String? = null,
)

/**
 * Area picker: every nordic area of France on a map, search by name, nearest areas, and
 * the download of the packs the server publishes (K2). Work is done when the picker opens
 * and on each query or button, never in the background; downloads are WorkManager's.
 */
class DomainsViewModel(
    private val catalog: DomainCatalog,
    private val packs: PackRepository,
    private val locator: Locator,
    private val downloads: Downloads? = null,
) : ViewModel() {
    private val mutableState = MutableStateFlow(DomainsState())
    val state: StateFlow<DomainsState> = mutableState.asStateFlow()
    private var installed: List<PackEntry> = emptyList()
    private var server: List<ServerSite> = emptyList()
    private var searchJob: Job? = null
    private var watching: Job? = null

    /** Loads the catalogue, the packs on the phone and the server sites (each time the picker opens). */
    fun open() {
        viewModelScope.launch {
            installed = packs.list()
            server = downloads?.cachedSites().orEmpty()
            show(offline = false)
            // Then the server, once: the copy is kept for offline use.
            val d = downloads ?: return@launch
            val fresh = d.refreshSites()
            if (fresh != null) server = fresh
            show(offline = fresh == null)
        }
        val d = downloads ?: return
        if (watching == null) {
            watching =
                viewModelScope.launch {
                    d.states.collect { states ->
                        val finished = states.filterValues { it == DownloadState.Done }.keys
                        val before = mutableState.value.downloads
                        mutableState.update { it.copy(downloads = states) }
                        // A pack just installed: its status changes in the list.
                        if (finished.any { before[it] != null && before[it] != DownloadState.Done }) {
                            installed = packs.list()
                            show(mutableState.value.offline)
                        }
                    }
                }
        }
    }

    private suspend fun show(offline: Boolean) {
        val all = catalog.search("", installed, server)
        val q = mutableState.value.query
        val results = if (q.isBlank()) all else catalog.search(q, installed, server)
        val versions = installed.associate { it.siteId to it.packVersion }
        val updates = server.filter { s -> versions[s.siteId]?.let { it != s.latestVersion } == true }.map { it.siteId }.toSet()
        mutableState.update {
            it.copy(
                loading = false,
                results = results,
                all = all,
                outline = if (it.outline.isEmpty()) catalog.outline() else it.outline,
                updates = updates,
                offline = offline,
            )
        }
    }

    fun search(query: String) {
        mutableState.update { it.copy(query = query, distances = emptyMap(), noPosition = false, selected = null) }
        searchJob?.cancel()
        searchJob =
            viewModelScope.launch {
                val results = catalog.search(query, installed, server)
                mutableState.update { if (it.query == query) it.copy(results = results) else it }
            }
    }

    /** Areas sorted by distance to the current position (permission already granted). */
    fun nearest() {
        mutableState.update { it.copy(locating = true, noPosition = false) }
        viewModelScope.launch {
            val fix = locator.locate()
            if (fix == null) {
                mutableState.update { it.copy(locating = false, noPosition = true) }
                return@launch
            }
            near(fix.longitude, fix.latitude, keepQuery = false)
        }
    }

    /** Touch on the map of France at ([lon], [lat]): the areas nearest to it, first selected. */
    fun pickOnMap(
        lon: Double,
        lat: Double,
    ) {
        viewModelScope.launch { near(lon, lat, keepQuery = false, select = true) }
    }

    private suspend fun near(
        lon: Double,
        lat: Double,
        keepQuery: Boolean,
        select: Boolean = false,
    ) {
        val near = catalog.nearest(lon, lat, installed, server)
        mutableState.update { s ->
            s.copy(
                locating = false,
                query = if (keepQuery) s.query else "",
                results = near.map { it.domain },
                distances = near.associate { it.domain.id to it.km },
                selected = if (select) near.firstOrNull()?.domain?.id else null,
            )
        }
    }

    fun permissionDenied() {
        mutableState.update { it.copy(locating = false, noPosition = true) }
    }

    /** Downloads (or updates) the pack of [site]; [mobileData]: large download allowed off Wi-Fi. */
    fun download(
        site: String,
        mobileData: Boolean = false,
    ) {
        val s = server.firstOrNull { it.siteId == site } ?: return
        downloads?.download(s, mobileData)
    }

    fun cancel(site: String) {
        downloads?.cancel(site)
    }

    fun remove(site: String) {
        val d = downloads ?: return
        viewModelScope.launch {
            d.remove(site)
            installed = packs.list()
            show(mutableState.value.offline)
        }
    }
}
