package fr.skimap.tracking

import android.content.Context
import java.util.Locale
import kotlin.math.roundToLong

/** The words of the guidance texts in one language (English by default, French on a French phone). */
data class GuidanceLabels(
    val locale: Locale,
    val left: String,
    val right: String,
    val slightLeft: String,
    val slightRight: String,
    val keepLeft: String,
    val keepRight: String,
    val straight: String,
    val uTurn: String,
    /** « Retour : » (way back), with its trailing space. */
    val back: String,
    /** « Départ %1$s ». */
    val start: String,
    val finished: String,
    /** « Arrivée dans %1$s ». */
    val arrivalIn: String,
    /** « Pause · ». */
    val paused: String,
    /** « Au refuge · ». */
    val refuge: String,
    /** « %1$s restantes · arrivée %2$s%3$s ». */
    val detail: String,
    /** « %1$s · arrivée %2$s ». */
    val short: String,
) {
    companion object {
        /** The labels of the app's language (string resources: `values`, `values-fr`). */
        fun of(context: Context): GuidanceLabels {
            val r = context.resources
            return GuidanceLabels(
                r.configuration.locales[0] ?: Locale.ENGLISH,
                r.getString(R.string.turn_left),
                r.getString(R.string.turn_right),
                r.getString(R.string.turn_slight_left),
                r.getString(R.string.turn_slight_right),
                r.getString(R.string.turn_keep_left),
                r.getString(R.string.turn_keep_right),
                r.getString(R.string.turn_straight),
                r.getString(R.string.turn_u_turn),
                r.getString(R.string.guidance_back) + " ",
                r.getString(R.string.guidance_start),
                r.getString(R.string.guidance_finished),
                r.getString(R.string.guidance_arrival_in),
                r.getString(R.string.guidance_paused) + " · ",
                r.getString(R.string.guidance_refuge) + " · ",
                r.getString(R.string.guidance_detail),
                r.getString(R.string.guidance_short),
            )
        }
    }
}

/**
 * Texts of the thin banner and of the lock-screen notification, in the app's language
 * ([GuidanceLabels], always passed by the caller): short, rounded so that they change only when
 * it matters (the notification is posted again only when one of them changes).
 */
object GuidanceText {
    /** « 80 m », « 250 m », « 1,2 km » (« 1.2 km » in English), « 12 km ». */
    fun distance(
        m: Double,
        locale: Locale,
    ): String =
        when {
            m < STEP_10_UNDER -> "${(m / STEP_10).roundToLong() * STEP_10.toLong()} m"
            m < KM -> "${(m / STEP_50).roundToLong() * STEP_50.toLong()} m"
            m < TENTHS_UNDER -> String.format(locale, "%.1f km", m / KM)
            else -> "${(m / KM).roundToLong()} km"
        }

    private const val STEP_10 = 10.0
    private const val STEP_10_UNDER = 200.0
    private const val STEP_50 = 50.0
    private const val KM = 1000.0
    private const val TENTHS_UNDER = 10_000.0

    /** « à gauche », « légèrement à droite »… */
    fun turn(
        t: Turn,
        l: GuidanceLabels,
    ): String =
        when (t) {
            Turn.LEFT -> l.left
            Turn.RIGHT -> l.right
            Turn.SLIGHT_LEFT -> l.slightLeft
            Turn.SLIGHT_RIGHT -> l.slightRight
            Turn.KEEP_LEFT -> l.keepLeft
            Turn.KEEP_RIGHT -> l.keepRight
            Turn.STRAIGHT -> l.straight
            Turn.U_TURN -> l.uTurn
        }

    /** Arrow for the notification text. */
    fun arrow(t: Turn?): String =
        when (t) {
            Turn.LEFT -> "←"
            Turn.RIGHT -> "→"
            Turn.SLIGHT_LEFT, Turn.KEEP_LEFT -> "↖"
            Turn.SLIGHT_RIGHT, Turn.KEEP_RIGHT -> "↗"
            Turn.STRAIGHT, null -> "↑"
            Turn.U_TURN -> "↩"
        }

    /** First line: the next direction and its distance, else the finish ([withArrow]: a text arrow first). */
    fun headline(
        g: Guidance,
        withArrow: Boolean = true,
        l: GuidanceLabels,
    ): String {
        val back = if (g.route == "back") l.back else ""
        return when {
            g.phase == Phase.WAITING -> {
                l.start.format(g.label)
            }

            g.phase == Phase.FINISHED -> {
                l.finished
            }

            g.nextTurn != null && g.nextDistanceM != null -> {
                back + (if (withArrow) "${arrow(g.nextTurn)} " else "") + "${distance(g.nextDistanceM, l.locale)} ${turn(g.nextTurn, l)}" +
                    (g.nextName?.let { " · $it" } ?: "")
            }

            else -> {
                back + l.arrivalIn.format(distance(g.remainingM, l.locale))
            }
        }
    }

    private fun pause(
        g: Guidance,
        l: GuidanceLabels,
    ): String =
        when (g.phase) {
            Phase.PAUSED -> l.paused
            Phase.REFUGE -> l.refuge
            else -> ""
        }

    /** Second line: time left and arrival. */
    fun detail(
        g: Guidance,
        l: GuidanceLabels,
    ): String {
        val range = g.arrivalRange?.let { " ($it)" } ?: ""
        return pause(g, l) + l.detail.format(g.remainingText, g.arrival, range)
    }

    /** Second line of the thin banner on the map: « 1 h 05 · arrivée 12 h 40 » (the range is in the notification and the plan). */
    fun short(
        g: Guidance,
        l: GuidanceLabels,
    ): String = pause(g, l) + l.short.format(g.remainingText, g.arrival)

    /** Status bar chip (Live Updates): the distance to the next turn, else the time left. */
    fun chip(
        g: Guidance,
        l: GuidanceLabels,
    ): String = if (g.nextDistanceM != null && g.phase == Phase.MOVING) distance(g.nextDistanceM, l.locale) else g.remainingText

    /** What the user hears and reads when the core says nothing (spoken description). */
    fun accessibility(
        g: Guidance,
        l: GuidanceLabels,
    ): String = "${headline(g, l = l)}. ${detail(g, l)}."
}
