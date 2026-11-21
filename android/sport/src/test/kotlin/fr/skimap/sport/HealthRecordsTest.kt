package fr.skimap.sport

import androidx.health.connect.client.records.DistanceRecord
import androidx.health.connect.client.records.ElevationGainedRecord
import androidx.health.connect.client.records.ExerciseSessionRecord
import androidx.health.connect.client.records.metadata.Device
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.time.Instant
import java.time.ZoneId
import java.time.ZoneOffset

/** The records of an outing in Health Connect, as the permissions allow them. */
class HealthRecordsTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val paris = ZoneId.of("Europe/Paris")
    private val device = Device(type = Device.TYPE_PHONE)

    @Test
    fun sessionWithRouteDistanceAndClimb() {
        val o = SportFixtures.outing(tmp.root)
        val r = HealthRecords.of(o, HealthPermissions.ALL, paris, device)
        assertEquals(3, r.size)
        val s = r[0] as ExerciseSessionRecord
        assertEquals(ExerciseSessionRecord.EXERCISE_TYPE_SKIING, s.exerciseType)
        assertEquals("Boucle du Chaumoz", s.title)
        assertEquals("6,2 km en 1 h 10.", s.notes)
        assertEquals(Instant.ofEpochMilli(o.startMs), s.startTime)
        // Every route point before the end: one second after the last.
        assertEquals(Instant.ofEpochMilli(o.endMs + 1000), s.endTime)
        assertEquals(ZoneOffset.ofHours(1), s.startZoneOffset)
        assertEquals("skimap-2026-01-15-boucle-session", s.metadata.clientRecordId)
        val route = (s.exerciseRouteResult as androidx.health.connect.client.records.ExerciseRouteResult.Data).exerciseRoute.route
        assertEquals(o.points.size, route.size)
        assertEquals(o.points[7].lat, route[7].latitude, 0.0)
        assertEquals(o.points[7].ele!!, route[7].altitude!!.inMeters, 1e-9)
        val d = r[1] as DistanceRecord
        assertEquals(o.distanceM, d.distance.inMeters, 1e-9)
        assertEquals("skimap-2026-01-15-boucle-distance", d.metadata.clientRecordId)
        val e = r[2] as ElevationGainedRecord
        assertEquals(119.0, e.elevation.inMeters, 1e-9)
        assertEquals(s.startTime, e.startTime)
    }

    @Test
    fun onlyWhatIsAllowed() {
        val o = SportFixtures.outing(tmp.root)
        val r = HealthRecords.of(o, setOf(HealthPermissions.SESSION), paris, device)
        assertEquals(1, r.size)
        val s = r[0] as ExerciseSessionRecord
        assertTrue(s.exerciseRouteResult !is androidx.health.connect.client.records.ExerciseRouteResult.Data)
        // Without the session's permission nothing is written.
        assertEquals(emptyList<Any>(), HealthRecords.of(o, setOf(HealthPermissions.DISTANCE, HealthPermissions.ROUTE), paris, device))
        // No altitude known: no climb record.
        val flat = o.copy(ascentM = null)
        assertNull(HealthRecords.of(flat, HealthPermissions.ALL, paris, device).firstOrNull { it is ElevationGainedRecord })
    }
}
