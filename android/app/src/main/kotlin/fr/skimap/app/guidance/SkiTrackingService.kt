package fr.skimap.app.guidance

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import fr.skimap.app.R
import fr.skimap.app.day.DayStore
import fr.skimap.tracking.GuidanceNotifier
import fr.skimap.tracking.Recap
import fr.skimap.tracking.ReturnReminder
import fr.skimap.tracking.TrackingCore
import fr.skimap.tracking.TrackingService

/**
 * The app's location service: the core's tracker of the plan (Rust), and at the end the
 * group learns from the outing (C11) and the user is offered the SMS « bien rentré » to
 * the contact (K10, pre-filled, sent by the user).
 */
class SkiTrackingService : TrackingService() {
    override fun openCore(planDir: String): TrackingCore = RustTrackingCore.open(planDir)

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
