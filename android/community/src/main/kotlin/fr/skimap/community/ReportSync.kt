package fr.skimap.community

import android.content.Context
import android.graphics.Bitmap
import android.util.Log
import androidx.core.graphics.scale
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.io.ByteArrayOutputStream
import java.io.File
import java.net.HttpURLConnection
import java.net.URI
import java.util.concurrent.TimeUnit

/**
 * The queue of the reports sent by WorkManager (V11): one job with a network constraint,
 * enqueued at each report (APPEND_OR_REPLACE: a report made while the queue drains is sent
 * by the next run). Without network the job waits; nothing polls.
 */
object ReportSync {
    const val WORK = "community-reports"
    private const val LOG_TAG = "skimap"
    private const val BACKOFF_SECONDS = 30L
    private const val PREFS = "skimap_community"
    private const val KEY_SERVER = "server_url"
    private const val DEFAULT_SERVER = "https://skimap.warnier.net"

    fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /** Server of the reports: the app's (preference `server_url` of `skimap`, as the packs). */
    fun serverUrl(context: Context): String =
        context.getSharedPreferences("skimap", Context.MODE_PRIVATE).getString(KEY_SERVER, null) ?: DEFAULT_SERVER

    fun outbox(context: Context) = ReportOutbox(File(context.filesDir, "community/outbox"))

    fun device(context: Context) = DeviceId({ prefs(context) })

    fun mine(context: Context) = MyReports { prefs(context) }

    fun api(context: Context) = CommunityApi(serverUrl(context), "skimap-android")

    fun enqueue(context: Context) {
        val request =
            OneTimeWorkRequestBuilder<ReportUploadWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, BACKOFF_SECONDS, TimeUnit.SECONDS)
                .addTag(WORK)
                .build()
        WorkManager.getInstance(context).enqueueUniqueWork(WORK, ExistingWorkPolicy.APPEND_OR_REPLACE, request)
    }

    fun log(message: String) {
        Log.i(LOG_TAG, "community: $message")
    }
}

/** Drains the queue; retries (with backoff) while something could not be sent. */
class ReportUploadWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val c = applicationContext
        val r =
            ReportUploader(ReportSync.outbox(c), ReportSync.api(c), { ReportSync.device(c).current() }, ReportSync.mine(c))
                .drain()
        ReportSync.log("sent ${r.sent}, dropped ${r.dropped}, retry ${r.retry}")
        return if (r.retry) Result.retry() else Result.success()
    }
}

/**
 * The optional photo of a report, reduced on the phone (V9): at most [MAX_SIDE] pixels,
 * re-encoded in JPEG until it weighs at most [MAX_BYTES]. `Bitmap.compress` writes no
 * EXIF: no position, no phone model, no time in the file (the server checks again).
 */
object PhotoReducer {
    const val MAX_SIDE = 1024
    const val MAX_BYTES = 60 * 1024
    private const val START_QUALITY = 80
    private const val MIN_QUALITY = 40
    private const val QUALITY_STEP = 10
    private const val SHRINK = 0.75

    fun reduce(source: Bitmap): ByteArray {
        var bmp = scaled(source, MAX_SIDE)
        while (true) {
            var q = START_QUALITY
            while (q >= MIN_QUALITY) {
                val out = ByteArrayOutputStream()
                bmp.compress(Bitmap.CompressFormat.JPEG, q, out)
                if (out.size() <= MAX_BYTES) return out.toByteArray()
                q -= QUALITY_STEP
            }
            bmp = scaled(bmp, (maxOf(bmp.width, bmp.height) * SHRINK).toInt())
        }
    }

    private fun scaled(
        b: Bitmap,
        side: Int,
    ): Bitmap {
        val long = maxOf(b.width, b.height)
        if (long <= side) return b
        val k = side.toDouble() / long
        return b.scale((b.width * k).toInt().coerceAtLeast(1), (b.height * k).toInt().coerceAtLeast(1))
    }
}

/**
 * Sends a note to the OSM notes API (V12), anonymous: only when the user taps « Envoyer »
 * after reading the text. [base]: `https://api.openstreetmap.org` in the app; the tests use
 * a local server or the sandbox (`https://master.apis.dev.openstreetmap.org`), never the
 * production API.
 */
class OsmNotes(
    private val base: String,
    private val userAgent: String = "skimap-android",
) {
    /** The URL of the note created, or an error message. */
    fun send(note: OsmNote): kotlin.Result<String> =
        runCatching {
            val c = URI(base.trimEnd('/') + "/api/0.6/notes.json").toURL().openConnection() as HttpURLConnection
            try {
                c.requestMethod = "POST"
                c.connectTimeout = TIMEOUT_MS
                c.readTimeout = TIMEOUT_MS
                c.doOutput = true
                c.setRequestProperty("User-Agent", userAgent)
                c.setRequestProperty("Content-Type", "application/x-www-form-urlencoded")
                val body = note.formBody.toByteArray()
                c.setFixedLengthStreamingMode(body.size)
                c.outputStream.use { it.write(body) }
                check(c.responseCode == HTTP_OK) { "HTTP ${c.responseCode}" }
                val json = org.json.JSONObject(c.inputStream.use { it.readBytes().decodeToString() })
                val id = json.getJSONObject("properties").getLong("id")
                webUrl(base) + "/note/$id"
            } finally {
                c.disconnect()
            }
        }

    companion object {
        const val PRODUCTION = "https://api.openstreetmap.org"
        const val SANDBOX = "https://master.apis.dev.openstreetmap.org"
        private const val TIMEOUT_MS = 20_000
        private const val HTTP_OK = 200

        /** The website of an API (where the note is read). */
        fun webUrl(base: String): String = if (base.trimEnd('/') == PRODUCTION) "https://www.openstreetmap.org" else base.trimEnd('/')
    }
}
