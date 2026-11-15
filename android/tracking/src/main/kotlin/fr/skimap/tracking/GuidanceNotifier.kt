package fr.skimap.tracking

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

/**
 * The ongoing notification of the location service, on the lock screen (K13): next
 * direction and distance, time left, arrival. Promoted to a Live Update where the platform
 * has them (Android 16: chip in the status bar, progress along the route); a plain ongoing
 * notification elsewhere. Posted again only when its text changes. Actions: « Ramène-moi »,
 * and the answer to a proposal or the refusal of a switch, one gesture each.
 */
class GuidanceNotifier(
    private val context: Context,
    private val service: Class<out TrackingService>,
) {
    private val nm = NotificationManagerCompat.from(context)
    private val labels = GuidanceLabels.of(context)
    private var shown: List<Any?>? = null

    /** Notifications posted (tests, logs). */
    var posted = 0
        private set

    init {
        val ch =
            NotificationChannel(CHANNEL, context.getString(R.string.tracking_channel), NotificationManager.IMPORTANCE_LOW).apply {
                description = context.getString(R.string.tracking_channel_description)
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
                setShowBadge(false)
            }
        context.getSystemService(NotificationManager::class.java)?.createNotificationChannel(ch)
        val alerts =
            NotificationChannel(CHANNEL_REMINDER, context.getString(R.string.reminder_channel), NotificationManager.IMPORTANCE_HIGH).apply {
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            }
        context.getSystemService(NotificationManager::class.java)?.createNotificationChannel(alerts)
    }

    private fun action(
        a: String,
        code: Int,
    ): PendingIntent =
        PendingIntent.getService(
            context,
            code,
            Intent(context, service).setAction(a),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    private fun open(): PendingIntent? =
        context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
            PendingIntent.getActivity(context, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }

    /** The notification for [g] ([starting]: before the core answered). */
    fun build(
        g: Guidance?,
        starting: Boolean = false,
    ): Notification {
        val b =
            NotificationCompat
                .Builder(context, CHANNEL)
                .setSmallIcon(R.drawable.ic_tracking)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setSilent(true)
                .setCategory(NotificationCompat.CATEGORY_NAVIGATION)
                .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
                .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
                .setContentIntent(open())
                .setRequestPromotedOngoing(true)
        if (g == null || starting) {
            return b.setContentTitle(context.getString(R.string.tracking_starting)).build()
        }
        val headline = GuidanceText.headline(g, l = labels)
        val detail = GuidanceText.detail(g, labels)
        b
            .setContentTitle(headline)
            .setContentText(detail)
            .setShortCriticalText(GuidanceText.chip(g, labels))
        if (g.lengthM > 0) {
            val progress = (g.progressM / g.lengthM * PROGRESS_MAX).toInt().coerceIn(0, PROGRESS_MAX)
            b.setStyle(
                NotificationCompat
                    .ProgressStyle()
                    .setProgress(progress)
                    .setProgressSegments(listOf(NotificationCompat.ProgressStyle.Segment(PROGRESS_MAX))),
            )
        }
        val p = g.proposal
        val s = g.switch
        val profile = g.profileProposal
        when {
            profile != null -> {
                b.setContentText(profile.text)
                b.addAction(0, context.getString(R.string.profile_choose), action(Tracking.ACTION_PROFILE_ACCEPT, CODE_PROFILE_ACCEPT))
                b.addAction(0, context.getString(R.string.profile_keep), action(Tracking.ACTION_PROFILE_DECLINE, CODE_PROFILE_DECLINE))
            }

            p != null -> {
                b.setContentText(p.text)
                b.addAction(0, context.getString(R.string.accept), action(Tracking.ACTION_ACCEPT, CODE_ACCEPT))
                b.addAction(0, context.getString(R.string.decline), action(Tracking.ACTION_DECLINE, CODE_DECLINE))
            }

            s != null && s.refusable -> {
                b.setContentText(context.getString(R.string.switched, s.cause))
                b.addAction(0, context.getString(R.string.refuse_switch), action(Tracking.ACTION_REFUSE_SWITCH, CODE_REFUSE))
            }
        }
        b.addAction(
            0,
            context.getString(if (g.route == "back") R.string.back_stop else R.string.bring_me_back),
            action(Tracking.ACTION_BACK, CODE_BACK),
        )
        return b.build()
    }

    /**
     * Posts the notification when its content changed; true when posted. Screen off
     * ([screenOn] false), only what matters changes it (next turn, time left, arrival,
     * phase, proposal, switch): the distances are refreshed when the screen comes on.
     */
    @android.annotation.SuppressLint("MissingPermission")
    fun update(
        g: Guidance,
        screenOn: Boolean = true,
    ): Boolean {
        val key =
            if (screenOn) {
                listOf(
                    GuidanceText.headline(g, l = labels),
                    GuidanceText.detail(g, labels),
                    g.proposal?.id,
                    g.profileProposal,
                    g.switch,
                    g.route,
                    // Progress bar by the percent.
                    (g.progressM / g.lengthM.coerceAtLeast(1.0) * PERCENT).toInt(),
                )
            } else {
                listOf(
                    g.phase,
                    g.nextTurn,
                    g.nextName,
                    GuidanceText.detail(g, labels),
                    g.proposal?.id,
                    g.profileProposal,
                    g.switch,
                    g.route,
                )
            }
        if (key == shown) return false
        shown = key
        if (nm.areNotificationsEnabled()) {
            nm.notify(ID, build(g))
            posted++
        }
        return true
    }

    companion object {
        const val ID = 4207
        const val CHANNEL = "guidance"
        const val CHANNEL_REMINDER = "return_reminder"
        const val PROGRESS_MAX = 1000
        private const val PERCENT = 100
        private const val CODE_ACCEPT = 1
        private const val CODE_DECLINE = 2
        private const val CODE_REFUSE = 3
        private const val CODE_BACK = 4
        private const val CODE_PROFILE_ACCEPT = 5
        private const val CODE_PROFILE_DECLINE = 6
    }
}
