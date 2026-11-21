package fr.skimap.tracking

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Stale announcements (#183): past their position, or the same text a moment ago. */
class FreshnessTest {
    private var now = 0L
    private var here: SpokenAt? = null
    private val f = Freshness({ here }, { now })

    private fun u(
        text: String,
        at: SpokenAt?,
        safety: Boolean = false,
    ) = Utterance(text, safety, listOf("turn"), at)

    /** [m] metres north of [a]. */
    private fun north(
        a: SpokenAt,
        m: Double,
        dtMs: Long,
    ) = SpokenAt(a.tMs + dtMs, a.lon, a.lat + m / METRES_PER_DEGREE)

    @Test
    fun theDistanceIsRight() {
        assertEquals(100.0, Freshness.metres(A, north(A, 100.0, 0)), 0.5)
    }

    @Test
    fun anAnnouncementPastItsPositionIsNotSaid() {
        here = north(A, 30.0, 6_000)
        assertNull(f.check(u("Dans deux cents mètres, tournez à gauche.", A)))
        here = north(A, Freshness.PASSED_M + 5, 9_000)
        assertEquals(Freshness.Stale.PASSED, f.check(u("Il reste quatre kilomètres.", A)))
        // Made at the last fix: fresh.
        assertNull(f.check(u("Il reste trois kilomètres.", here)))
    }

    @Test
    fun aSafetyAnnouncementIsSaidEvenLate() {
        here = north(A, 500.0, 60_000)
        assertNull(f.check(u("Vous avez quitté l'itinéraire.", A, safety = true)))
    }

    @Test
    fun theSameTextAMomentAgoIsNotSaidAgain() {
        here = A
        assertNull(f.check(u("Brouillard annoncé.", A, safety = true)))
        now += Freshness.REPEAT_MS - 1
        assertEquals(Freshness.Stale.REPEATED, f.check(u("Brouillard annoncé.", A, safety = true)))
        now += 1
        assertNull(f.check(u("Brouillard annoncé.", A, safety = true)))
    }

    @Test
    fun whatAButtonAskedIsAlwaysSaid() {
        here = north(A, 500.0, 60_000)
        assertNull(f.check(u("Il reste trois kilomètres.", null)))
        assertNull(f.check(u("Il reste trois kilomètres.", null)))
    }

    private companion object {
        val A = SpokenAt(1_000_000L, 6.72, 44.85)
        const val METRES_PER_DEGREE = 6_371_000.0 * Math.PI / 180
    }
}
