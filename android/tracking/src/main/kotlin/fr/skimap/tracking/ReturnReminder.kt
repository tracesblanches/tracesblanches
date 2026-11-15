package fr.skimap.tracking

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.edit
import androidx.core.net.toUri

/**
 * The local reminder before the return time (K10): one inexact alarm (no exact-alarm
 * permission, no timer running), a notification « Retour prévu à 15 h 20 » with the SMS
 * « bien rentré » to the contact, pre-filled: the user sends it (the app never sends nor
 * calls by itself). Nothing is shown when the outing already ended.
 */
object ReturnReminder {
    private const val PREFS = "skimap_reminder"
    private const val NOTIFICATION_ID = 4208

    fun schedule(
        context: Context,
        atMs: Long,
        title: String,
        text: String,
        smsNumber: String?,
        smsBody: String?,
    ) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit {
            putString("title", title)
            putString("text", text)
            putString("number", smsNumber)
            putString("body", smsBody)
            putLong("at", atMs)
        }
        val am = context.getSystemService(AlarmManager::class.java) ?: return
        am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, atMs, pending(context))
    }

    fun cancel(context: Context) {
        context.getSystemService(AlarmManager::class.java)?.cancel(pending(context))
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit { clear() }
    }

    /** When the reminder is due, epoch ms (`null`: none). */
    fun dueAt(context: Context): Long? = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getLong("at", -1L).takeIf { it > 0 }

    private fun pending(context: Context): PendingIntent =
        PendingIntent.getBroadcast(
            context,
            0,
            Intent(context, ReturnReminderReceiver::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    /** The SMS app opened on [number] with [body]; the user presses send. */
    fun smsIntent(
        number: String,
        body: String,
    ): Intent = Intent(Intent.ACTION_SENDTO, "smsto:${Uri.encode(number)}".toUri()).putExtra("sms_body", body)

    @android.annotation.SuppressLint("MissingPermission")
    internal fun show(context: Context) {
        val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val title = p.getString("title", null) ?: return
        val b =
            NotificationCompat
                .Builder(context, GuidanceNotifier.CHANNEL_REMINDER)
                .setSmallIcon(R.drawable.ic_tracking)
                .setContentTitle(title)
                .setContentText(p.getString("text", ""))
                .setStyle(NotificationCompat.BigTextStyle().bigText(p.getString("text", "")))
                .setCategory(NotificationCompat.CATEGORY_REMINDER)
                .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
                .setAutoCancel(true)
        val number = p.getString("number", null)
        val body = p.getString("body", null)
        if (number != null && body != null) {
            val sms = PendingIntent.getActivity(context, 1, smsIntent(number, body), PendingIntent.FLAG_IMMUTABLE)
            b.addAction(0, context.getString(R.string.reminder_sms), sms)
        }
        val nm = NotificationManagerCompat.from(context)
        if (nm.areNotificationsEnabled()) nm.notify(NOTIFICATION_ID, b.build())
    }
}

/** The alarm of [ReturnReminder]. */
class ReturnReminderReceiver : BroadcastReceiver() {
    override fun onReceive(
        context: Context,
        intent: Intent,
    ) {
        // The outing ended and the user was told already: nothing to remind.
        if (Tracking.activePlan(context) == null && Tracking.state.value !is TrackingState.Running) return
        ReturnReminder.show(context)
    }
}
