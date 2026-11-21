package fr.skimap.sport

import android.app.Application
import android.util.Log
import androidx.health.connect.client.HealthConnectClient
import androidx.health.connect.client.records.DistanceRecord
import androidx.health.connect.client.records.ElevationGainedRecord
import androidx.health.connect.client.records.ExerciseRouteResult
import androidx.health.connect.client.records.ExerciseSessionRecord
import androidx.health.connect.client.records.HeartRateRecord
import androidx.health.connect.client.records.metadata.Metadata
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.time.Instant
import java.time.ZoneId
import java.time.ZoneOffset

/**
 * #63 on a device with Health Connect: the outing really written (session with its route,
 * distance, climb) and read back, written again without a second session, and the heart
 * rate of the outing's hours read.
 */
@RunWith(AndroidJUnit4::class)
class HealthConnectDeviceTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext.applicationContext as Application

    private fun shell(cmd: String) {
        instrumentation.uiAutomation.executeShellCommand(cmd).close()
    }

    @Test
    fun outingWrittenAndReadBack() =
        runBlocking {
            val pkg = context.packageName
            listOf(
                "WRITE_EXERCISE",
                "WRITE_EXERCISE_ROUTE",
                "WRITE_DISTANCE",
                "WRITE_ELEVATION_GAINED",
                "READ_HEART_RATE",
                "WRITE_HEART_RATE",
                "READ_EXERCISE",
                "READ_EXERCISE_ROUTES",
                "READ_DISTANCE",
                "READ_ELEVATION_GAINED",
            ).forEach { shell("pm grant $pkg android.permission.health.$it") }
            val api = HealthConnectApi(context)
            assertTrue("Health Connect on the device", api.available())
            val granted = api.granted()
            assertTrue("permissions granted: $granted", granted.containsAll(HealthPermissions.ALL))
            val o = SportFixtures.outing(File(context.cacheDir, "hc-${System.nanoTime()}"))
            val client = HealthConnectClient.getOrCreate(context)

            // A watch's heart rate during the outing, read back for the shared file.
            val start = Instant.ofEpochMilli(o.startMs)
            val samples = (0 until 10).map { HeartRateRecord.Sample(start.plusSeconds(it * 60L), 120L + it) }
            client.insertRecords(
                listOf(
                    HeartRateRecord(
                        startTime = start,
                        startZoneOffset = ZoneOffset.UTC,
                        endTime = start.plusSeconds(9 * 60L + 1),
                        endZoneOffset = ZoneOffset.UTC,
                        samples = samples,
                        metadata = Metadata.manualEntry(clientRecordId = "test-hr-${o.id}"),
                    ),
                ),
            )
            val heart = api.heart(o.startMs, o.endMs)
            assertEquals((0 until 10).map { 120 + it }, heart.map { it.bpm })

            val ids = api.insert(HealthRecords.of(o, granted, ZoneId.of("Europe/Paris"), HealthRecords.phone()))
            assertEquals(3, ids.size)
            val s = client.readRecord(ExerciseSessionRecord::class, ids[0]).record
            assertEquals(ExerciseSessionRecord.EXERCISE_TYPE_SKIING, s.exerciseType)
            assertEquals("Boucle du Chaumoz", s.title)
            val route = (s.exerciseRouteResult as ExerciseRouteResult.Data).exerciseRoute.route
            assertEquals(o.points.size, route.size)
            val d = client.readRecord(DistanceRecord::class, ids[1]).record
            assertEquals(o.distanceM, d.distance.inMeters, 0.01)
            val e = client.readRecord(ElevationGainedRecord::class, ids[2]).record
            assertEquals(119.0, e.elevation.inMeters, 0.01)
            Log.i(
                ShareFlow.TAG,
                "#63 health connect: session ${ids[0]}, ${route.size} route points, ${d.distance.inMeters} m, " +
                    "${e.elevation.inMeters} m climb, ${heart.size} heart samples read",
            )
            // Shared again: the same records (client record ids), not a second session.
            assertEquals(ids, api.insert(HealthRecords.of(o, granted, ZoneId.of("Europe/Paris"), HealthRecords.phone())))
            client.deleteRecords(ExerciseSessionRecord::class, listOf(ids[0]), emptyList())
            client.deleteRecords(DistanceRecord::class, listOf(ids[1]), emptyList())
            client.deleteRecords(ElevationGainedRecord::class, listOf(ids[2]), emptyList())
            client.deleteRecords(HeartRateRecord::class, emptyList(), listOf("test-hr-${o.id}"))
        }
}
