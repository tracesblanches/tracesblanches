package fr.skimap.app.sync

import android.content.Context
import androidx.work.NetworkType
import androidx.work.WorkInfo
import androidx.work.WorkManager
import fr.skimap.app.data.DownloadState
import fr.skimap.app.data.Downloads
import fr.skimap.core.ServerSite
import fr.skimap.core.removePack
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.withContext
import java.io.File

/** [Downloads] with WorkManager ([PackSync]) and the server catalog copy ([ServerCatalog]). */
class WorkManagerDownloads(
    private val context: Context,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : Downloads {
    private val catalog by lazy { PackSync.catalog(context) }

    override val states: Flow<Map<String, DownloadState>> =
        WorkManager
            .getInstance(context)
            .getWorkInfosByTagFlow(PackSync.TAG)
            .map { infos ->
                // Several works of a site (one finished, one queued): the most active wins.
                infos
                    .mapNotNull { info -> siteOf(info)?.let { site -> state(info)?.let { Triple(site, rank(info.state), it) } } }
                    .groupBy({ it.first }) { it.second to it.third }
                    .mapValues { (_, v) -> v.maxBy { it.first }.second }
            }

    private fun siteOf(info: WorkInfo): String? =
        info.tags
            .firstOrNull {
                it.startsWith("pack-") && it != "pack-updates"
            }?.removePrefix("pack-")

    /** Running, then queued, then failed, then finished. */
    private fun rank(s: WorkInfo.State): Int = ACTIVITY.indexOf(s)

    private fun state(info: WorkInfo): DownloadState? =
        when (info.state) {
            WorkInfo.State.ENQUEUED, WorkInfo.State.BLOCKED -> {
                DownloadState.Waiting(info.constraints.requiredNetworkType == NetworkType.UNMETERED)
            }

            WorkInfo.State.RUNNING -> {
                DownloadState.Running(info.progress.getLong(PackSyncWorker.DONE, 0), info.progress.getLong(PackSyncWorker.TOTAL, 0))
            }

            WorkInfo.State.FAILED -> {
                DownloadState.Failed(info.outputData.getString(PackSyncWorker.ERROR).orEmpty())
            }

            WorkInfo.State.SUCCEEDED -> {
                DownloadState.Done
            }

            WorkInfo.State.CANCELLED -> {
                null
            }
        }

    override suspend fun cachedSites(): List<ServerSite> = withContext(io) { catalog.cached() }

    override suspend fun refreshSites(): List<ServerSite>? = withContext(io) { catalog.refresh() }

    override fun download(
        site: ServerSite,
        mobileData: Boolean,
    ) {
        PackSync.download(context, site.siteId, site.indexUrl, site.firstDownloadBytes.toLong(), mobileData)
    }

    override fun cancel(site: String) = PackSync.cancel(context, site)

    override suspend fun remove(site: String) {
        PackSync.cancel(context, site)
        withContext(io) {
            removePack(File(PackSync.packsDir(context), site).absolutePath)
        }
    }

    companion object {
        private val ACTIVITY =
            listOf(
                WorkInfo.State.CANCELLED,
                WorkInfo.State.SUCCEEDED,
                WorkInfo.State.FAILED,
                WorkInfo.State.BLOCKED,
                WorkInfo.State.ENQUEUED,
                WorkInfo.State.RUNNING,
            )
    }
}
