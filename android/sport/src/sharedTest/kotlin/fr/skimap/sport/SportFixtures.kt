// Test code (the detekt configuration exempts src/test and src/androidTest from MagicNumber).
@file:Suppress("MagicNumber")

package fr.skimap.sport

import java.io.File
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.Locale

/** A trace as the core writes it (crates/ffi/src/tracking.rs, `gpx`) and a plan with its morning. */
object SportFixtures {
    /** 2026-01-15 at 09:30 UTC. */
    const val START_MS = 1_768_469_400_000L
    const val STEP_MS = 5_000L

    private val TIME = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'").withZone(ZoneOffset.UTC)

    fun gpx(
        points: Int = 120,
        name: String = "Boucle du Chaumoz",
    ): String =
        buildString {
            append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
            append("<gpx version=\"1.1\" creator=\"skimap\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n<trk><name>")
            append(name.replace("&", "&amp;"))
            append("</name><type>nordic_skiing</type><trkseg>\n")
            for (i in 0 until points) {
                val t = TIME.format(Instant.ofEpochMilli(START_MS + i * STEP_MS))
                append(
                    "<trkpt lat=\"%.7f\" lon=\"%.7f\"><time>%s</time></trkpt>\n"
                        .format(Locale.ROOT, 44.8650 + i * 0.0001, 6.7600 + i * 0.00005, t),
                )
            }
            append("</trkseg></trk>\n</gpx>\n")
        }

    const val PLAN =
        """{"format":"skimap-day-plan","version":1,"site":"cervieres","morning":[
        ["briefing",{"text":["Neige fraîche de 10 cm, damage ce matin.","Vent faible, −8 °C au départ."],"voice":[]}],
        ["best_time",{"text":["Partez vers 10 h."],"voice":[]}],
        ["wax",{"text":["Fart violet."],"voice":[]}],
        ["synthesis",{"text":["Itinéraire sans difficulté."],"voice":[]}]]}"""

    /** A plan directory with `plan.json` and `trace.gpx`; the trace. */
    fun outingDir(
        root: File,
        name: String = "2026-01-15-chaumoz",
        points: Int = 120,
    ): File {
        val d = File(root, name).also { it.mkdirs() }
        File(d, "plan.json").writeText(PLAN)
        return File(d, "trace.gpx").also { it.writeText(gpx(points)) }
    }

    fun outing(
        root: File,
        sport: SportType = SportType.NORDIC_SKI,
        name: String = "2026-01-15-chaumoz",
    ): Outing {
        val gpx = outingDir(root, name)
        return Outing.of(gpx.parent, gpx.path, "Sortie terminée. 6,2 km en 1 h 10.", 6.2, sport)
    }
}
