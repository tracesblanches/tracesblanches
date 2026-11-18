package fr.skimap.app.sync

import fr.skimap.core.BundleInfo
import fr.skimap.core.SyncException
import fr.skimap.core.SyncPlan
import fr.skimap.core.installBundle
import fr.skimap.core.recordSync
import fr.skimap.core.syncPlan
import fr.skimap.core.syncPlanFull
import fr.skimap.core.syncPlanWholeFiles
import fr.skimap.core.syncState
import fr.skimap.core.verifyBundle
import fr.skimap.sync.Fetched
import fr.skimap.sync.HttpSource
import java.io.File

/** A pack to keep in sync: a site pack, or a voice language pack of a site (VO6). */
data class SyncTarget(
    /** Site identifier. */
    val site: String,
    /** Index URL, relative to the server (`/v1/sites/<site>/index.json`). */
    val indexUrl: String,
    /** Where the pack is installed (created by the first download). */
    val dir: File,
    /** Voice language (`fr`, `en`) for a voice pack; `null` for the site pack. */
    val voiceLang: String? = null,
)

/** What a sync did. */
data class SyncReport(
    val target: SyncTarget,
    /** Version installed before (`null`: none). */
    val from: String?,
    /** Version installed now. */
    val to: String?,
    /** Nothing new on the server (304 or same version). */
    val upToDate: Boolean,
    /** Requests made and bytes of body received. */
    val requests: Int,
    val bytes: Long,
    val bundles: Int,
    /** A delta needed contents the phone no longer had: the full plan was used. */
    val fellBack: Boolean,
    /** Not downloaded: the plan is larger than allowed on this network. */
    val deferredBytes: Long = 0,
    val millis: Long,
)

/**
 * One sync of one pack (K2), the steps of `crates/pack/docs/mise-a-jour.md`: revalidate
 * the index (`If-None-Match`; 304: one request, done), let the core plan the bundles,
 * download each with resume, check it whole (length, SHA-256), let the core install it
 * (every blob checked again, new version swapped in), fall back to whole files when a file
 * rebuilt from a patch of tiles differs (#160), to the base when the
 * phone lacks contents, record the state. The pack in place stays complete and readable
 * at every step: a cut leaves the old version and the partial download, resumed next time.
 */
class PackSyncer(
    private val http: HttpSource,
    private val nanoTime: () -> Long = System::nanoTime,
) {
    /** What happened, before the counters are added. */
    private data class Outcome(
        val from: String?,
        val to: String?,
        val upToDate: Boolean,
        val bundles: Int = 0,
        val fellBack: Boolean = false,
        val deferred: Long = 0,
    )

    /**
     * Syncs [target]. [maxBytes]: largest download allowed now (metered network); a
     * larger plan is not started and the report says how much it needs.
     */
    fun sync(
        target: SyncTarget,
        maxBytes: Long = Long.MAX_VALUE,
        onProgress: (done: Long, total: Long) -> Unit = { _, _ -> },
    ): SyncReport {
        val t0 = nanoTime()
        val requests0 = http.requests
        val bytes0 = http.bodyBytes
        target.dir.parentFile?.mkdirs()
        val state = syncState(target.dir.absolutePath)
        val o =
            when (val r = http.fetch(target.indexUrl, state?.indexEtag)) {
                Fetched.NotModified -> Outcome(state?.version, state?.version, upToDate = true)
                is Fetched.Body -> syncTo(target, r.bytes.decodeToString(), r.etag, maxBytes, onProgress)
            }
        return SyncReport(
            target,
            o.from,
            o.to,
            o.upToDate,
            http.requests - requests0,
            http.bodyBytes - bytes0,
            o.bundles,
            o.fellBack,
            o.deferred,
            (nanoTime() - t0) / NANOS_PER_MILLI,
        )
    }

    /** From a new index: plan, download, check, install, record. */
    private fun syncTo(
        target: SyncTarget,
        index: String,
        etag: String?,
        maxBytes: Long,
        onProgress: (Long, Long) -> Unit,
    ): Outcome {
        val dir = target.dir.absolutePath
        var plan = syncPlan(index, dir)
        val size = plan.downloadBytes.toLong()
        val outcome =
            when {
                plan.bundles.isEmpty() -> {
                    Outcome(plan.installedVersion, plan.latestVersion, upToDate = true)
                }

                size > maxBytes -> {
                    Outcome(plan.installedVersion, plan.installedVersion, upToDate = false, deferred = size)
                }

                else -> {
                    val work = File(target.dir.parentFile, ".${target.dir.name}.download")
                    val fellBack =
                        try {
                            try {
                                installAll(plan, dir, work, onProgress)
                                false
                            } catch (_: SyncException.PatchFailed) {
                                // A file rebuilt from a patch of tiles differs: the same update, files whole.
                                plan = syncPlanWholeFiles(index, dir)
                                installAll(plan, dir, work, onProgress)
                                true
                            }
                        } catch (_: SyncException.MissingContents) {
                            // The phone no longer has what the delta builds on: base, then delta.
                            plan = syncPlanFull(index, dir)
                            installAll(plan, dir, work, onProgress)
                            true
                        }
                    work.deleteRecursively()
                    Outcome(plan.installedVersion, plan.latestVersion, upToDate = false, bundles = plan.bundles.size, fellBack = fellBack)
                }
            }
        if (outcome.deferred == 0L) recordSync(dir, index, etag)
        return outcome
    }

    private fun installAll(
        plan: SyncPlan,
        dir: String,
        work: File,
        onProgress: (Long, Long) -> Unit,
    ) {
        val total = plan.downloadBytes.toLong()
        var before = 0L
        for (b in plan.bundles) {
            val file = File(work, "${b.sha256}.part")
            downloadChecked(b, file) { n -> onProgress(before + n, total) }
            installBundle(dir, file.absolutePath)
            file.delete()
            before += b.bytes.toLong()
        }
    }

    /** Downloads [b] into [file] (resumed if partial) and checks it whole; once more from zero if wrong. */
    private fun downloadChecked(
        b: BundleInfo,
        file: File,
        onProgress: (Long) -> Unit,
    ) {
        repeat(ATTEMPTS) { attempt ->
            http.download(b.url, file, b.sha256, b.bytes.toLong()) { n, _ -> onProgress(n) }
            try {
                verifyBundle(file.absolutePath, b.sha256, b.bytes)
                return
            } catch (e: SyncException.Corrupt) {
                // Wrong bytes on disk or in transit.
                file.delete()
                if (attempt == ATTEMPTS - 1) throw e
            }
        }
    }

    private companion object {
        const val NANOS_PER_MILLI = 1_000_000L
        const val ATTEMPTS = 2
    }
}
