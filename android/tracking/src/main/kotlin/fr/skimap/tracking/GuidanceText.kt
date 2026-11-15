package fr.skimap.tracking

import java.util.Locale
import kotlin.math.roundToLong

/**
 * Texts of the thin banner and of the lock-screen notification (French): short, rounded
 * so that they change only when it matters (the notification is posted again only when
 * one of them changes).
 */
object GuidanceText {
    /** « 80 m », « 250 m », « 1,2 km », « 12 km ». */
    fun distance(m: Double): String =
        when {
            m < STEP_10_UNDER -> "${(m / STEP_10).roundToLong() * STEP_10.toLong()} m"
            m < KM -> "${(m / STEP_50).roundToLong() * STEP_50.toLong()} m"
            m < TENTHS_UNDER -> String.format(Locale.FRANCE, "%.1f km", m / KM)
            else -> "${(m / KM).roundToLong()} km"
        }

    private const val STEP_10 = 10.0
    private const val STEP_10_UNDER = 200.0
    private const val STEP_50 = 50.0
    private const val KM = 1000.0
    private const val TENTHS_UNDER = 10_000.0

    /** « à gauche », « légèrement à droite »… */
    fun turn(t: Turn): String =
        when (t) {
            Turn.LEFT -> "à gauche"
            Turn.RIGHT -> "à droite"
            Turn.SLIGHT_LEFT -> "légèrement à gauche"
            Turn.SLIGHT_RIGHT -> "légèrement à droite"
            Turn.KEEP_LEFT -> "restez à gauche"
            Turn.KEEP_RIGHT -> "restez à droite"
            Turn.STRAIGHT -> "tout droit"
            Turn.U_TURN -> "demi-tour"
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
    ): String {
        val back = if (g.route == "back") "Retour : " else ""
        return when {
            g.phase == Phase.WAITING -> {
                "Départ ${g.label}"
            }

            g.phase == Phase.FINISHED -> {
                "Sortie terminée"
            }

            g.nextTurn != null && g.nextDistanceM != null -> {
                back + (if (withArrow) "${arrow(g.nextTurn)} " else "") + "${distance(g.nextDistanceM)} ${turn(g.nextTurn)}" +
                    (g.nextName?.let { " · $it" } ?: "")
            }

            else -> {
                back + "Arrivée dans ${distance(g.remainingM)}"
            }
        }
    }

    /** Second line: time left and arrival. */
    fun detail(g: Guidance): String {
        val pause =
            when (g.phase) {
                Phase.PAUSED -> "Pause · "
                Phase.REFUGE -> "Au refuge · "
                else -> ""
            }
        val range = g.arrivalRange?.let { " ($it)" } ?: ""
        return "$pause${g.remainingText} restantes · arrivée ${g.arrival}$range"
    }

    /** Second line of the thin banner on the map: « 1 h 05 · arrivée 12 h 40 » (the range is in the notification and the plan). */
    fun short(g: Guidance): String {
        val pause =
            when (g.phase) {
                Phase.PAUSED -> "Pause · "
                Phase.REFUGE -> "Au refuge · "
                else -> ""
            }
        return "$pause${g.remainingText} · arrivée ${g.arrival}"
    }

    /** Status bar chip (Live Updates): the distance to the next turn, else the time left. */
    fun chip(g: Guidance): String = if (g.nextDistanceM != null && g.phase == Phase.MOVING) distance(g.nextDistanceM) else g.remainingText

    /** What the user hears and reads when the core says nothing (spoken description). */
    fun accessibility(g: Guidance): String = "${headline(g)}. ${detail(g)}."
}
