package fr.skimap.app.data

import fr.skimap.core.ServerSite
import kotlinx.coroutines.flow.Flow

/** Where the download of a pack stands (WorkManager work of the pack). */
sealed interface DownloadState {
    /** Queued, waiting for its constraints: a network ([unmetered]: Wi-Fi or similar). */
    data class Waiting(
        val unmetered: Boolean,
    ) : DownloadState

    /** Downloading: [done] of [total] bytes. */
    data class Running(
        val done: Long,
        val total: Long,
    ) : DownloadState

    /** Stopped on an error that a retry will not fix (the pack in place is untouched). */
    data class Failed(
        val message: String,
    ) : DownloadState

    /** Installed by the last run. */
    data object Done : DownloadState
}

/** Pack download and update (K2): the server catalog and the works of WorkManager. */
interface Downloads {
    /** State of the download of each site pack (by site identifier), live. */
    val states: Flow<Map<String, DownloadState>>

    /** Sites the server publishes: the copy on the phone (empty before the first read). */
    suspend fun cachedSites(): List<ServerSite>

    /** Sites the server publishes, asked again (one request, 304 when unchanged); `null` offline. */
    suspend fun refreshSites(): List<ServerSite>?

    /** Downloads (or updates) the pack of [site]. */
    fun download(
        site: ServerSite,
        mobileData: Boolean,
    )

    fun cancel(site: String)

    /** Deletes the pack of [site] from the phone. */
    suspend fun remove(site: String)
}
