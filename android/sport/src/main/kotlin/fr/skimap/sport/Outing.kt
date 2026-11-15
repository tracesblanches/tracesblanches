package fr.skimap.sport

import android.util.Xml
import org.json.JSONArray
import org.json.JSONObject
import org.xmlpull.v1.XmlPullParser
import java.io.File
import java.io.StringReader
import java.time.Instant

/** Strava's sport type of the outing (`sport_type` of the API) and Health Connect's. */
enum class SportType(
    val strava: String,
) {
    /** Cross-country skiing on groomed trails. */
    NORDIC_SKI("NordicSki"),

    /** Nordic backcountry skiing, off the groomed trails (ski de randonnée nordique). */
    BACKCOUNTRY_SKI("BackcountrySki"),
    ;

    companion object {
        fun of(name: String?): SportType = entries.firstOrNull { it.name == name } ?: NORDIC_SKI
    }
}

/** One point of the recorded trace. */
data class TrackPoint(
    val timeMs: Long,
    val lat: Double,
    val lon: Double,
    val ele: Double?,
)

/** The recorded trace: its name and points in time order. */
data class Track(
    val name: String?,
    val points: List<TrackPoint>,
) {
    val startMs: Long get() = points.first().timeMs
    val endMs: Long get() = points.last().timeMs
}

/**
 * An outing to send: the GPX the core wrote at the end ([gpx]), its name, the description
 * (the recap and the conditions of the morning) and the sport type. [id] is stable for the
 * outing: Health Connect replaces the session written with the same id, Strava refuses the
 * same `external_id` twice.
 */
data class Outing(
    val id: String,
    val gpx: String,
    val name: String,
    val description: String,
    val sport: SportType,
    val startMs: Long,
    val endMs: Long,
    val distanceKm: Double,
) {
    companion object {
        /**
         * The outing that ended: the trace [gpx] of the plan in [planDir] (its `plan.json`
         * gives the conditions of the morning), the recap said at the end, the practice.
         */
        fun of(
            planDir: String,
            gpx: String,
            recap: String?,
            distanceKm: Double,
            sport: SportType,
        ): Outing {
            val track = Gpx.read(File(gpx).readText())
            require(track.points.size >= 2) { "trace without two points: $gpx" }
            val plan = File(planDir, "plan.json").takeIf { it.isFile }?.readText()
            val conditions = plan?.let(Conditions::of).orEmpty()
            val name = track.name?.takeIf { it.isNotBlank() } ?: "skimap"
            return Outing(
                id = "${File(planDir).name}-${track.startMs}",
                gpx = gpx,
                name = name,
                description = Description.of(recap, conditions),
                sport = sport,
                startMs = track.startMs,
                endMs = track.endMs,
                distanceKm = distanceKm,
            )
        }
    }
}

/** The description sent with the outing: the recap, the conditions of the morning, the app. */
object Description {
    const val SIGNATURE = "Sortie guidée par skimap (cartes IGN, OpenStreetMap)."

    fun of(
        recap: String?,
        conditions: List<String>,
    ): String =
        buildList {
            recap?.trim()?.takeIf { it.isNotEmpty() }?.let(::add)
            if (conditions.isNotEmpty()) add("Conditions : " + conditions.joinToString(" "))
            add(SIGNATURE)
        }.joinToString("\n\n")
}

/**
 * The conditions of the day in the plan (`plan.json`, crates/plan): the briefing of the
 * morning (A1: snow, weather, trails) and the wax (A2), as the app said them.
 */
object Conditions {
    private val PARTS = listOf("briefing", "wax")

    fun of(planJson: String): List<String> {
        val morning = JSONObject(planJson).optJSONArray("morning") ?: return emptyList()
        val byKey =
            (0 until morning.length())
                .mapNotNull { morning.optJSONArray(it) }
                .associate { part ->
                    part.optString(0) to
                        part
                            .optJSONObject(1)
                            ?.optJSONArray("text")
                            ?.strings()
                            .orEmpty()
                }
        return PARTS.flatMap { byKey[it].orEmpty() }.map(String::trim).filter(String::isNotEmpty)
    }

    private fun JSONArray.strings(): List<String> = (0 until length()).map { optString(it) }
}

/** Reads a GPX 1.1 track (the core writes `trk/trkseg/trkpt` with `time`, maybe `ele`). */
object Gpx {
    fun read(text: String): Track {
        val p = Xml.newPullParser()
        p.setFeature(XmlPullParser.FEATURE_PROCESS_NAMESPACES, false)
        p.setInput(StringReader(text))
        val r = Reader(p)
        var event = p.eventType
        while (event != XmlPullParser.END_DOCUMENT) {
            if (event == XmlPullParser.START_TAG) {
                r.start()
            } else if (event == XmlPullParser.END_TAG) {
                r.end()
            }
            event = p.next()
        }
        return Track(r.name, r.points.sortedBy { it.timeMs })
    }

    /** State of the reading: the track's name, the point being read, the points read. */
    private class Reader(
        val p: XmlPullParser,
    ) {
        val points = ArrayList<TrackPoint>()
        var name: String? = null
        private var point: TrackPoint? = null
        private var timed = false
        private var inTrack = false

        fun start() {
            val pt = point
            when (p.name) {
                "trk" -> {
                    inTrack = true
                }

                "trkpt" -> {
                    point = TrackPoint(0, p.getAttributeValue(null, "lat").toDouble(), p.getAttributeValue(null, "lon").toDouble(), null)
                    timed = false
                }

                "ele" -> {
                    if (pt != null) point = pt.copy(ele = p.nextText().trim().toDoubleOrNull())
                }

                "time" -> {
                    if (pt != null) {
                        point = pt.copy(timeMs = Instant.parse(p.nextText().trim()).toEpochMilli())
                        timed = true
                    }
                }

                "name" -> {
                    if (inTrack && pt == null && name == null) name = p.nextText().trim()
                }
            }
        }

        fun end() {
            if (p.name != "trkpt") return
            // A point without time cannot be placed in the session: skipped.
            point?.takeIf { timed }?.let(points::add)
            point = null
        }
    }
}
