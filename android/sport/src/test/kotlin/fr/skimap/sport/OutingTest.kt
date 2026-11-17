package fr.skimap.sport

import androidx.health.connect.client.records.ExerciseSessionRecord
import androidx.health.connect.client.records.metadata.Device
import androidx.health.connect.client.records.metadata.Metadata
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import java.time.Instant
import java.time.ZoneId
import java.time.ZoneOffset

/** The outing read from the core's GPX and plan, and the Health Connect session made of it. */
@RunWith(AndroidJUnit4::class)
class OutingTest {
    @get:Rule
    val tmp = TemporaryFolder()

    @Test
    fun readsTheCoresGpx() {
        val t = Gpx.read(SportFixtures.gpx(points = 3))
        assertEquals("Boucle du Chaumoz", t.name)
        assertEquals(3, t.points.size)
        assertEquals(SportFixtures.START_MS, t.startMs)
        assertEquals(SportFixtures.START_MS + 2 * SportFixtures.STEP_MS, t.endMs)
        assertEquals(44.8651, t.points[1].lat, 1e-9)
        assertEquals(6.76005, t.points[1].lon, 1e-9)
        assertNull(t.points[0].ele)
    }

    @Test
    fun readsElevationAndSkipsPointsWithoutTime() {
        val gpx =
            """<?xml version="1.0"?><gpx xmlns="http://www.topografix.com/GPX/1/1"><trk><name>A &amp; B</name><trkseg>
            <trkpt lat="45.1" lon="6.1"><ele>1850.5</ele><time>2026-01-15T09:30:05Z</time></trkpt>
            <trkpt lat="45.2" lon="6.2"></trkpt>
            <trkpt lat="45.0" lon="6.0"><ele>1849</ele><time>2026-01-15T09:30:00Z</time></trkpt>
            </trkseg></trk></gpx>"""
        val t = Gpx.read(gpx)
        assertEquals("A & B", t.name)
        assertEquals(listOf(45.0, 45.1), t.points.map { it.lat })
        assertEquals(1850.5, t.points[1].ele!!, 1e-9)
    }

    @Test
    fun outingTakesTheRecapAndTheConditionsOfTheMorning() {
        val o = SportFixtures.outing(tmp.root, SportType.BACKCOUNTRY_SKI)
        assertEquals("2026-01-15-chaumoz-${SportFixtures.START_MS}", o.id)
        assertEquals("Boucle du Chaumoz", o.name)
        assertEquals("BackcountrySki", o.sport.strava)
        assertEquals(
            "Sortie terminée. 6,2 km en 1 h 10.\n\n" +
                "Conditions : Neige fraîche de 10 cm, damage ce matin. Vent faible, −8 °C au départ. Fart violet.\n\n" +
                Description.SIGNATURE,
            o.description,
        )
        assertEquals(SportFixtures.START_MS + 119 * SportFixtures.STEP_MS, o.endMs)
    }

    @Test
    fun englishPhoneSignsInEnglish() {
        val d = Description.of("Outing finished. 6.2 km in 1 h 10.", listOf("Fresh snow."), french = false)
        assertEquals(
            "Outing finished. 6.2 km in 1 h 10.\n\nConditions: Fresh snow.\n\n" + Description.SIGNATURE_EN,
            d,
        )
    }

    @Test
    fun outingWithoutPlanOrRecap() {
        val gpx = SportFixtures.outingDir(tmp.root)
        java.io.File(gpx.parent, "plan.json").delete()
        val o = Outing.of(gpx.parent, gpx.path, null, 0.0, SportType.NORDIC_SKI, french = true)
        assertEquals(Description.SIGNATURE, o.description)
        assertEquals("NordicSki", o.sport.strava)
    }

    @Test
    fun aTraceOfOnePointIsNotAnOuting() {
        val gpx = SportFixtures.outingDir(tmp.root, points = 1)
        assertThrows(
            IllegalArgumentException::class.java,
        ) { Outing.of(gpx.parent, gpx.path, null, 0.0, SportType.NORDIC_SKI, french = true) }
    }

    @Test
    fun healthConnectSessionIsSkiingWithTheWholeRoute() {
        val o = SportFixtures.outing(tmp.root)
        val track = Gpx.read(java.io.File(o.gpx).readText())
        val zone = ZoneId.of("Europe/Paris")
        val r = HealthRecords.session(o, track, zone, Device(type = Device.TYPE_PHONE))
        assertEquals(ExerciseSessionRecord.EXERCISE_TYPE_SKIING, r.exerciseType)
        assertEquals(Instant.ofEpochMilli(SportFixtures.START_MS), r.startTime)
        assertEquals(Instant.ofEpochMilli(o.endMs + 1000), r.endTime)
        assertEquals(ZoneOffset.ofHours(1), r.startZoneOffset)
        assertEquals("Boucle du Chaumoz", r.title)
        assertEquals(o.description, r.notes)
        assertEquals("skimap-${o.id}", r.metadata.clientRecordId)
        assertEquals(Metadata.RECORDING_METHOD_ACTIVELY_RECORDED, r.metadata.recordingMethod)
        val route = (r.exerciseRouteResult as androidx.health.connect.client.records.ExerciseRouteResult.Data).exerciseRoute.route
        assertEquals(120, route.size)
        assertTrue(route.all { !it.time.isBefore(r.startTime) && it.time.isBefore(r.endTime) })
        assertEquals(track.points.last().lat, route.last().latitude, 1e-9)
    }
}
