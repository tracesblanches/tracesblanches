package fr.skimap.app.data

import fr.skimap.core.Catalog
import fr.skimap.core.Domain
import fr.skimap.core.DomainDistance
import fr.skimap.core.PackEntry
import fr.skimap.core.ServerSite
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

/**
 * The nordic areas of France (census, 219 areas), whether their pack is on the phone and
 * whether the server publishes it. Search and distances run in the Rust core; one call
 * per query.
 */
interface DomainCatalog {
    /** Outline of metropolitan France: rings of `[lon, lat, lon, lat, ...]`. */
    suspend fun outline(): List<List<Double>>

    /** Areas matching [query] (every word, accents ignored); all areas when blank. */
    suspend fun search(
        query: String,
        packs: List<PackEntry>,
        server: List<ServerSite> = emptyList(),
    ): List<Domain>

    /** Areas nearest to a position. */
    suspend fun nearest(
        lon: Double,
        lat: Double,
        packs: List<PackEntry>,
        server: List<ServerSite> = emptyList(),
    ): List<DomainDistance>
}

/** [DomainCatalog] read from the app resource `catalog.json`, parsed once by the core. */
class RustDomainCatalog(
    private val readJson: () -> String,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : DomainCatalog {
    private val mutex = Mutex()
    private var catalog: Catalog? = null

    private suspend fun catalog(): Catalog =
        mutex.withLock {
            catalog ?: withContext(io) { Catalog.fromJson(readJson()) }.also { catalog = it }
        }

    override suspend fun outline(): List<List<Double>> {
        val c = catalog()
        return withContext(io) { c.outline() }
    }

    override suspend fun search(
        query: String,
        packs: List<PackEntry>,
        server: List<ServerSite>,
    ): List<Domain> {
        val c = catalog()
        return withContext(io) { c.search(query, packs, server, MAX_RESULTS) }
    }

    override suspend fun nearest(
        lon: Double,
        lat: Double,
        packs: List<PackEntry>,
        server: List<ServerSite>,
    ): List<DomainDistance> {
        val c = catalog()
        return withContext(io) { c.nearest(lon, lat, packs, server, NEAREST) }
    }

    private companion object {
        const val MAX_RESULTS = 300u
        const val NEAREST = 5u
    }
}
