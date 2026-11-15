package fr.skimap.group

// What the group mode exchanges with the core (Rust, crates/ffi/src/group.rs). The app
// adapts the UniFFI types to these (GroupBackend); this module knows nothing of the
// bindings, so its logic is tested on the JVM with a fake core.

/** A group, as the screens show it. [qrText] is the group itself (kept by [GroupStore]). */
data class GroupInfo(
    val qrText: String,
    val shortId: String,
    val members: List<String>,
    val hasPlan: Boolean,
)

/** The QR code: [width] modules per side, row by row (`true`: dark). */
class QrMatrix(
    val width: Int,
    val modules: BooleanArray,
)

/** Phase of the outing, from the tracker. */
enum class OwnPhase { MOVING, PAUSED, REFUGE }

/** This phone's position after a GPS batch. */
data class OwnFix(
    val tMs: Long,
    val lat: Double,
    val lon: Double,
    val phase: OwnPhase,
    val offRoute: Boolean,
    val progressM: Double?,
    val batteryLow: Boolean,
)

/** A member: last known position (null: never heard), its age and how it came. */
data class Member(
    val number: Int,
    val name: String,
    val me: Boolean,
    val lat: Double?,
    val lon: Double?,
    val ageS: Long?,
    /** 0: heard from the member's phone; n: relayed n times. */
    val hops: Int?,
    val stopped: Boolean,
    val offRoute: Boolean,
    val batteryLow: Boolean,
    val distanceM: Double?,
)

/** Kinds of group alerts. */
enum class GroupAlertKind { STRAGGLER, LONG_STOP, OFF_ROUTE }

/** An alert about a member: said and vibrated by the location service. */
data class GroupAlert(
    val kind: GroupAlertKind,
    val member: Int,
    val text: String,
)

/** What one call to the session produced. */
class Tick(
    /** This phone's frame (first advertising set). */
    val ownFrame: ByteArray?,
    /** A relayed position, in turn (second advertising set). */
    val relayFrame: ByteArray?,
    /** Prefixes the scan filters on (they change every 15 min). */
    val prefixes: List<ByteArray>,
    val alerts: List<GroupAlert>,
    val members: List<Member>,
    val newPositions: Int,
)

/** One phone's group session during an outing (calls from one thread). */
interface GroupSessionCore : AutoCloseable {
    fun tick(
        nowMs: Long,
        fix: OwnFix?,
        frames: List<ByteArray>,
    ): Tick
}

/** The core's group functions. */
interface GroupBackend {
    /** 16-bit UUID of the frames' Service Data. */
    val serviceUuid: Int

    fun create(
        key: ByteArray,
        members: List<String>,
        planDir: String?,
        createdS: Long,
    ): GroupInfo

    fun read(qrText: String): GroupInfo

    fun qr(qrText: String): QrMatrix

    /** The session of member [me], following the plan in [planDir]. */
    fun open(
        qrText: String,
        me: Int,
        planDir: String?,
    ): GroupSessionCore
}
