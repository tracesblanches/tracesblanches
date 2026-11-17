package fr.skimap.app.prep

import android.content.Context
import android.os.SystemClock
import android.util.Log
import androidx.core.content.edit
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import fr.skimap.app.R
import fr.skimap.app.appLang
import fr.skimap.app.day.DayStore
import fr.skimap.app.fog.Outings
import fr.skimap.app.ui.CoreErrors
import fr.skimap.core.prepRun
import org.json.JSONObject
import java.io.File
import java.time.LocalDate
import java.util.concurrent.TimeUnit

/**
 * Preparation mode (E2 #126): on charge and on Wi-Fi (WorkManager constraints), once a
 * day, the group recalibrated on the outings not learned yet (C11), then the next three
 * days of every pack prepared (`prep_run`: mornings and plans of the days the pack's
 * forecast covers). Time, battery level and temperature are recorded for each run
 * ([LAST_FILE]). In multi-day ski mode nothing runs.
 */
object PrepWork {
    private const val PREFS = "skimap_prep"
    private const val KEY_AUTO = "auto"
    private const val KEY_SKI = "ski_mode"
    private const val PERIODIC = "prep-daily"
    private const val NOW = "prep-now"
    const val DAYS = 3u
    const val LAST_FILE = "last.json"

    private fun prefs(c: Context) = c.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun auto(c: Context) = prefs(c).getBoolean(KEY_AUTO, true)

    fun skiMode(c: Context) = prefs(c).getBoolean(KEY_SKI, false)

    fun setSkiMode(
        c: Context,
        on: Boolean,
    ) = prefs(c).edit { putBoolean(KEY_SKI, on) }

    fun setAuto(
        c: Context,
        on: Boolean,
    ) {
        prefs(c).edit { putBoolean(KEY_AUTO, on) }
        schedule(c)
    }

    /** Where the prepared days are written. */
    fun dir(c: Context): File = File(c.filesDir, "prep")

    /** The daily job, with its constraints (KEEP: scheduled once). */
    fun schedule(c: Context) {
        val wm = WorkManager.getInstance(c)
        if (!auto(c)) {
            wm.cancelUniqueWork(PERIODIC)
            return
        }
        val req =
            PeriodicWorkRequestBuilder<PrepWorker>(1, TimeUnit.DAYS)
                .setConstraints(constraints())
                .build()
        wm.enqueueUniquePeriodicWork(PERIODIC, ExistingPeriodicWorkPolicy.KEEP, req)
    }

    /** The constraints of the preparation: charging, unmetered network (Wi-Fi), idle not required. */
    fun constraints(): Constraints =
        Constraints
            .Builder()
            .setRequiresCharging(true)
            // The night, as a rule: the phone idle on its charger.
            .setRequiresDeviceIdle(true)
            .setRequiredNetworkType(NetworkType.UNMETERED)
            .setRequiresStorageNotLow(true)
            .build()

    /** « Préparer maintenant »: at once, without constraints (the user asked). */
    fun runNow(c: Context) {
        WorkManager.getInstance(c).enqueueUniqueWork(NOW, ExistingWorkPolicy.REPLACE, OneTimeWorkRequestBuilder<PrepWorker>().build())
    }

    /** One preparation of every pack: what it did, measured. */
    fun prepare(context: Context): JSONObject {
        val t0 = SystemClock.elapsedRealtime()
        val before = Battery.now(context)
        val store = DayStore(context)
        // The group learns from the outings ended and not learned yet (C11).
        Outings
            .plansRoot(context)
            .listFiles()
            .orEmpty()
            .forEach { d -> runCatching { store.learnFrom(d.path) } }
        val out = dir(context).also { it.mkdirs() }
        val today = LocalDate.now().toString()
        val group = Outings.groupJson(context)
        // The outing the morning starts from: the last choice, else the core's default (#170).
        val outing = store.outing
        val days = mutableListOf<String>()
        val skipped = mutableListOf<String>()
        for (p in Outings.packDirs(context)) {
            runCatching { prepRun(p.path, Outings.workDir(context).path, out.path, group, outing, today, DAYS, appLang()) }
                .onSuccess { r ->
                    days += r.days.map { "${it.siteId} ${it.date}" }
                    skipped += r.skipped.map { context.getString(R.string.label_value, p.name, it) }
                }.onFailure {
                    skipped += context.getString(R.string.label_value, p.name, CoreErrors.text(it, context.resources))
                    Log.w("skimap", "prep ${p.name}", it)
                }
        }
        val after = Battery.now(context)
        val o =
            JSONObject()
                .put("at_ms", System.currentTimeMillis())
                .put("ms", SystemClock.elapsedRealtime() - t0)
                .put("days", days.joinToString())
                .put("skipped", skipped.joinToString(" ; "))
                .put("battery_pct", "${before?.levelPct?.toInt()} → ${after?.levelPct?.toInt()}")
                .put("battery_c", "${before?.tempC} → ${after?.tempC}")
        File(out, LAST_FILE).writeText(o.toString())
        Log.i("skimap", "prep: $o")
        return o
    }
}

/** The work of [PrepWork]. */
class PrepWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        if (PrepWork.skiMode(applicationContext)) return Result.success()
        PrepWork.prepare(applicationContext)
        return Result.success()
    }
}
