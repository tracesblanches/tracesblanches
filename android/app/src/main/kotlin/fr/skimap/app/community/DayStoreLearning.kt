package fr.skimap.app.community

import android.content.Context
import fr.skimap.app.appLang
import fr.skimap.app.day.DayStore
import fr.skimap.core.GroupView
import fr.skimap.core.groupLearn
import java.io.File

/**
 * The calibration (C11) learns the outings imported from Strava (V15): they are the
 * user's own, so the group that learns is the user skiing alone (« Seul », or the first
 * group of one adult without children).
 */
class DayStoreLearning(
    private val store: DayStore,
) {
    constructor(context: Context) : this(DayStore(context))

    private fun solo(): GroupView? {
        val groups = store.groups()
        return groups.firstOrNull { it.spec.name == SOLO }
            ?: groups.firstOrNull { it.spec.adults.toInt() == 1 && it.spec.children.isEmpty() }
    }

    /** Technique of the group that learns (model times of the observations). */
    fun skating(): Boolean = solo()?.spec?.skating ?: false

    /** Learns each observation once; the number learned. */
    fun learn(observations: List<File>): Int {
        var g = solo() ?: return 0
        var n = 0
        for (f in observations) {
            g = runCatching { groupLearn(g.modelJson, f.readText(), appLang()) }.getOrNull() ?: continue
            n++
        }
        if (n > 0) store.putGroup(g)
        return n
    }

    private companion object {
        const val SOLO = "Seul"
    }
}
