package fr.skimap.sport

import android.content.Context
import androidx.core.content.edit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONObject
import java.io.File

/** Where one outing is in its way to a destination. */
enum class Step { OFF, PENDING, UPLOADING, DONE, FAILED }

/** One destination of an outing: its step, and the upload or activity id, or the error. */
data class Delivery(
    val step: Step,
    val detail: String? = null,
) {
    fun toJson(): JSONObject = JSONObject().put("step", step.name).put("detail", detail)

    companion object {
        val OFF = Delivery(Step.OFF)

        fun of(o: JSONObject?): Delivery =
            o?.let { Delivery(Step.valueOf(it.getString("step")), it.optString("detail").takeIf { _ -> !it.isNull("detail") }) } ?: OFF
    }
}

/** An outing in the queue: what is sent (its own copy of the GPX) and where it is. */
data class Queued(
    val outing: Outing,
    val health: Delivery,
    val strava: Delivery,
    val queuedMs: Long,
) {
    val waiting: Boolean get() = health.step in ACTIVE || strava.step in ACTIVE

    fun toJson(): JSONObject =
        JSONObject()
            .put("id", outing.id)
            .put("gpx", outing.gpx)
            .put("name", outing.name)
            .put("description", outing.description)
            .put("sport", outing.sport.name)
            .put("start_ms", outing.startMs)
            .put("end_ms", outing.endMs)
            .put("distance_km", outing.distanceKm)
            .put("queued_ms", queuedMs)
            .put("health", health.toJson())
            .put("strava", strava.toJson())

    companion object {
        val ACTIVE = setOf(Step.PENDING, Step.UPLOADING)

        fun of(o: JSONObject): Queued =
            Queued(
                outing =
                    Outing(
                        id = o.getString("id"),
                        gpx = o.getString("gpx"),
                        name = o.getString("name"),
                        description = o.getString("description"),
                        sport = SportType.of(o.optString("sport")),
                        startMs = o.getLong("start_ms"),
                        endMs = o.getLong("end_ms"),
                        distanceKm = o.optDouble("distance_km"),
                    ),
                health = Delivery.of(o.optJSONObject("health")),
                strava = Delivery.of(o.optJSONObject("strava")),
                queuedMs = o.optLong("queued_ms"),
            )
    }
}

/**
 * What the module keeps, in the app's private storage: the choices (send automatically),
 * the Strava tokens and the pending OAuth state (preferences, not backed up: the app has
 * `allowBackup=false`), and the queue (`files/sport/queue/<id>.json` and the GPX copy),
 * so that an outing waits for the network across restarts.
 */
@Suppress("TooManyFunctions") // getter and setter of each thing kept, and the queue
class SportStore(
    context: Context,
) {
    private val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    private val dir = File(context.filesDir, "sport/queue")

    fun autoSend(): Boolean = prefs.getBoolean(KEY_AUTO, false)

    fun setAutoSend(on: Boolean) {
        prefs.edit { putBoolean(KEY_AUTO, on) }
        changed()
    }

    /** skimap-server (OAuth exchange); a test points it at its fake. */
    fun server(): String = prefs.getString(KEY_SERVER, null) ?: DEFAULT_SERVER

    /** The Strava API; a test points it at its fake. */
    fun api(): String = prefs.getString(KEY_API, null) ?: StravaClient.DEFAULT_API

    fun setEndpoints(
        server: String,
        api: String,
    ) = prefs.edit {
        putString(KEY_SERVER, server)
        putString(KEY_API, api)
    }

    fun tokens(): StravaTokens? {
        val access = prefs.getString(KEY_ACCESS, null)
        val refresh = prefs.getString(KEY_REFRESH, null)
        return if (access == null || refresh == null) {
            null
        } else {
            StravaTokens(access, refresh, prefs.getLong(KEY_EXPIRES, 0), prefs.getString(KEY_ATHLETE, null))
        }
    }

    fun setTokens(t: StravaTokens?) {
        prefs.edit {
            if (t == null) {
                remove(KEY_ACCESS)
                remove(KEY_REFRESH)
                remove(KEY_EXPIRES)
                remove(KEY_ATHLETE)
            } else {
                putString(KEY_ACCESS, t.access)
                putString(KEY_REFRESH, t.refresh)
                putLong(KEY_EXPIRES, t.expiresAt)
                putString(KEY_ATHLETE, t.athlete)
            }
        }
        changed()
    }

    /** The `state` of the sign-in under way (checked at the callback). */
    fun oauthState(): String? = prefs.getString(KEY_STATE, null)

    fun setOauthState(s: String?) = prefs.edit { if (s == null) remove(KEY_STATE) else putString(KEY_STATE, s) }

    /** Outings queued, newest first. */
    @Synchronized
    fun queue(): List<Queued> =
        (dir.listFiles { f -> f.name.endsWith(".json") } ?: emptyArray())
            .mapNotNull { f -> runCatching { Queued.of(JSONObject(f.readText())) }.getOrNull() }
            .sortedByDescending { it.queuedMs }

    fun get(id: String): Queued? = queue().firstOrNull { it.outing.id == id }

    /** Adds [outing] (its GPX copied into the queue) or updates its destinations. */
    @Synchronized
    fun put(q: Queued): Queued {
        dir.mkdirs()
        val gpx = File(dir, "${safe(q.outing.id)}.gpx")
        val src = File(q.outing.gpx)
        if (src.absolutePath != gpx.absolutePath) src.copyTo(gpx, overwrite = true)
        val stored = q.copy(outing = q.outing.copy(gpx = gpx.absolutePath))
        val tmp = File(dir, "${safe(q.outing.id)}.json.tmp")
        tmp.writeText(stored.toJson().toString())
        tmp.renameTo(File(dir, "${safe(q.outing.id)}.json"))
        changed()
        return stored
    }

    /** Updates one destination of a queued outing. */
    @Synchronized
    fun update(
        id: String,
        health: Delivery? = null,
        strava: Delivery? = null,
    ) {
        val q = get(id) ?: return
        put(q.copy(health = health ?: q.health, strava = strava ?: q.strava))
    }

    private fun safe(id: String): String = id.replace(Regex("[^A-Za-z0-9._-]"), "_")

    private fun changed() {
        revisionFlow.value++
    }

    companion object {
        const val DEFAULT_SERVER = "https://skimap.warnier.net"
        private const val PREFS = "skimap_sport"
        private const val KEY_AUTO = "auto_send"
        private const val KEY_SERVER = "server"
        private const val KEY_API = "api"
        private const val KEY_ACCESS = "strava_access"
        private const val KEY_REFRESH = "strava_refresh"
        private const val KEY_EXPIRES = "strava_expires_at"
        private const val KEY_ATHLETE = "strava_athlete"
        private const val KEY_STATE = "strava_state"

        private val revisionFlow = MutableStateFlow(0L)

        /** Bumped at every change of the queue, the tokens or the choices (the screens follow it). */
        val revision: StateFlow<Long> = revisionFlow.asStateFlow()
    }
}
