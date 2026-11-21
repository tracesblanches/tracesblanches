package fr.skimap.sport

import java.io.File

/** The practice of the outing, as the files and Health Connect say it. */
enum class SportType {
    /** Cross-country skiing on groomed trails (ski de fond). */
    NORDIC_SKI,

    /** Nordic backcountry skiing, off the groomed trails (ski de randonnée nordique). */
    BACKCOUNTRY_SKI,
}

/** One point of the exported trace. */
data class OutingPoint(
    val timeMs: Long,
    val lat: Double,
    val lon: Double,
    /** Altitude, metres, when known. */
    val ele: Double?,
    /** Distance skied from the start, metres. */
    val distanceM: Double,
    val heartBpm: Int?,
)

/** A heart rate sample read for the outing (Health Connect, from a watch). */
data class HeartSample(
    val timeMs: Long,
    val bpm: Int,
)

/**
 * A finished outing exported by the core (crates/ffi `export_outing`): the GPX 1.1 and FIT
 * files to share, and what is written to Health Connect. [id] is stable for the outing
 * (its directory): written again, the session in Health Connect is replaced.
 */
data class SharedOuting(
    val id: String,
    val title: String,
    val sport: SportType,
    val startMs: Long,
    val endMs: Long,
    val distanceM: Double,
    val ascentM: Double?,
    val points: List<OutingPoint>,
    val gpx: File,
    val fit: File,
    /** The recap, written as the session's notes. */
    val notes: String? = null,
)

/** The file shared: FIT (sport kept: Strava, Garmin Connect) or GPX (every app). */
enum class ShareFormat(
    val mime: String,
) {
    FIT("application/vnd.ant.fit"),
    GPX("application/gpx+xml"),
    ;

    fun file(o: SharedOuting): File = if (this == FIT) o.fit else o.gpx
}
