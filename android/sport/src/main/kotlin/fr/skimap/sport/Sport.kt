package fr.skimap.sport

import android.content.Context
import android.util.Log
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import kotlinx.coroutines.delay
import java.io.File
import java.io.IOException
import java.util.concurrent.TimeUnit

/** Writes an outing to Health Connect ([HealthConnectSender]; a fake in the JVM tests). */
interface HealthWriter {
    /** Health Connect is on the phone and up to date. */
    fun available(): Boolean

    suspend fun granted(): Boolean

    suspend fun insert(outing: Outing): String
}

/**
 * Sending the outing (K11). [send] puts it in the queue with its destinations — Health
 * Connect when the phone has it, Strava when the athlete is connected — and gives each
 * destination to WorkManager: Health Connect at once (on the phone), Strava when there is
 * a network ([NetworkType.CONNECTED]: in airplane mode or out of coverage, it waits, then
 * goes by itself, also after a restart). Nothing runs otherwise.
 */
object Sport {
    const val TAG = "skimap"
    const val WORK_HEALTH = "sport-health"
    const val WORK_STRAVA = "sport-strava"

    /** Strava of the store's endpoints (tests: their fake). */
    @Volatile var strava: (SportStore) -> StravaClient = { StravaClient(it.server(), it.api()) }

    @Volatile var health: (Context) -> HealthWriter = { HealthConnectSender(it) }

    /** Strava processes an upload in a few seconds: polled this often, this many times per run. */
    @Volatile var pollDelayMs = 2_000L

    @Volatile var polls = 8

    private const val BACKOFF_S = 30L

    /**
     * Queues [outing] for its destinations: Health Connect when the phone has it, Strava
     * when the athlete is connected. A destination not set up stays off, noted; [resume]
     * takes it up once it is.
     */
    fun send(
        context: Context,
        outing: Outing,
    ): Queued {
        val store = SportStore(context)
        val hc = health(context).available()
        val connected = store.tokens() != null
        val previous = store.get(outing.id)
        val q =
            store.put(
                Queued(
                    outing = outing,
                    health = again(previous?.health, hc, if (hc) NOT_ALLOWED else UNAVAILABLE),
                    strava = again(previous?.strava, connected, NOT_CONNECTED),
                    queuedMs = previous?.queuedMs ?: System.currentTimeMillis(),
                ),
            )
        schedule(context, q)
        Log.i(TAG, "sport: ${outing.id} queued, health ${q.health.step}, strava ${q.strava.step}")
        return q
    }

    /**
     * After Health Connect was allowed or Strava connected: the outings that waited for it
     * are sent.
     */
    fun resume(context: Context) {
        val store = SportStore(context)
        val connected = store.tokens() != null
        for (q in store.queue()) {
            val h = q.health.takeUnless { it.step != Step.DONE && it.detail == NOT_ALLOWED } ?: Delivery(Step.PENDING)
            val s = q.strava.takeUnless { connected && it.step != Step.DONE && it.detail == NOT_CONNECTED } ?: Delivery(Step.PENDING)
            if (h != q.health || s != q.strava) store.update(q.outing.id, h, s)
        }
        schedule(context)
    }

    /** At the end of the outing, when the user asked for it (« Envoyer automatiquement »). */
    fun sendIfAuto(
        context: Context,
        outing: Outing,
    ): Queued? = if (SportStore(context).autoSend()) send(context, outing) else null

    /** A destination already done stays done; otherwise it is tried (again). */
    private fun again(
        d: Delivery?,
        on: Boolean,
        why: String,
    ): Delivery =
        when {
            d?.step == Step.DONE || d?.step == Step.UPLOADING -> d
            on -> Delivery(Step.PENDING)
            else -> Delivery(Step.OFF, why)
        }

    /** Gives the queue to WorkManager (also after the athlete connects Strava). */
    fun schedule(
        context: Context,
        q: Queued? = null,
    ) {
        val wm = WorkManager.getInstance(context)
        if (q == null || q.health.step == Step.PENDING) {
            val r =
                OneTimeWorkRequestBuilder<SportWorker>()
                    .setInputData(workDataOf(SportWorker.TARGET to SportWorker.HEALTH))
                    .addTag(TAG_WORK)
                    .build()
            wm.enqueueUniqueWork(WORK_HEALTH, ExistingWorkPolicy.APPEND_OR_REPLACE, r)
        }
        if (q == null || q.strava.step in Queued.ACTIVE) {
            val r =
                OneTimeWorkRequestBuilder<SportWorker>()
                    .setInputData(workDataOf(SportWorker.TARGET to SportWorker.STRAVA))
                    .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                    .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, BACKOFF_S, TimeUnit.SECONDS)
                    .addTag(TAG_WORK)
                    .build()
            wm.enqueueUniqueWork(WORK_STRAVA, ExistingWorkPolicy.APPEND_OR_REPLACE, r)
        }
    }

    /** Tag of every sending work. */
    const val TAG_WORK = "skimap-sport"

    /** Health Connect's part of the queue. */
    suspend fun runHealth(context: Context) {
        val store = SportStore(context)
        val writer = health(context)
        for (q in store.queue().filter { it.health.step == Step.PENDING }) {
            val d =
                try {
                    if (!writer.granted()) {
                        Delivery(Step.OFF, NOT_ALLOWED)
                    } else {
                        Delivery(Step.DONE, writer.insert(q.outing))
                    }
                } catch (e: SecurityException) {
                    Delivery(Step.OFF, NOT_ALLOWED).also { Log.w(TAG, "sport: health connect", e) }
                } catch (e: IOException) {
                    Delivery(Step.FAILED, e.message).also { Log.w(TAG, "sport: health connect", e) }
                } catch (e: IllegalArgumentException) {
                    Delivery(Step.FAILED, e.message).also { Log.w(TAG, "sport: health connect", e) }
                } catch (e: IllegalStateException) {
                    Delivery(Step.FAILED, e.message).also { Log.w(TAG, "sport: health connect", e) }
                }
            Log.i(TAG, "sport: ${q.outing.id} health connect ${d.step}")
            store.update(q.outing.id, health = d)
        }
    }

    /** Strava's part of the queue; `true`: something must be tried again later. */
    suspend fun runStrava(context: Context): Boolean {
        val store = SportStore(context)
        val client = strava(store)
        var later = false
        for (q in store.queue().filter { it.strava.step in Queued.ACTIVE }) {
            if (store.tokens() == null) {
                // Not connected (or no longer): the outing waits for [resume].
                store.update(q.outing.id, strava = Delivery(Step.OFF, NOT_CONNECTED))
            } else {
                later = stravaTry(store, client, q) || later
            }
        }
        return later
    }

    /** One outing to Strava; `true`: to try again later. */
    private suspend fun stravaTry(
        store: SportStore,
        client: StravaClient,
        q: Queued,
    ): Boolean =
        try {
            stravaOne(store, client, q)
        } catch (e: StravaException) {
            Log.w(TAG, "sport: strava ${q.outing.id}", e)
            if (e.unauthorized) {
                // Revoked on strava.com: the athlete connects again, the outing waits.
                store.setTokens(null)
                store.update(q.outing.id, strava = Delivery(Step.FAILED, NOT_CONNECTED))
            } else if (!e.retry) {
                store.update(q.outing.id, strava = Delivery(Step.FAILED, e.message))
            }
            e.retry
        } catch (e: IOException) {
            // No network after all (or Strava unreachable): WorkManager tries again.
            Log.w(TAG, "sport: strava ${q.outing.id}", e)
            true
        }

    private suspend fun stravaOne(
        store: SportStore,
        client: StravaClient,
        q: Queued,
    ): Boolean {
        var tokens = store.tokens() ?: return false
        if (tokens.expiring(System.currentTimeMillis() / MS)) {
            tokens = client.refresh(tokens)
            store.setTokens(tokens)
        }
        var status =
            if (q.strava.step == Step.UPLOADING) {
                client.uploadStatus(tokens.access, q.strava.detail!!.toLong())
            } else {
                client.upload(tokens.access, q.outing, File(q.outing.gpx)).also {
                    store.update(q.outing.id, strava = Delivery(Step.UPLOADING, it.id.toString()))
                }
            }
        var n = 0
        while (status.activityId == null && status.error == null && n < polls) {
            delay(pollDelayMs)
            status = client.uploadStatus(tokens.access, status.id)
            n++
        }
        val activity = status.activityId
        return when {
            status.error != null -> {
                store.update(q.outing.id, strava = Delivery(Step.FAILED, status.error))
                false
            }

            activity == null -> {
                // Still processed by Strava: the next run asks again.
                true
            }

            else -> {
                client.updateActivity(tokens.access, activity, q.outing)
                store.update(q.outing.id, strava = Delivery(Step.DONE, activity.toString()))
                Log.i(TAG, "sport: ${q.outing.id} on strava, activity $activity")
                false
            }
        }
    }

    /** Detail of a destination turned off: the permission was not given. */
    const val NOT_ALLOWED = "not_allowed"

    /** Detail of a destination turned off: the phone has no Health Connect. */
    const val UNAVAILABLE = "unavailable"

    /** Detail of a destination turned off: Strava is not connected. */
    const val NOT_CONNECTED = "not_connected"

    private const val MS = 1000L
}

/** One destination of the queue (input [TARGET]: [HEALTH] or [STRAVA]). */
class SportWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result =
        when (inputData.getString(TARGET)) {
            HEALTH -> {
                Sport.runHealth(applicationContext)
                Result.success()
            }

            STRAVA -> {
                if (Sport.runStrava(applicationContext)) Result.retry() else Result.success()
            }

            else -> {
                Result.failure()
            }
        }

    companion object {
        const val TARGET = "target"
        const val HEALTH = "health"
        const val STRAVA = "strava"
    }
}
