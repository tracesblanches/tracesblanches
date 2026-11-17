package fr.skimap.app.ui

import android.content.res.Resources
import fr.skimap.app.R
import fr.skimap.core.TrackingException

/**
 * An error of the core said in the language of the app (VO6): the typed errors with the
 * string resources, the free one ([TrackingException.Invalid]) with the text the core
 * wrote in the language asked, any other with its message.
 */
object CoreErrors {
    fun text(
        e: Throwable,
        res: Resources,
    ): String =
        when (e) {
            is TrackingException.Invalid -> e.detail
            is TrackingException.Unsupported -> res.getString(R.string.error_unsupported_site, e.site)
            is TrackingException.NoMorning -> res.getString(R.string.error_no_loop)
            is TrackingException.Closed -> res.getString(R.string.error_tracking_closed)
            is TrackingException.NotAnOuting -> res.getString(R.string.error_not_an_outing)
            is TrackingException.BadFixes -> res.getString(R.string.error_bad_fixes, e.bytes.toLong())
            is TrackingException.BadCarPosition -> res.getString(R.string.error_bad_car_position)
            is TrackingException.NoTrailGraph -> res.getString(R.string.error_no_trail_graph)
            else -> e.message.orEmpty()
        }
}
