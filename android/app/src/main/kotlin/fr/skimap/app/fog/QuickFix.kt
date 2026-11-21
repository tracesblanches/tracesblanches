package fr.skimap.app.fog

import android.location.Location
import android.location.LocationManager

/**
 * A position of the bearing screen (#181): where it is, how precise, from the GPS or not,
 * and when it was measured (a position not refreshed keeps its time).
 */
data class Fix(
    val lon: Double,
    val lat: Double,
    /** Radius of 68 % confidence, metres; `null` when the provider gives none. */
    val accuracyM: Float?,
    /** From the GPS itself (`gps` provider), not the network or the fused provider. */
    val gps: Boolean,
    /** Measured at, ms since the epoch. */
    val timeMs: Long,
    val altitudeM: Double,
    /** Direction of travel, degrees from true north, when moving. */
    val courseDeg: Double? = null,
) {
    companion object {
        /** Above this speed the direction of travel is known, m/s. */
        const val MOVING_MPS = 0.6f

        fun of(l: Location) =
            Fix(
                lon = l.longitude,
                lat = l.latitude,
                accuracyM = if (l.hasAccuracy()) l.accuracy else null,
                gps = l.provider == LocationManager.GPS_PROVIDER,
                timeMs = l.time,
                altitudeM = if (l.hasAltitude()) l.altitude else 0.0,
                courseDeg = if (l.hasBearing() && l.speed > MOVING_MPS) l.bearing.toDouble() else null,
            )
    }
}

/**
 * Quick start of the position of the bearing screen (#181): the GPS alone took about 25 s
 * to give a first fix on a Pixel 8 Pro. The first position comes from the last one the
 * phone knows (at most [LAST_KNOWN_MAX_MS] old) or from the fused / network provider,
 * and the GPS replaces it as soon as it has a fix; a quick position never replaces a GPS
 * fix less than [GPS_STALE_MS] old.
 */
object QuickFix {
    /** A GPS fix older than this may give way to a quick position, ms. */
    const val GPS_STALE_MS = 30_000L

    /** A last known position older than this is not shown, ms. */
    const val LAST_KNOWN_MAX_MS = 30 * 60_000L

    /** Whether [candidate] replaces [current] at [nowMs]. */
    fun replaces(
        current: Fix?,
        candidate: Fix,
        nowMs: Long,
    ): Boolean =
        when {
            current == null -> true
            candidate.timeMs < current.timeMs -> false
            candidate.gps -> true
            !current.gps -> true
            else -> nowMs - current.timeMs > GPS_STALE_MS
        }

    /** The freshest of the last known positions [fixes], if not older than [LAST_KNOWN_MAX_MS]. */
    fun freshest(
        fixes: List<Fix>,
        nowMs: Long,
    ): Fix? = fixes.filter { nowMs - it.timeMs <= LAST_KNOWN_MAX_MS }.maxByOrNull { it.timeMs }
}
