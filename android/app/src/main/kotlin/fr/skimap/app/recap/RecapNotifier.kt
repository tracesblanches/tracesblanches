package fr.skimap.app.recap

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.Context
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import fr.skimap.app.R
import fr.skimap.app.fog.Outings
import fr.skimap.core.outingRecap
import fr.skimap.tracking.GuidanceNotifier

/** The recap of an outing (A10) at its end: a notification that opens it, written and read aloud. */
object RecapNotifier {
    private const val ID = 4210

    @SuppressLint("MissingPermission")
    fun ended(
        context: Context,
        dir: String,
    ) {
        val r =
            runCatching { outingRecap(dir, Outings.plansRoot(context).path, Outings.workDir(context).path) }
                .onFailure { Log.w("skimap", "recap", it) }
                .getOrNull() ?: return
        val open =
            PendingIntent.getActivity(
                context,
                ID,
                RecapActivity.intent(context, dir, speak = true),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        val n =
            NotificationCompat
                .Builder(context, GuidanceNotifier.CHANNEL_REMINDER)
                .setSmallIcon(fr.skimap.tracking.R.drawable.ic_tracking)
                .setContentTitle(context.getString(R.string.recap_notification))
                .setContentText(r.spoken)
                .setStyle(NotificationCompat.BigTextStyle().bigText(r.text))
                .setContentIntent(open)
                .setAutoCancel(true)
                .build()
        val nm = NotificationManagerCompat.from(context)
        if (nm.areNotificationsEnabled()) nm.notify(ID, n)
    }
}
