package fr.skimap.app.fog

import android.content.Intent
import fr.skimap.app.guidance.RustTrackingCore
import fr.skimap.app.prep.ReturnAlert
import fr.skimap.app.recap.BaroRecorder
import fr.skimap.app.recap.RecapNotifier
import fr.skimap.core.GuidedSession
import fr.skimap.core.TrackActivity
import fr.skimap.core.TrackEvent
import fr.skimap.core.openGuided
import fr.skimap.tracking.Activity
import fr.skimap.tracking.Batch
import fr.skimap.tracking.Guidance
import fr.skimap.tracking.Recap
import fr.skimap.tracking.StepReport
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingCore
import fr.skimap.tracking.TrackingService
import fr.skimap.tracking.Utterance

/**
 * A guided outing (K7 bearing in the fog, V1 off-piste route) behind the location service's
 * interface: the same service, voice, vibrations, buttons and notification as the day plan
 * (module `:tracking`); only the core differs ([GuidedSession], Rust).
 */
@Suppress("TooManyFunctions")
class GuidedCore(
    private val session: GuidedSession,
) : TrackingCore {
    override fun pushFixes(packed: ByteArray): Batch = RustTrackingCore.map(session.pushFixes(packed))

    /** The steps are recorded with the fixes (X10); the bearing does not use them. */
    override fun pushBatch(
        packed: ByteArray,
        steps: List<StepReport>,
    ): Batch = RustTrackingCore.map(session.pushBatch(packed, steps.map { fr.skimap.core.StepReport(it.tMs, it.steps.toUInt()) }))

    /** A guided outing proposes no profile (X14): the answer is only recorded. */
    override fun profileAnswer(
        tMs: Long,
        accept: Boolean,
    ): Batch = RustTrackingCore.map(session.pushEvent(TrackEvent.ProfileAnswer(tMs, accept)))

    override fun screen(
        tMs: Long,
        on: Boolean,
    ): Batch = RustTrackingCore.map(session.pushEvent(TrackEvent.Screen(tMs, on)))

    override fun activity(
        tMs: Long,
        activity: Activity,
    ): Batch =
        RustTrackingCore.map(
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
    ): Batch = RustTrackingCore.map(session.pushEvent(TrackEvent.Closed(tMs, ways.toList())))

    override fun answer(
        tMs: Long,
        accept: Boolean,
    ): Batch = RustTrackingCore.map(session.pushEvent(TrackEvent.Answer(tMs, accept)))

    override fun refuse(tMs: Long): Batch = RustTrackingCore.map(session.pushEvent(TrackEvent.Refuse(tMs)))

    override fun backStart(tMs: Long): Batch = RustTrackingCore.map(session.backStart(tMs))

    override fun backStop(): Batch = RustTrackingCore.map(session.backStop())

    override fun sayNext(tMs: Long): Utterance? = session.sayNext(tMs)?.let(RustTrackingCore::utterance)

    override fun sayRemaining(tMs: Long): Utterance? = session.sayRemaining(tMs)?.let(RustTrackingCore::utterance)

    override fun guidance(): Guidance = RustTrackingCore.guidance(session.guidance())

    override fun observation(): String = session.observation()

    override fun writeGpx(): String = session.writeGpx()

    override fun close() = session.close()

    companion object {
        fun open(dir: String): GuidedCore = GuidedCore(openGuided(dir))
    }
}

/** The location service of a guided outing; at the end, the recap (A10). */
class GuidedTrackingService : TrackingService() {
    private var baro: BaroRecorder? = null

    override fun openCore(planDir: String): TrackingCore = GuidedCore.open(planDir)

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        if (intent?.action == Tracking.ACTION_START && baro == null) {
            intent.getStringExtra(Tracking.EXTRA_PLAN_DIR)?.let { baro = BaroRecorder.start(this, it) }
        }
        return super.onStartCommand(intent, flags, startId)
    }

    override fun onEnded(
        planDir: String,
        recap: Recap?,
        gpx: String?,
    ) {
        baro?.stop()
        baro = null
        RecapNotifier.ended(this, planDir)
        ReturnAlert.checkIn(this, planDir)
    }

    override fun onDestroy() {
        baro?.stop()
        baro = null
        super.onDestroy()
    }
}
