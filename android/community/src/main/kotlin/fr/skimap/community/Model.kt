package fr.skimap.community

import java.io.File

/** Colours of the kinds on the map and the buttons (white text reads on each). */
private object KindColors {
    const val TRACK = 0xFF1565C0
    const val GROOMED = 0xFF2E7D32
    const val ICE = 0xFF00838F
    const val WIND_BLOWN = 0xFF6A1B9A
    const val OBSTACLE = 0xFFE65100
    const val CLOSED = 0xFFC62828
}

/** Kinds of report (crates/collab/docs/regles.md): no free text. */
enum class ReportKind(
    val id: String,
    val label: Int,
    /** Colour on the map, ARGB. */
    val color: Long,
) {
    TRACK("track", R.string.kind_track, KindColors.TRACK),
    GROOMED("groomed", R.string.kind_groomed, KindColors.GROOMED),
    ICE("ice", R.string.kind_ice, KindColors.ICE),
    WIND_BLOWN("wind_blown", R.string.kind_wind_blown, KindColors.WIND_BLOWN),
    OBSTACLE("obstacle", R.string.kind_obstacle, KindColors.OBSTACLE),
    CLOSED("closed", R.string.kind_closed, KindColors.CLOSED),
    ;

    companion object {
        fun of(id: String): ReportKind? = entries.firstOrNull { it.id == id }
    }
}

/** A report made on the phone, waiting in the queue ([ReportOutbox]). */
data class QueuedReport(
    val localId: String,
    val kind: ReportKind,
    val lon: Double,
    val lat: Double,
    /** When it was seen: the gesture, not the sending (V11). */
    val seenAtMs: Long,
    /** Reduced JPEG beside the queue entry, if any. */
    val photo: File? = null,
)

/** A report as the server shows it. */
data class ServerReport(
    val id: String,
    val kind: ReportKind,
    val lon: Double,
    val lat: Double,
    val seenAtMs: Long,
    val expiresAtMs: Long,
    val confirmations: Int,
    val denials: Int,
    val photo: Boolean,
)

/** An outing of « Mes sorties ». */
data class Outing(
    val id: String,
    val source: String,
    val name: String,
    val startMs: Long,
    val km: Double,
    val movingMin: Double,
    val gainM: Double,
)

/** Kind of challenge. */
enum class ChallengeKind { ROUTE, DOMAIN, DISTANCE }

/** A challenge and its progress, computed on the phone. */
data class Challenge(
    val id: String,
    val kind: ChallengeKind,
    val title: String,
    val doneKm: Double,
    val totalKm: Double,
    val share: Double,
    val outings: Int,
    val doneOn: String?,
    val attribution: String,
)

/** What an import of the Strava export did. */
data class ImportSummary(
    val imported: Int,
    val already: Int,
    val withoutTrack: Int,
    val otherSports: Int,
    val observations: Int,
    /** Groups' outings learned by the calibration. */
    val learned: Int,
    val errors: List<String>,
)

/** A note for OpenStreetMap, as it will be published. */
data class OsmNote(
    val lat: Double,
    val lon: Double,
    val text: String,
    val formBody: String,
)

/** Corrections of the OSM map (crates/collab/src/osm_note.rs). */
enum class Correction(
    val id: String,
    val label: Int,
) {
    MISSING_PISTE("missing_piste", R.string.osm_missing_piste),
    PISTE_GONE("piste_gone", R.string.osm_piste_gone),
    WRONG_NAME("wrong_name", R.string.osm_wrong_name),
    WRONG_DIFFICULTY("wrong_difficulty", R.string.osm_wrong_difficulty),
    WRONG_GROOMING("wrong_grooming", R.string.osm_wrong_grooming),
    WRONG_ONEWAY("wrong_oneway", R.string.osm_wrong_oneway),
    OTHER("other", R.string.osm_other),
}

/** The area shown when the screen opened: its pack, for the map and the challenges. */
data class AreaContext(
    val siteId: String,
    val name: String,
    /** `[minLon, minLat, maxLon, maxLat]`. */
    val bbox: DoubleArray,
    /** MapLibre style instantiated on the pack (file:// URLs), or null. */
    val styleJson: String?,
    /** The pack's `layers/pistes.geojson`, or null. */
    val pistesPath: String?,
) {
    override fun equals(other: Any?): Boolean =
        other is AreaContext && other.siteId == siteId && other.bbox.contentEquals(bbox) && other.styleJson == styleJson

    override fun hashCode(): Int = siteId.hashCode()
}

/**
 * What the module needs from the Rust core and the app (implemented by the app over the
 * UniFFI bindings, faked in the JVM tests).
 */
interface CommunityCore {
    /** Imports the Strava archive into [outRoot], then the group learns the outings. */
    fun importStrava(
        zip: File,
        outRoot: File,
    ): ImportSummary

    /** Outings under [roots], newest first. */
    fun outings(roots: List<File>): List<Outing>

    /** Challenges from the outings under [roots]. */
    fun challenges(
        roots: List<File>,
        routesJson: String,
        domains: List<AreaContext>,
        nowMs: Long,
    ): List<Challenge>

    /** The OSM note of a correction. */
    fun osmNote(
        correction: Correction,
        pisteName: String?,
        osmWayId: Long?,
        comment: String,
        lon: Double,
        lat: Double,
        french: Boolean,
    ): OsmNote

    /** The area of [siteId] (or the first pack), with its style, if a pack is installed. */
    fun area(siteId: String?): AreaContext?

    /** Roots of the outings: the app's plans and the imported ones. */
    fun outingRoots(): List<File>
}
