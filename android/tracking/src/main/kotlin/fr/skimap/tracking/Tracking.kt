package fr.skimap.tracking

import android.content.Context
import android.content.Intent
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** What the screens show of the outing. */
sealed interface TrackingState {
    /** No outing followed. */
    data object Idle : TrackingState

    /** The location service follows the plan in [planDir]. */
    data class Running(
        val planDir: String,
        val guidance: Guidance,
        val gps: GpsRequest?,
        /** Last thing said, for the screen (and the deaf). */
        val lastSaid: String?,
    ) : TrackingState

    /** The outing is over (or stopped). */
    data class Ended(
        val planDir: String,
        val recap: Recap?,
        val gpx: String?,
    ) : TrackingState
}

/**
 * The outing seen from the app: the state the location service publishes, and the
 * commands sent to it (intents: the same ones the notification's actions send).
 */
object Tracking {
    const val ACTION_START = "fr.skimap.tracking.START"
    const val ACTION_STOP = "fr.skimap.tracking.STOP"
    const val ACTION_ACCEPT = "fr.skimap.tracking.ACCEPT"
    const val ACTION_DECLINE = "fr.skimap.tracking.DECLINE"
    const val ACTION_REFUSE_SWITCH = "fr.skimap.tracking.REFUSE_SWITCH"
    const val ACTION_BACK = "fr.skimap.tracking.BACK"
    const val ACTION_SAY = "fr.skimap.tracking.SAY"

    /** « La voiture est ici » (K4), with [EXTRA_LON] and [EXTRA_LAT]. */
    const val ACTION_CAR = "fr.skimap.tracking.CAR"
    const val EXTRA_LON = "lon"
    const val EXTRA_LAT = "lat"

    /** A car set before the outing (no outing running), kept for the next one within [CAR_KEEP_MS]. */
    const val KEY_CAR_LON = "car_lon"
    const val KEY_CAR_LAT = "car_lat"
    const val KEY_CAR_AT = "car_at"
    const val CAR_KEEP_MS = 12 * 3_600_000L

    /** Answers to the proposal to choose another group profile (X14). */
    const val ACTION_PROFILE_ACCEPT = "fr.skimap.tracking.PROFILE_ACCEPT"
    const val ACTION_PROFILE_DECLINE = "fr.skimap.tracking.PROFILE_DECLINE"

    /** Short sounds left / right / off the route / reassurance (K6): on, and their volume (0 to 1). */
    const val KEY_EARCONS = "earcons"
    const val KEY_EARCON_VOLUME = "earcon_volume"
    const val EXTRA_PLAN_DIR = "plan_dir"

    /** `fused` (default when the platform has it) or `gps`: the A/B of report 09 on the Pixel. */
    const val EXTRA_PROVIDER = "provider"

    /**
     * Replay of recorded inputs (instrumented tests): the inputs carry their own clock, so
     * the service tells the core nothing of its own (no screen event, no platform fix at the
     * wall-clock time; the GPS requests are recorded, not given to the platform); everything
     * else is the same.
     */
    const val EXTRA_REPLAY = "replay"

    /** Preferences of the module: the plan being followed (resumed after a restart). */
    const val PREFS = "skimap_tracking"
    const val KEY_ACTIVE = "active_plan"

    private val mutable = MutableStateFlow<TrackingState>(TrackingState.Idle)
    val state: StateFlow<TrackingState> = mutable.asStateFlow()

    internal fun publish(s: TrackingState) {
        mutable.value = s
    }

    /** The short sounds of K6: on or off, and their volume (0 to 1); read when the outing starts. */
    fun setEarcons(
        context: Context,
        enabled: Boolean,
        volume: Float,
    ) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit {
            putBoolean(KEY_EARCONS, enabled)
            putFloat(KEY_EARCON_VOLUME, volume.coerceIn(0f, 1f))
        }
    }

    fun earcons(context: Context): EarconSettings {
        val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        return EarconSettings(p.getBoolean(KEY_EARCONS, true), p.getFloat(KEY_EARCON_VOLUME, EarconSettings.DEFAULT_VOLUME))
    }

    /** Plan of an outing not ended (the process was stopped): offered to resume. */
    fun activePlan(context: Context): String? = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_ACTIVE, null)

    /** « Valider la journée »: starts the location service (a user gesture, report 09). */
    fun start(
        context: Context,
        service: Class<out TrackingService>,
        planDir: String,
    ) {
        val i = Intent(context, service).setAction(ACTION_START).putExtra(EXTRA_PLAN_DIR, planDir)
        ContextCompat.startForegroundService(context, i)
    }

    fun command(
        context: Context,
        service: Class<out TrackingService>,
        action: String,
    ) {
        context.startService(Intent(context, service).setAction(action))
    }

    /**
     * « La voiture est ici » (a long touch on the map, K4): to the outing running (the way
     * back to the car follows), else kept for the next outing started within [CAR_KEEP_MS]
     * (the car is otherwise recorded on its own at the start). Nothing to set beforehand.
     */
    fun setCar(
        context: Context,
        service: Class<out TrackingService>,
        lat: Double,
        lon: Double,
        nowMs: Long = System.currentTimeMillis(),
    ) {
        if (state.value is TrackingState.Running) {
            context.startService(Intent(context, service).setAction(ACTION_CAR).putExtra(EXTRA_LON, lon).putExtra(EXTRA_LAT, lat))
        } else {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit {
                putLong(KEY_CAR_LON, lon.toRawBits())
                putLong(KEY_CAR_LAT, lat.toRawBits())
                putLong(KEY_CAR_AT, nowMs)
            }
        }
    }

    /** The car set before the outing (longitude, latitude), if recent; taken once. */
    fun takePendingCar(
        context: Context,
        nowMs: Long,
    ): Pair<Double, Double>? {
        val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val at = p.getLong(KEY_CAR_AT, -1L)
        if (at < 0) return null
        val car = Double.fromBits(p.getLong(KEY_CAR_LON, 0L)) to Double.fromBits(p.getLong(KEY_CAR_LAT, 0L))
        p.edit {
            remove(KEY_CAR_LON)
            remove(KEY_CAR_LAT)
            remove(KEY_CAR_AT)
        }
        return car.takeIf { nowMs - at in 0..CAR_KEEP_MS }
    }
}
