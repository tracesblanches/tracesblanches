package fr.skimap.app.group

import android.content.Context
import fr.skimap.core.GroupFix
import fr.skimap.core.GroupPhase
import fr.skimap.core.GroupSession
import fr.skimap.core.Lang
import fr.skimap.core.groupModeCreate
import fr.skimap.core.groupModeQr
import fr.skimap.core.groupModeRadio
import fr.skimap.core.groupModeRead
import fr.skimap.core.openGroupSession
import fr.skimap.group.GroupAlert
import fr.skimap.group.GroupAlertKind
import fr.skimap.group.GroupBackend
import fr.skimap.group.GroupInfo
import fr.skimap.group.GroupSessionCore
import fr.skimap.group.Member
import fr.skimap.group.OwnFix
import fr.skimap.group.OwnPhase
import fr.skimap.group.QrMatrix
import fr.skimap.group.Tick
import java.io.File
import java.util.TimeZone
import fr.skimap.core.GroupAlertKind as CoreAlertKind
import fr.skimap.core.GroupInfo as CoreInfo

/** The core's group mode (Rust, crates/ffi/src/group.rs) behind the group module's interface. */
class RustGroupBackend(
    private val lang: () -> Lang = {
        if (java.util.Locale
                .getDefault()
                .language == "fr"
        ) {
            Lang.FR
        } else {
            Lang.EN
        }
    },
) : GroupBackend {
    override val serviceUuid: Int by lazy { groupModeRadio().serviceUuid.toInt() }

    override fun create(
        key: ByteArray,
        members: List<String>,
        planDir: String?,
        createdS: Long,
    ): GroupInfo = info(groupModeCreate(key, members, planDir, createdS.toUInt()))

    override fun read(qrText: String): GroupInfo = info(groupModeRead(qrText))

    override fun qr(
        qrText: String,
        member: Int?,
    ): QrMatrix {
        val m = groupModeQr(qrText, member?.toUByte())
        return QrMatrix(m.width.toInt(), m.modules.toBooleanArray())
    }

    override fun open(
        qrText: String,
        me: Int,
        planDir: String?,
    ): GroupSessionCore {
        val offset = TimeZone.getDefault().getOffset(System.currentTimeMillis()) / MS
        return RustGroupSession(openGroupSession(qrText, me.toUByte(), planDir, lang(), offset.toLong()))
    }

    private fun info(i: CoreInfo) = GroupInfo(i.qrText, i.shortId, i.members, i.hasPlan, i.member?.toInt())

    companion object {
        private const val MS = 1000

        /**
         * The plan a group created now shares: the outing's, else the newest plan of the
         * day validated on this phone (`files/plans/<date>-<n>/plan.json`, today's only).
         */
        fun planOfTheDay(
            context: Context,
            active: String?,
        ): String? {
            if (active != null && File(active, "plan.json").isFile) return active
            val today =
                java.time.LocalDate
                    .now()
                    .toString()
            return File(context.filesDir, "plans")
                .listFiles()
                .orEmpty()
                .filter { it.name.startsWith(today) && File(it, "plan.json").isFile }
                .maxByOrNull { File(it, "plan.json").lastModified() }
                ?.absolutePath
        }
    }
}

/** One phone's group session (Rust). */
class RustGroupSession(
    private val session: GroupSession,
) : GroupSessionCore {
    override fun tick(
        nowMs: Long,
        fix: OwnFix?,
        frames: List<ByteArray>,
    ): Tick {
        val t =
            session.tick(
                nowMs,
                fix?.let {
                    GroupFix(
                        it.tMs,
                        it.lat,
                        it.lon,
                        when (it.phase) {
                            OwnPhase.MOVING -> GroupPhase.MOVING
                            OwnPhase.PAUSED -> GroupPhase.PAUSED
                            OwnPhase.REFUGE -> GroupPhase.REFUGE
                        },
                        it.offRoute,
                        it.progressM,
                        it.batteryLow,
                    )
                },
                frames,
            )
        return Tick(
            ownFrame = t.ownFrame,
            relayFrame = t.relayFrame,
            prefixes = t.scanPrefixes,
            alerts =
                t.alerts.map {
                    GroupAlert(
                        when (it.kind) {
                            CoreAlertKind.STRAGGLER -> GroupAlertKind.STRAGGLER
                            CoreAlertKind.LONG_STOP -> GroupAlertKind.LONG_STOP
                            CoreAlertKind.OFF_ROUTE -> GroupAlertKind.OFF_ROUTE
                        },
                        it.member.toInt(),
                        it.text,
                    )
                },
            members =
                t.members.map {
                    Member(
                        number = it.member.toInt(),
                        name = it.name,
                        me = it.me,
                        lat = it.lat,
                        lon = it.lon,
                        ageS = it.ageS,
                        hops = it.hops?.toInt(),
                        stopped = it.stopped,
                        offRoute = it.offRoute,
                        batteryLow = it.batteryLow,
                        distanceM = it.distanceM,
                    )
                },
            newPositions = t.newPositions.toInt(),
        )
    }

    override fun close() = session.close()
}
