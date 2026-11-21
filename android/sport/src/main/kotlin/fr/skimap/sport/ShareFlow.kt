package fr.skimap.sport

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.ActivityResultRegistry
import androidx.core.content.FileProvider
import androidx.core.content.edit
import androidx.health.connect.client.PermissionController
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.io.IOException
import java.time.ZoneId

/** How a share went: written to Health Connect or not, the chooser opened or not. */
data class ShareResult(
    /** Records written to Health Connect (0: not written). */
    val healthRecords: Int,
    /** The share menu opened with the file. */
    val shared: Boolean,
)

/**
 * The share of a finished outing (#63), one gesture: the outing exported by the core as
 * GPX and FIT, written to Health Connect when the phone has it and the user allows it
 * (asked here, at the first share, never at the start; declined, it is not asked again,
 * granted later in Health Connect's settings, it is written), then the file given to the
 * share menu (ACTION_SEND through the [FileProvider]: Strava, Komoot, Garmin Connect, a
 * mail...). No account, no key, nothing sent by the app itself.
 *
 * Built in the activity's `onCreate` (the permission request is registered then).
 */
class ShareFlow(
    private val activity: ComponentActivity,
    private val health: HealthApi = HealthConnectApi(activity),
    registry: ActivityResultRegistry = activity.activityResultRegistry,
    private val zone: () -> ZoneId = { ZoneId.systemDefault() },
) {
    private var answer: CompletableDeferred<Set<String>>? = null
    private val ask: ActivityResultLauncher<Set<String>> =
        registry.register(KEY, activity, PermissionController.createRequestPermissionResultContract()) { granted ->
            answer?.complete(granted)
        }

    /**
     * Shares the outing [export] gives (the heart rate read for it: may be empty; `null`:
     * nothing to share) as [format]; [done] with how it went, on the main thread.
     */
    fun share(
        format: ShareFormat,
        export: suspend (List<HeartSample>) -> SharedOuting?,
        done: (ShareResult) -> Unit = {},
    ): Job =
        activity.lifecycleScope.launch {
            val granted = permissions()
            val first = withContext(Dispatchers.IO) { export(emptyList()) }
            if (first == null) {
                done(ShareResult(0, false))
                return@launch
            }
            val o = withHeart(first, granted, export)
            val written = write(o, granted)
            val shared = open(o, format)
            Log.i(TAG, "share ${o.id}: ${format.name}, health connect $written records, menu $shared")
            done(ShareResult(written, shared))
        }

    /** The permissions granted; asked once if none of them is. */
    private suspend fun permissions(): Set<String> {
        val present = health.available()
        val have = if (present) runCatching { health.granted() }.getOrDefault(emptySet()) else emptySet()
        val prefs = activity.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val first = present && HealthPermissions.SESSION !in have && !prefs.getBoolean(ASKED, false)
        if (!first) return have
        prefs.edit { putBoolean(ASKED, true) }
        return request() ?: have
    }

    /** Health Connect's dialog; `null` when it cannot be shown. */
    private suspend fun request(): Set<String>? {
        val d = CompletableDeferred<Set<String>>()
        answer = d
        return try {
            ask.launch(HealthPermissions.ALL)
            d.await()
        } catch (e: ActivityNotFoundException) {
            Log.w(TAG, "health connect permissions", e)
            null
        }
    }

    /** The outing exported again with the heart rate of its hours when it can be read. */
    private suspend fun withHeart(
        o: SharedOuting,
        granted: Set<String>,
        export: suspend (List<HeartSample>) -> SharedOuting?,
    ): SharedOuting {
        val heart = if (HealthPermissions.HEART in granted) heart(o) else emptyList()
        return if (heart.isEmpty()) o else withContext(Dispatchers.IO) { export(heart) } ?: o
    }

    private suspend fun heart(o: SharedOuting): List<HeartSample> =
        try {
            health.heart(o.startMs, o.endMs)
        } catch (e: SecurityException) {
            Log.w(TAG, "heart rate", e)
            emptyList()
        } catch (e: IOException) {
            Log.w(TAG, "heart rate", e)
            emptyList()
        }

    private suspend fun write(
        o: SharedOuting,
        granted: Set<String>,
    ): Int {
        val records = HealthRecords.of(o, granted, zone(), HealthRecords.phone())
        if (records.isEmpty()) return 0
        return try {
            health.insert(records).size
        } catch (e: SecurityException) {
            Log.w(TAG, "health connect", e)
            0
        } catch (e: IOException) {
            Log.w(TAG, "health connect", e)
            0
        } catch (e: IllegalArgumentException) {
            Log.w(TAG, "health connect", e)
            0
        }
    }

    private fun open(
        o: SharedOuting,
        format: ShareFormat,
    ): Boolean =
        try {
            activity.startActivity(OutingShare.chooser(activity, o, format))
            true
        } catch (e: ActivityNotFoundException) {
            Log.w(TAG, "share", e)
            false
        } catch (e: IllegalArgumentException) {
            // The file is not under the provider's directory.
            Log.w(TAG, "share", e)
            false
        }

    companion object {
        const val TAG = "skimap"
        private const val KEY = "sport-health-permissions"
        private const val PREFS = "sport"
        private const val ASKED = "health_asked"
    }
}

/** The share menu with the outing's file. */
object OutingShare {
    /** Where the core writes the files shared (cache, under the [FileProvider]'s path). */
    fun dir(context: Context): File = File(context.cacheDir, "sport-share")

    fun authority(context: Context): String = "${context.packageName}.sport.files"

    /** ACTION_SEND of the file of [format], readable by the app chosen, in the system's chooser. */
    fun chooser(
        context: Context,
        o: SharedOuting,
        format: ShareFormat,
    ): Intent {
        val file = format.file(o)
        val uri = FileProvider.getUriForFile(context, authority(context), file)
        val send =
            Intent(Intent.ACTION_SEND)
                .setType(format.mime)
                .putExtra(Intent.EXTRA_STREAM, uri)
                .putExtra(Intent.EXTRA_SUBJECT, o.title)
                .putExtra(Intent.EXTRA_TITLE, file.name)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        send.clipData = ClipData.newRawUri(file.name, uri)
        return Intent.createChooser(send, context.getString(R.string.sport_share_chooser))
    }
}
