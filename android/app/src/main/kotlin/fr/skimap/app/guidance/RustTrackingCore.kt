package fr.skimap.app.guidance

import fr.skimap.core.AlertView
import fr.skimap.core.BatchOut
import fr.skimap.core.GpsModeView
import fr.skimap.core.GpsRequestView
import fr.skimap.core.GuidanceView
import fr.skimap.core.PhaseView
import fr.skimap.core.SpeechPriority
import fr.skimap.core.SpokenView
import fr.skimap.core.TrackActivity
import fr.skimap.core.TrackEvent
import fr.skimap.core.TrackingSession
import fr.skimap.core.TurnDirection
import fr.skimap.core.openTracking
import fr.skimap.tracking.Activity
import fr.skimap.tracking.AlertKind
import fr.skimap.tracking.Batch
import fr.skimap.tracking.Clip
import fr.skimap.tracking.GpsMode
import fr.skimap.tracking.GpsRequest
import fr.skimap.tracking.Guidance
import fr.skimap.tracking.Phase
import fr.skimap.tracking.ProfileProposal
import fr.skimap.tracking.Proposal
import fr.skimap.tracking.Recap
import fr.skimap.tracking.RouteSwitch
import fr.skimap.tracking.StepReport
import fr.skimap.tracking.TrackingCore
import fr.skimap.tracking.Turn
import fr.skimap.tracking.Utterance

/** The core's tracking session (Rust, UniFFI) behind the location service's interface, call for call. */
@Suppress("TooManyFunctions")
class RustTrackingCore(
    // Read by the voice commands (« Où suis-je ? » from the core's guidance).
    internal val session: TrackingSession,
) : TrackingCore {
    override fun pushFixes(packed: ByteArray): Batch = map(session.pushFixes(packed))

    override fun pushBatch(
        packed: ByteArray,
        steps: List<StepReport>,
    ): Batch = map(session.pushBatch(packed, steps.map { fr.skimap.core.StepReport(it.tMs, it.steps.toUInt()) }))

    override fun profileAnswer(
        tMs: Long,
        accept: Boolean,
    ): Batch = map(session.pushEvent(TrackEvent.ProfileAnswer(tMs, accept)))

    override fun screen(
        tMs: Long,
        on: Boolean,
    ): Batch = map(session.pushEvent(TrackEvent.Screen(tMs, on)))

    override fun activity(
        tMs: Long,
        activity: Activity,
    ): Batch =
        map(
            session.pushEvent(
                TrackEvent.Activity(
                    tMs,
                    when (activity) {
                        Activity.STILL -> TrackActivity.STILL
                        Activity.ON_FOOT -> TrackActivity.ON_FOOT
                        Activity.IN_VEHICLE -> TrackActivity.IN_VEHICLE
                        Activity.UNKNOWN -> TrackActivity.UNKNOWN
                        Activity.SIGNIFICANT_MOTION -> TrackActivity.SIGNIFICANT_MOTION
                    },
                ),
            ),
        )

    override fun closed(
        tMs: Long,
        ways: LongArray,
    ): Batch = map(session.pushEvent(TrackEvent.Closed(tMs, ways.toList())))

    override fun answer(
        tMs: Long,
        accept: Boolean,
    ): Batch = map(session.pushEvent(TrackEvent.Answer(tMs, accept)))

    override fun refuse(tMs: Long): Batch = map(session.pushEvent(TrackEvent.Refuse(tMs)))

    override fun backStart(tMs: Long): Batch = map(session.backStart(tMs))

    override fun backStop(): Batch = map(session.backStop())

    override fun sayNext(tMs: Long): Utterance? = session.sayNext(tMs)?.let(::utterance)

    override fun sayRemaining(tMs: Long): Utterance? = session.sayRemaining(tMs)?.let(::utterance)

    override fun guidance(): Guidance = guidance(session.guidance())

    override fun observation(): String = session.observation()

    override fun writeGpx(): String = session.writeGpx()

    override fun close() = session.close()

    companion object {
        /** Opens the tracking of the plan in [planDir] (replays the inputs already recorded there). */
        fun open(planDir: String): RustTrackingCore = RustTrackingCore(openTracking(planDir))

        fun map(b: BatchOut): Batch =
            Batch(
                utterances = b.spoken.map(::utterance),
                alerts = b.alerts.map(::alert),
                switches = b.switches.map { RouteSwitch(it.route, it.cause, it.reverted, it.refusable) },
                proposals = b.proposals.map { Proposal(it.id.toInt(), it.kind, it.text) },
                session = b.session.map { it.what },
                gps = b.gps?.let(::gps),
                recap = b.recap?.let { Recap(it.arrived, it.distanceKm, it.movingMin, it.text) },
                guidance = guidance(b.guidance),
                profileProposals = b.profileProposals.map { ProfileProposal(it.name, it.profileJson, it.text) },
                profileChosen = b.profileChosen,
            )

        fun utterance(s: SpokenView): Utterance =
            Utterance(
                text = s.text,
                clips = s.segments.map { Clip(it.text, it.file, it.pauseMs.toInt()) },
                allClips = s.allClips,
                safety = s.priority == SpeechPriority.SAFETY,
                kinds = s.kinds,
            )

        fun gps(g: GpsRequestView): GpsRequest =
            GpsRequest(
                mode =
                    when (g.mode) {
                        GpsModeView.IDLE -> GpsMode.IDLE
                        GpsModeView.POCKET -> GpsMode.POCKET
                        GpsModeView.APPROACH -> GpsMode.APPROACH
                        GpsModeView.DEVIATION -> GpsMode.DEVIATION
                        GpsModeView.INTERACTION -> GpsMode.INTERACTION
                        GpsModeView.PAUSE -> GpsMode.PAUSE
                        GpsModeView.REFUGE -> GpsMode.REFUGE
                        GpsModeView.WAKING -> GpsMode.WAKING
                    },
                gnss = g.gnss,
                intervalMs = g.intervalMs,
                maxDelayMs = g.maxDelayMs,
                significantMotion = g.significantMotion,
            )

        private fun alert(a: AlertView): AlertKind =
            when (a.kind) {
                fr.skimap.core.AlertKind.OFF_ROUTE -> AlertKind.OFF_ROUTE
                fr.skimap.core.AlertKind.BACK_ON_ROUTE -> AlertKind.BACK_ON_ROUTE
                fr.skimap.core.AlertKind.WIND_RISING -> AlertKind.WIND_RISING
                fr.skimap.core.AlertKind.FOG -> AlertKind.FOG
                fr.skimap.core.AlertKind.SNOW -> AlertKind.SNOW
                fr.skimap.core.AlertKind.TEMPERATURE_DROP -> AlertKind.TEMPERATURE_DROP
                fr.skimap.core.AlertKind.TURNAROUND_ADVANCED -> AlertKind.TURNAROUND_ADVANCED
                fr.skimap.core.AlertKind.TURNAROUND_NOW -> AlertKind.TURNAROUND_NOW
                fr.skimap.core.AlertKind.CLOSED -> AlertKind.CLOSED
                fr.skimap.core.AlertKind.CLOSED_NO_DETOUR -> AlertKind.CLOSED_NO_DETOUR
            }

        fun turn(d: TurnDirection): Turn =
            when (d) {
                TurnDirection.LEFT -> Turn.LEFT
                TurnDirection.RIGHT -> Turn.RIGHT
                TurnDirection.SLIGHT_LEFT -> Turn.SLIGHT_LEFT
                TurnDirection.SLIGHT_RIGHT -> Turn.SLIGHT_RIGHT
                TurnDirection.KEEP_LEFT -> Turn.KEEP_LEFT
                TurnDirection.KEEP_RIGHT -> Turn.KEEP_RIGHT
                TurnDirection.STRAIGHT -> Turn.STRAIGHT
                TurnDirection.U_TURN -> Turn.U_TURN
            }

        fun guidance(g: GuidanceView): Guidance =
            Guidance(
                phase =
                    when (g.phase) {
                        PhaseView.WAITING -> Phase.WAITING
                        PhaseView.MOVING -> Phase.MOVING
                        PhaseView.PAUSED -> Phase.PAUSED
                        PhaseView.REFUGE -> Phase.REFUGE
                        PhaseView.FINISHED -> Phase.FINISHED
                    },
                route = g.route,
                label = g.label,
                remainingM = g.remainingM,
                lengthM = g.lengthM,
                progressM = g.progressM,
                nextTurn = g.next?.direction?.let(::turn),
                nextDistanceM = g.next?.distanceM,
                nextName = g.next?.name,
                remainingS = g.remainingS,
                remainingText = g.remainingText,
                arrival = g.arrival,
                arrivalMs = g.arrivalMs,
                arrivalRange = g.arrivalRange,
                limit = g.limit,
                offRouteM = g.offRouteM,
                gpsMode = gps(GpsRequestView(g.gpsMode, true, 0, 0, false)).mode,
                proposal = g.proposal?.let { Proposal(it.id.toInt(), it.kind, it.text) },
                switch = g.switch?.let { RouteSwitch(it.route, it.cause, it.reverted, it.refusable) },
                profileProposal = g.profileProposal?.let { ProfileProposal(it.name, it.profileJson, it.text) },
            )
    }
}
