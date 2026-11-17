package fr.skimap.tracking

// What the location service exchanges with the core (Rust, crates/ffi/src/tracking.rs).
// The app adapts the UniFFI types to these (TrackingCore); this module knows nothing of
// the bindings, so its logic is tested on the JVM with a fake core.

/** GPS mode decided by the core (report 09). */
enum class GpsMode { IDLE, POCKET, APPROACH, DEVIATION, INTERACTION, PAUSE, REFUGE, WAKING }

/** What the location service applies: interval, batching, GNSS off with significant motion. */
data class GpsRequest(
    val mode: GpsMode,
    val gnss: Boolean,
    val intervalMs: Long,
    val maxDelayMs: Long,
    val significantMotion: Boolean,
)

/** One clip of an announcement: Ogg/Opus file of the pack (null: not in the pack). */
data class Clip(
    val text: String,
    val file: String?,
    val pauseMs: Int,
)

/** Something to say: the pack's clips assembled, else the Android voice reads [text]. */
data class Utterance(
    val text: String,
    val clips: List<Clip>,
    val allClips: Boolean,
    val safety: Boolean,
    val kinds: List<String>,
)

/** Alerts with their own vibration (K6). */
enum class AlertKind {
    OFF_ROUTE,
    BACK_ON_ROUTE,
    WIND_RISING,
    FOG,
    SNOW,
    TEMPERATURE_DROP,
    TURNAROUND_ADVANCED,
    TURNAROUND_NOW,
    CLOSED,
    CLOSED_NO_DETOUR,
}

/** Direction of the next turn. */
enum class Turn { LEFT, RIGHT, SLIGHT_LEFT, SLIGHT_RIGHT, KEEP_LEFT, KEEP_RIGHT, STRAIGHT, U_TURN }

/** Activity told to the core: the significant motion sensor, or an activity recognised (replays). */
enum class Activity { STILL, ON_FOOT, IN_VEHICLE, UNKNOWN, SIGNIFICANT_MOTION }

/** Phase of the outing. */
enum class Phase { WAITING, MOVING, PAUSED, REFUGE, FINISHED }

/** A proposal of the core (C11), answered with one gesture. */
data class Proposal(
    val id: Int,
    val kind: String,
    val text: String,
) {
    companion object {
        /** « Ramène-moi » proposed when the return time of the plan calls for going home (K4). */
        const val BACK = "back"
    }
}

/**
 * « Ramène-moi » under way (K4): where it goes, how, and the next useful point of the way,
 * where the arrow of the screen points (never straight across the terrain to the end).
 */
data class BackGuide(
    /** `destination` (of the day plan), `car`, `start` (no car known). */
    val target: String,
    /** Name of the destination or of the car park, when known. */
    val targetName: String?,
    /** `pistes`, `trace_pistes` (back along the way skied to the pistes), `trace`. */
    val way: String,
    val hereLon: Double,
    val hereLat: Double,
    val guideLon: Double,
    val guideLat: Double,
    /** From the last fix to the next useful point, degrees from true north. */
    val guideBearingDeg: Double,
    val guideDistanceM: Double,
    /** Off the way: the arrow shows on its own. */
    val offWay: Boolean,
)

/** The core asks whether to choose another group profile (X14), answered with one gesture. */
data class ProfileProposal(
    val name: String,
    /** The profile, JSON (the core's `GroupProfile`): the group of the day once accepted. */
    val profileJson: String,
    val text: String,
)

/** Steps counted by the phone over one report (X10): end of the report, steps since the previous one. */
data class StepReport(
    val tMs: Long,
    val steps: Int,
)

/** A change of route the skier may refuse with one gesture. */
data class RouteSwitch(
    val route: String,
    val cause: String,
    val reverted: Boolean,
    val refusable: Boolean,
)

/** Thin banner and lock-screen notification: next direction, distance, time left, arrival. */
data class Guidance(
    val phase: Phase,
    /** `plan`, `plan_b`, `live`, `back` (« ramène-moi »). */
    val route: String,
    val label: String,
    val remainingM: Double,
    val lengthM: Double,
    val progressM: Double,
    val nextTurn: Turn?,
    val nextDistanceM: Double?,
    val nextName: String?,
    val remainingS: Double,
    /** « 1 h 05 ». */
    val remainingText: String,
    /** « 15 h 20 ». */
    val arrival: String,
    val arrivalMs: Long,
    /** « 15 h 05 à 15 h 40 ». */
    val arrivalRange: String?,
    val limit: String?,
    val offRouteM: Double,
    val gpsMode: GpsMode,
    val proposal: Proposal?,
    val switch: RouteSwitch?,
    /** Profile proposal waiting for an answer (X14). */
    val profileProposal: ProfileProposal? = null,
    /** « Ramène-moi » under way (K4). */
    val back: BackGuide? = null,
)

/** End of the outing. */
data class Recap(
    val arrived: Boolean,
    val distanceKm: Double,
    val movingMin: Double,
    val text: String,
)

/** What one call to the core produced. */
data class Batch(
    val utterances: List<Utterance>,
    val alerts: List<AlertKind>,
    val switches: List<RouteSwitch>,
    val proposals: List<Proposal>,
    /** Decisions on the outing: `started`, `paused`, `arrived`… */
    val session: List<String>,
    val gps: GpsRequest?,
    val recap: Recap?,
    val guidance: Guidance,
    /** Profile proposals (X14). */
    val profileProposals: List<ProfileProposal> = emptyList(),
    /** The profile chosen by an answer (X14), JSON. */
    val profileChosen: String? = null,
)

/**
 * The core's tracker of one outing (one per service start), call for call the API of
 * crates/ffi/src/tracking.rs. Calls come from one thread.
 */
@Suppress("TooManyFunctions")
interface TrackingCore : AutoCloseable {
    /** A batch of fixes packed by [FixPacker]. */
    fun pushFixes(packed: ByteArray): Batch

    /** A batch of fixes and the step reports over the same time (X10), in one call. */
    fun pushBatch(
        packed: ByteArray,
        steps: List<StepReport>,
    ): Batch

    /** Answers the profile proposal (X14). */
    fun profileAnswer(
        tMs: Long,
        accept: Boolean,
    ): Batch

    fun screen(
        tMs: Long,
        on: Boolean,
    ): Batch

    fun activity(
        tMs: Long,
        activity: Activity,
    ): Batch

    /** Trails closed (OSM ways), from a status update or a report. */
    fun closed(
        tMs: Long,
        ways: LongArray,
    ): Batch

    fun answer(
        tMs: Long,
        accept: Boolean,
    ): Batch

    fun refuse(tMs: Long): Batch

    fun backStart(tMs: Long): Batch

    fun backStop(): Batch

    /** « La voiture est ici » (K4): the car is there; `null` when the outing has no car (a guided outing). */
    fun setCar(
        tMs: Long,
        lon: Double,
        lat: Double,
    ): Batch? = null

    fun sayNext(tMs: Long): Utterance?

    fun sayRemaining(tMs: Long): Utterance?

    fun guidance(): Guidance

    /** What the outing tells about the group (C11), JSON. */
    fun observation(): String

    /** Writes the trace as GPX; its path. */
    fun writeGpx(): String

    /**
     * Language of the announcements (`fr`, `en`, VO6): that of the voice pack in use, else
     * the phone's. The Android voice reads the texts the pack has no clip for in it.
     */
    fun speechLang(): String = Announcer.phoneLang()
}
