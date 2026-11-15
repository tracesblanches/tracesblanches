package fr.skimap.app.sync

import android.content.Context
import android.net.ConnectivityManager
import android.util.Log
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.Data
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import fr.skimap.core.SyncException
import fr.skimap.core.listPacks
import fr.skimap.sync.HttpSource
import java.io.File
import java.io.IOException
import java.util.concurrent.TimeUnit

/**
 * Download and update of packs (K2 #52) with WorkManager. Nothing runs without a
 * constraint (E1 #125): a download asked by the user waits for a network, an unmetered one
 * when it is large (unless the user chose mobile data); the daily update of the installed
 * packs waits for a network and a battery that is not low, and leaves large plans to the
 * next unmetered network. Without network (airplane mode), work just waits: the packs on
 * the phone stay complete and the map works.
 */
object PackSync {
    /** Tag of every sync work (the UI follows them). */
    const val TAG = "skimap-sync"

    /** Above this, a download waits for an unmetered network unless the user allows mobile data. */
    const val BIG_BYTES = 10L * 1024 * 1024

    /** The production server (crates/server/docs/api.md). */
    const val DEFAULT_SERVER = "https://skimap.warnier.net"

    private const val PREFS = "skimap"
    private const val LOG_TAG = "skimap"
    private const val KEY_SERVER = "server_url"
    private const val PERIODIC = "pack-updates"
    private const val BACKOFF_SECONDS = 30L
    private const val PERIOD_HOURS = 24L

    /** Server of the app; a test or debug build may point it elsewhere (preference). */
    fun serverUrl(context: Context): String =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_SERVER, null) ?: DEFAULT_SERVER

    fun http(context: Context): HttpSource {
        val version =
            context.packageManager
                .getPackageInfo(context.packageName, 0)
                .versionName ?: "0"
        return HttpSource(serverUrl(context), "skimap-android/$version")
    }

    fun packsDir(context: Context): File = File(context.getExternalFilesDir(null), "packs")

    fun voiceDir(context: Context): File = File(context.getExternalFilesDir(null), "voice")

    fun catalog(context: Context): ServerCatalog = ServerCatalog({ http(context) }, File(context.filesDir, "server"))

    /** Unique work name of a pack (site or voice language). */
    fun workName(
        site: String,
        voiceLang: String? = null,
    ): String = if (voiceLang == null) "pack-$site" else "voice-$site-$voiceLang"

    /**
     * Downloads or updates the pack of [site] ([bytes]: size of the download, to choose the
     * network). [mobileData]: the user accepts a large download on a metered network.
     */
    fun download(
        context: Context,
        site: String,
        indexUrl: String,
        bytes: Long,
        mobileData: Boolean = false,
        voiceLang: String? = null,
    ) {
        val big = bytes > BIG_BYTES && !mobileData
        val request =
            OneTimeWorkRequestBuilder<PackSyncWorker>()
                .setConstraints(
                    Constraints
                        .Builder()
                        .setRequiredNetworkType(if (big) NetworkType.UNMETERED else NetworkType.CONNECTED)
                        .setRequiresStorageNotLow(true)
                        .build(),
                ).setBackoffCriteria(BackoffPolicy.EXPONENTIAL, BACKOFF_SECONDS, TimeUnit.SECONDS)
                .setInputData(
                    workDataOf(
                        PackSyncWorker.SITE to site,
                        PackSyncWorker.INDEX to indexUrl,
                        PackSyncWorker.VOICE to voiceLang,
                        PackSyncWorker.METERED_OK to mobileData,
                        PackSyncWorker.BYTES to bytes,
                    ),
                ).addTag(TAG)
                .addTag(workName(site, voiceLang))
                .build()
        WorkManager.getInstance(context).enqueueUniqueWork(workName(site, voiceLang), ExistingWorkPolicy.REPLACE, request)
        Log.i(LOG_TAG, "sync queued: ${workName(site, voiceLang)}, $bytes bytes, ${if (big) "unmetered" else "any"} network")
    }

    fun cancel(
        context: Context,
        site: String,
        voiceLang: String? = null,
    ) {
        WorkManager.getInstance(context).cancelUniqueWork(workName(site, voiceLang))
    }

    /** Daily update of the installed packs, with its constraints; [on] = false removes it. */
    fun schedulePeriodic(
        context: Context,
        on: Boolean,
    ) {
        val wm = WorkManager.getInstance(context)
        if (!on) {
            wm.cancelUniqueWork(PERIODIC)
            return
        }
        val request =
            PeriodicWorkRequestBuilder<PackSyncWorker>(PERIOD_HOURS, TimeUnit.HOURS)
                .setConstraints(
                    Constraints
                        .Builder()
                        .setRequiredNetworkType(NetworkType.CONNECTED)
                        .setRequiresBatteryNotLow(true)
                        .setRequiresStorageNotLow(true)
                        .build(),
                ).setBackoffCriteria(BackoffPolicy.EXPONENTIAL, BACKOFF_SECONDS, TimeUnit.SECONDS)
                .setInputData(workDataOf(PackSyncWorker.ALL to true))
                .addTag(TAG)
                .build()
        wm.enqueueUniquePeriodicWork(PERIODIC, ExistingPeriodicWorkPolicy.KEEP, request)
        // The speech model of the voice commands comes with the packs, on its own (Wi-Fi, charging).
        fr.skimap.app.voicecmd.VoiceModelWorker
            .schedule(context)
    }
}

/** The work of [PackSync]: one pack, or every installed pack (daily update). */
class PackSyncWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val ctx = applicationContext
        val http = PackSync.http(ctx)
        val syncer = PackSyncer(http)
        val metered = ctx.getSystemService(ConnectivityManager::class.java)?.isActiveNetworkMetered ?: true
        val meteredOk = inputData.getBoolean(METERED_OK, false)
        val maxBytes = if (metered && !meteredOk) PackSync.BIG_BYTES else Long.MAX_VALUE
        return try {
            if (inputData.getBoolean(ALL, false)) updateAll(syncer, maxBytes) else one(syncer, maxBytes)
        } catch (e: IOException) {
            // Network lost, server unreachable, cut download: kept for a later attempt.
            Log.w(TAG, "sync interrupted, retried later: $e")
            Result.retry()
        } catch (e: SyncException) {
            Log.e(TAG, "sync failed: $e")
            Result.failure(workDataOf(ERROR to e.toString()))
        }
    }

    private suspend fun one(
        syncer: PackSyncer,
        maxBytes: Long,
    ): Result {
        val site = inputData.getString(SITE)
        val index = inputData.getString(INDEX)
        if (site == null || index == null) return Result.failure()
        val voice = inputData.getString(VOICE)
        val dir =
            if (voice ==
                null
            ) {
                File(PackSync.packsDir(applicationContext), site)
            } else {
                File(PackSync.voiceDir(applicationContext), "$site/$voice")
            }
        setProgress(workDataOf(DONE to 0L, TOTAL to inputData.getLong(BYTES, 0)))
        val r =
            syncer.sync(SyncTarget(site, index, dir, voice), maxBytes) { done, total ->
                setProgressAsync(workDataOf(DONE to done, TOTAL to total))
            }
        log(r)
        if (r.deferredBytes > 0) {
            // Large and on a metered network: wait for an unmetered one.
            PackSync.download(applicationContext, site, index, r.deferredBytes, mobileData = false, voiceLang = voice)
        } else if (voice != null) {
            // One voice language per site: the new one replaces the others (VO6).
            dir.parentFile?.listFiles()?.filter { it.isDirectory && it.name != voice && !it.name.startsWith(".") }?.forEach {
                it.deleteRecursively()
            }
        }
        return Result.success(output(r))
    }

    private fun updateAll(
        syncer: PackSyncer,
        maxBytes: Long,
    ): Result {
        val ctx = applicationContext
        val catalog = PackSync.catalog(ctx)
        val sites = (catalog.refresh() ?: catalog.cached()).associateBy { it.siteId }
        var changed = false
        for (p in listPacks(PackSync.packsDir(ctx).absolutePath)) {
            val site = sites[p.siteId] ?: continue
            val r = syncer.sync(SyncTarget(site.siteId, site.indexUrl, File(p.dir)), maxBytes)
            log(r)
            changed = changed || (!r.upToDate && r.deferredBytes == 0L)
            if (r.deferredBytes > 0) PackSync.download(ctx, site.siteId, site.indexUrl, r.deferredBytes)
            for (v in site.voice) {
                val dir = File(PackSync.voiceDir(ctx), "${site.siteId}/${v.lang}")
                if (!dir.isDirectory) continue
                val vr = syncer.sync(SyncTarget(site.siteId, v.indexUrl, dir, v.lang), maxBytes)
                log(vr)
            }
        }
        return Result.success(workDataOf(CHANGED to changed))
    }

    private fun log(r: SyncReport) {
        Log.i(
            TAG,
            "sync ${r.target.site}${r.target.voiceLang?.let { " voice $it" } ?: ""}: ${r.from} -> ${r.to}, " +
                "${r.requests} requests, ${r.bytes} bytes, ${r.bundles} bundles, ${r.millis} ms" +
                (if (r.upToDate) ", up to date" else "") + (if (r.deferredBytes > 0) ", deferred ${r.deferredBytes} bytes" else ""),
        )
    }

    private fun output(r: SyncReport): Data =
        workDataOf(
            SITE to r.target.site,
            VOICE to r.target.voiceLang,
            FROM to r.from,
            TO to r.to,
            CHANGED to (!r.upToDate && r.deferredBytes == 0L),
            REQUESTS to r.requests,
            RECEIVED to r.bytes,
            MILLIS to r.millis,
            DEFERRED to r.deferredBytes,
        )

    companion object {
        private const val TAG = "skimap"
        const val SITE = "site"
        const val INDEX = "index"
        const val VOICE = "voice"
        const val METERED_OK = "metered_ok"
        const val BYTES = "bytes"
        const val ALL = "all"
        const val DONE = "done"
        const val TOTAL = "total"
        const val FROM = "from"
        const val TO = "to"
        const val CHANGED = "changed"
        const val REQUESTS = "requests"
        const val RECEIVED = "received"
        const val MILLIS = "millis"
        const val DEFERRED = "deferred"
        const val ERROR = "error"
    }
}
