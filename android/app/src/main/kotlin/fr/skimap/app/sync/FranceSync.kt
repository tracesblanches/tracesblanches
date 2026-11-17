package fr.skimap.app.sync

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.workDataOf
import fr.skimap.core.SyncException
import fr.skimap.core.francePlan
import fr.skimap.core.installFrance
import fr.skimap.sync.Fetched
import fr.skimap.sync.HttpSource
import java.io.File
import java.io.IOException
import java.util.concurrent.TimeUnit

/** Where the France basemap lives on the phone, and its work (X20, #154). */
object FranceBasemap {
    /** The France basemap of the phone: under the packs, alone without any pack. */
    fun dir(context: Context): File = File(context.getExternalFilesDir(null), "france")

    /** Unique work of the France basemap; its `pack-` tag lets the map reload when it lands. */
    const val WORK = "pack-france-basemap"

    private const val BACKOFF_SECONDS = 30L

    /**
     * Fetches or revalidates the France basemap: automatic, at every start, on any network
     * (2 MB; the daily update revalidates it too). KEEP: one work at a time.
     */
    fun download(context: Context) {
        val request =
            OneTimeWorkRequestBuilder<PackSyncWorker>()
                .setConstraints(
                    Constraints
                        .Builder()
                        .setRequiredNetworkType(NetworkType.CONNECTED)
                        .setRequiresStorageNotLow(true)
                        .build(),
                ).setBackoffCriteria(BackoffPolicy.EXPONENTIAL, BACKOFF_SECONDS, TimeUnit.SECONDS)
                .setInputData(workDataOf(PackSyncWorker.FRANCE to true))
                .addTag(PackSync.TAG)
                .addTag(WORK)
                .build()
        WorkManager.getInstance(context).enqueueUniqueWork(WORK, ExistingWorkPolicy.KEEP, request)
    }
}

/** What a sync of the France basemap did. */
data class FranceReport(
    /** Build of the basemap on the phone now (`null`: none). */
    val build: String?,
    /** Nothing new on the server (304 or same description). */
    val upToDate: Boolean,
    /** Files and bytes downloaded. */
    val files: Int,
    val bytes: Long,
    val millis: Long,
)

/**
 * The France basemap (X20, #154): one light background of the whole of France, shared by
 * every pack, kept on the phone so that the free mode moves over the whole country without
 * any pack (`crates/server/docs/api.md`, « Fond de France »). Revalidates the description
 * (`If-None-Match`; 304: one request, done), downloads the files the core says are missing
 * (resumed, named by their SHA-256), lets the core check and install them, the description
 * last. The basemap in place stays readable at every step.
 */
class FranceSyncer(
    private val http: HttpSource,
    private val nanoTime: () -> Long = System::nanoTime,
) {
    /**
     * Syncs the France basemap into [dir]. One sync at a time in the process: its own work
     * and the daily update may run together, and share the download directory.
     */
    fun sync(dir: File): FranceReport = synchronized(LOCK) { syncAlone(dir) }

    private fun syncAlone(dir: File): FranceReport {
        val t0 = nanoTime()
        dir.mkdirs()
        val etagFile = File(dir.parentFile, "${dir.name}.etag")
        val installed = File(dir, DESCRIPTION).isFile
        val etag = etagFile.takeIf { installed && it.isFile }?.readText()
        val millis = { (nanoTime() - t0) / NANOS_PER_MILLI }
        return when (val r = http.fetch(DESCRIPTION_URL, etag)) {
            Fetched.NotModified -> {
                FranceReport(null, upToDate = true, files = 0, bytes = 0, millis = millis())
            }

            is Fetched.Body -> {
                val json = r.bytes.decodeToString()
                val plan = francePlan(json, dir.absolutePath)
                if (!plan.upToDate) install(json, dir, plan.files)
                r.etag?.let { etagFile.writeText(it) }
                FranceReport(plan.build, plan.upToDate, plan.files.size, plan.downloadBytes.toLong(), millis())
            }
        }
    }

    private fun install(
        json: String,
        dir: File,
        files: List<fr.skimap.core.FranceFile>,
    ) {
        val work = File(dir.parentFile, ".${dir.name}.download")
        for (f in files) {
            http.download(FILES_URL + f.sha256, File(work, f.sha256), f.sha256, f.bytes.toLong())
        }
        try {
            installFrance(json, dir.absolutePath, work.absolutePath)
        } catch (e: SyncException.Corrupt) {
            // The core deleted the bad file: downloaded again on the next attempt.
            throw IOException("France basemap: $e", e)
        }
        work.deleteRecursively()
    }

    companion object {
        /** Description of the France basemap on the server. */
        const val DESCRIPTION_URL = "/v1/basemap/france.json"

        /** Files of the France basemap on the server, by SHA-256. */
        const val FILES_URL = "/v1/basemap/files/"

        /** Held by a sync of the France basemap (one directory per phone). */
        private val LOCK = Any()

        /** Name of the description in the basemap directory. */
        const val DESCRIPTION = "france.json"
        private const val NANOS_PER_MILLI = 1_000_000L
    }
}
