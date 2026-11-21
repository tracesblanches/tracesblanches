package fr.skimap.tracking

import android.os.SystemClock
import kotlin.math.cos
import kotlin.math.hypot

/**
 * Whether an announcement that waited in the voice queue is still worth saying (#183),
 * checked when it is given to the speech synthesis (the engine holds at most the one being
 * said and the next): an announcement waits only while the ones before it are said.
 *
 * - The same text given to the engine less than [REPEAT_MS] ago (phone clock) is not said
 *   again: no burst of an identical text, safety ones included.
 * - An announcement of the core ([Utterance.at]) made more than [PASSED_M] before the
 *   last fix known ([here]) is past its position: not said. A safety one is said all the
 *   same (it was already felt, and late is better than never).
 *
 * The announcements asked by a button or an extension (no [Utterance.at]) are always said.
 */
class Freshness(
    /** The last fix known. */
    private val here: () -> SpokenAt?,
    /** Monotonic clock, milliseconds. */
    private val clock: () -> Long = SystemClock::elapsedRealtime,
) {
    /** Why an announcement is not said. */
    enum class Stale {
        /** The same text was given to the engine just before. */
        REPEATED,

        /** The skier went past where it was made. */
        PASSED,
    }

    private val given = HashMap<String, Long>()

    /** Null when [u] is to be said now (then remembered), else why not. */
    @Synchronized
    fun check(u: Utterance): Stale? {
        if (u.at == null) return null
        val now = clock()
        given.values.removeIf { now - it >= REPEAT_MS }
        val why =
            when {
                u.text in given -> Stale.REPEATED
                !u.safety && passed(u.at) -> Stale.PASSED
                else -> null
            }
        if (why == null) given[u.text] = now
        return why
    }

    private fun passed(at: SpokenAt): Boolean {
        val h = here() ?: return false
        return h.tMs >= at.tMs && metres(at, h) > PASSED_M
    }

    companion object {
        /**
         * An identical text within this time is a burst (phone clock, ms): heard a moment
         * ago, about three announcements of the queue. Safety ones too: each one is felt
         * when it comes ([HapticPatterns.SAFETY]), said or not.
         */
        const val REPEAT_MS = 30_000L

        /**
         * Distance from where an announcement was made past which it is stale, m: a quarter
         * of the 200 m of a turn announced, about 4 s of a fast descent, 40 s of walking;
         * above the usual error of a fix outdoors.
         */
        const val PASSED_M = 50.0

        private const val EARTH_M = 6_371_000.0

        /** Distance between two positions, m (equirectangular: exact enough below a few km). */
        fun metres(
            a: SpokenAt,
            b: SpokenAt,
        ): Double {
            val lat = Math.toRadians((a.lat + b.lat) / 2)
            val dx = Math.toRadians(b.lon - a.lon) * cos(lat)
            val dy = Math.toRadians(b.lat - a.lat)
            return hypot(dx, dy) * EARTH_M
        }
    }
}
