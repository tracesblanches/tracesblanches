package fr.skimap.app.guidance

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import fr.skimap.app.R
import fr.skimap.app.day.DayStore
import fr.skimap.app.group.RustGroupBackend
import fr.skimap.group.GroupLink
import fr.skimap.group.GroupStore
import fr.skimap.tracking.GuidanceNotifier
import fr.skimap.tracking.OutingExtension
import fr.skimap.tracking.Recap
import fr.skimap.tracking.ReturnReminder
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingCore
import fr.skimap.tracking.TrackingService
import fr.skimap.tracking.TrackingState

/**
 * The app's location service: the core's tracker of the plan (Rust), and at the end the
 * group learns from the outing (C11) and the user is offered the SMS « bien rentré » to
 * the contact (K10, pre-filled, sent by the user).
 */
class SkiTrackingService : TrackingService() {
    override fun openCore(planDir: String): TrackingCore =
        RustTrackingCore.open(
            planDir,
            fr.skimap.app.sync.PackSync
                .voiceDir(this)
                .absolutePath,
            fr.skimap.app.sync.WorkManagerDownloads
                .voiceLang(),
        )

    /** The group mode (N2): BLE advertising and batched scan during the outing, if this phone has a group. */
    override fun extensions(planDir: String): List<OutingExtension> = listOf(GroupLink(RustGroupBackend(), GroupStore(this), planDir))

    /** The profile chosen during the outing (X14), kept next to the plan for the screens and the learning. */
    override fun onProfileChosen(profileJson: String) {
        val dir = (Tracking.state.value as? TrackingState.Running)?.planDir ?: return
        runCatching { java.io.File(dir, DayStore.PROFILE_CHOSEN_FILE).writeText(profileJson) }
            .onSuccess { Log.i(TAG, "profile chosen during the outing: $profileJson") }
            .onFailure { Log.w(TAG, "profile chosen", it) }
    }

    /** Altitude by the barometer during the outing (K8). */
    private var baro: fr.skimap.app.recap.BaroRecorder? = null

    override fun onStartCommand(
        intent: android.content.Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        if (intent?.action == fr.skimap.tracking.Tracking.ACTION_START && baro == null) {
            intent
                .getStringExtra(
                    fr.skimap.tracking.Tracking.EXTRA_PLAN_DIR,
                )?.let {
                    baro =
                        fr.skimap.app.recap.BaroRecorder
                            .start(this, it)
                }
        }
        return super.onStartCommand(intent, flags, startId)
    }

    override fun onDestroy() {
        baro?.stop()
        baro = null
        super.onDestroy()
    }

    @SuppressLint("MissingPermission")
    override fun onEnded(
        planDir: String,
        recap: Recap?,
        gpx: String?,
    ) {
        val store = DayStore(this)
        // A model or an observation the core cannot read: nothing learned, the outing ends all the same.
        runCatching { store.learnFrom(planDir) }
            .onSuccess { g -> g?.let { Log.i(TAG, "group ${it.spec.name}: ${it.learned}") } }
            .onFailure { Log.w(TAG, "group learning", it) }
        ReturnReminder.cancel(this)
        baro?.stop()
        baro = null
        // The recap (A10) and the return confirmed to the server (S2) when it had the plan.
        fr.skimap.app.recap.RecapNotifier
            .ended(this, planDir)
        fr.skimap.app.prep.ReturnAlert
            .checkIn(this, planDir)
        val text = recap?.text ?: getString(R.string.day_ended_stopped)
        val b =
            NotificationCompat
                .Builder(this, GuidanceNotifier.CHANNEL_REMINDER)
                .setSmallIcon(fr.skimap.tracking.R.drawable.ic_tracking)
                .setContentTitle(getString(R.string.day_ended))
                .setContentText(text)
                .setStyle(NotificationCompat.BigTextStyle().bigText(text))
                .setAutoCancel(true)
        val contact = store.contact()
        if (contact != null && recap?.arrived == true) {
            val sms = ReturnReminder.smsIntent(contact.number, getString(R.string.sms_back_home))
            b.addAction(
                0,
                getString(R.string.day_tell_contact, contact.name.ifBlank { contact.number }),
                PendingIntent.getActivity(this, 2, sms, PendingIntent.FLAG_IMMUTABLE),
            )
        }
        val nm = NotificationManagerCompat.from(this)
        if (nm.areNotificationsEnabled()) nm.notify(ENDED_ID, b.build())
    }

    private companion object {
        const val ENDED_ID = 4209
    }
}
