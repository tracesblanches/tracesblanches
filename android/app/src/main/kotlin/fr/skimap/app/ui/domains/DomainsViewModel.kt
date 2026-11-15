package fr.skimap.app.ui.domains

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import fr.skimap.app.data.DomainCatalog
import fr.skimap.app.data.Locator
import fr.skimap.app.data.PackRepository
import fr.skimap.core.Domain
import fr.skimap.core.PackEntry
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
)

/**
 * Area picker: every nordic area of France on a map, search by name, nearest areas. Work
 * is done when the picker opens and on each query, never in the background.
 */
class DomainsViewModel(
    private val catalog: DomainCatalog,
    private val packs: PackRepository,
    private val locator: Locator,
) : ViewModel() {
    private val mutableState = MutableStateFlow(DomainsState())
    val state: StateFlow<DomainsState> = mutableState.asStateFlow()
    private var installed: List<PackEntry> = emptyList()
    private var searchJob: Job? = null

    /** Loads the catalogue and the packs on the phone (each time the picker opens). */
    fun open() {
        viewModelScope.launch {
            installed = packs.list()
            val all = catalog.search("", installed)
            mutableState.value =
                DomainsState(loading = false, results = all, all = all, outline = catalog.outline())
        }
    }

    fun search(query: String) {
        mutableState.update { it.copy(query = query, distances = emptyMap(), noPosition = false) }
        searchJob?.cancel()
        searchJob =
            viewModelScope.launch {
                val results = catalog.search(query, installed)
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
            val near = catalog.nearest(fix.longitude, fix.latitude, installed)
            mutableState.update { s ->
                s.copy(
                    locating = false,
                    query = "",
                    results = near.map { it.domain },
                    distances = near.associate { it.domain.id to it.km },
                )
            }
        }
    }

    fun permissionDenied() {
        mutableState.update { it.copy(locating = false, noPosition = true) }
    }
}
