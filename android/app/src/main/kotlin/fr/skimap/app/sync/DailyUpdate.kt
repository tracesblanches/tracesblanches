package fr.skimap.app.sync

import android.content.Context
import android.util.Log
import fr.skimap.core.SyncException
import java.io.File
import java.io.IOException
import java.time.Instant
import java.time.temporal.ChronoUnit

/** What the daily update did with one pack. */
sealed interface PackUpdate {
    val target: SyncTarget

    /** Synced: up to date, installed, or left for an unmetered network ([SyncReport.deferredBytes]). */
    data class Synced(
        override val target: SyncTarget,
        val report: SyncReport,
    ) : PackUpdate

    /** Not synced, the pack in place kept: [error]; [retry] when a later attempt may pass (network, server). */
    data class Failed(
        override val target: SyncTarget,
        val error: Exception,
        val retry: Boolean,
    ) : PackUpdate
}

/**
 * The daily update of the installed packs (site packs and their voice), #176: every pack
 * is tried, one that fails (its index or a bundle unreachable, a bundle that does not
 * install) keeps its version and leaves the next ones be; each pack gets one line in
 * [journal], with the reason of a failure or of a wait.
 */
class DailyUpdate(
    private val syncer: PackSyncer,
    private val journal: (String) -> Unit,
) {
    fun run(
        targets: List<SyncTarget>,
        maxBytes: Long,
    ): List<PackUpdate> =
        targets
            .map { t ->
                try {
                    PackUpdate.Synced(t, syncer.sync(t, maxBytes))
                } catch (e: IOException) {
                    PackUpdate.Failed(t, e, retry = true)
                } catch (e: SyncException) {
                    PackUpdate.Failed(t, e, retry = false)
                }
            }.onEach { journal(describe(it)) }

    companion object {
        /** One line of the journal for [u]. */
        fun describe(u: PackUpdate): String =
            when (u) {
                is PackUpdate.Synced -> describe(u.report)
                is PackUpdate.Failed -> "sync ${name(u.target)} failed${if (u.retry) " (retried later)" else ""}, pack kept: ${u.error}"
            }

        fun describe(r: SyncReport): String =
            "sync ${name(r.target)}: ${r.from} -> ${r.to}, " +
                "${r.requests} requests, ${r.bytes} bytes, ${r.bundles} bundles, ${r.millis} ms" +
                (if (r.upToDate) ", up to date" else "") +
                (if (r.deferredBytes > 0) ", deferred ${r.deferredBytes} bytes: metered network, waits for Wi-Fi" else "")

        private fun name(t: SyncTarget) = t.site + (t.voiceLang?.let { " voice $it" } ?: "")
    }
}

/**
 * The sync journal on the phone (#176): one dated line per pack and per sync, the last
 * [maxLines] kept, so that what happened at night is still there in the morning (logcat
 * keeps a few hours). Next to the packs (`files/sync-journal.txt` of the app's external
 * storage), readable with adb.
 */
class SyncJournal(
    private val file: File,
    private val now: () -> Instant = Instant::now,
    private val maxLines: Int = MAX_LINES,
) {
    /** Adds [line]; the file is replaced whole (temporary file, then rename). */
    fun add(line: String) =
        synchronized(LOCK) {
            val old = if (file.isFile) file.readLines() else emptyList()
            val lines = (old + "${now().truncatedTo(ChronoUnit.SECONDS)} $line").takeLast(maxLines)
            file.parentFile?.mkdirs()
            val tmp = File(file.parentFile, ".${file.name}.tmp")
            tmp.writeText(lines.joinToString("\n", postfix = "\n"))
            if (!tmp.renameTo(file)) throw IOException("$tmp: not renamed to $file")
        }

    fun lines(): List<String> = synchronized(LOCK) { if (file.isFile) file.readLines() else emptyList() }

    companion object {
        private const val MAX_LINES = 400
        private const val LOG_TAG = "skimap"
        private val LOCK = Any()

        /**
         * [line] in logcat and in the journal of the app (`sync-journal.txt` next to the
         * packs); a journal that cannot be written is said in logcat.
         */
        fun note(
            context: Context,
            line: String,
        ) {
            Log.i(LOG_TAG, line)
            try {
                SyncJournal(File(context.getExternalFilesDir(null), "sync-journal.txt")).add(line)
            } catch (e: IOException) {
                Log.w(LOG_TAG, "sync journal not written: $e")
            }
        }
    }
}
