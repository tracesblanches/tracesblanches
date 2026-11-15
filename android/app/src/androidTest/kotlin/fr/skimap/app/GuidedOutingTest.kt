package fr.skimap.app

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.hardware.Sensor
import android.hardware.SensorManager
import android.os.IBinder
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import fr.skimap.app.fog.Compass
import fr.skimap.app.fog.GuidedTrackingService
import fr.skimap.app.fog.Outings
import fr.skimap.core.FogTarget
import fr.skimap.core.Lang
import fr.skimap.core.guidedBearing
import fr.skimap.core.outingHistory
import fr.skimap.core.outingRecap
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingService
import fr.skimap.tracking.TrackingState
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * A bearing outing in the fog (K7) through the real location service of the app on the
 * device: the guided core says the bearing, then the arrival, the outing ends on its own,
 * its recap (A10) is written, the barometer recorded (K8) when the device has one; the
 * compass of the device gives a heading.
 */
@RunWith(AndroidJUnit4::class)
class GuidedOutingTest {
    private val target: Context = InstrumentationRegistry.getInstrumentation().targetContext

    private fun shell(cmd: String) {
        InstrumentationRegistry
            .getInstrumentation()
            .uiAutomation
            .executeShellCommand(cmd)
            .close()
    }

    private fun bind(): TrackingService {
        val latch = CountDownLatch(1)
        var service: TrackingService? = null
        val conn =
            object : ServiceConnection {
                override fun onServiceConnected(
                    name: ComponentName?,
                    binder: IBinder?,
                ) {
                    service = (binder as TrackingService.Local).service
                    latch.countDown()
                }

                override fun onServiceDisconnected(name: ComponentName?) = Unit
            }
        target.bindService(Intent(target, GuidedTrackingService::class.java), conn, Context.BIND_AUTO_CREATE)
        assertTrue("service bound", latch.await(BIND_S, TimeUnit.SECONDS))
        return service!!
    }

    @Test
    fun bearingOutingEndsWithItsRecap() {
        for (p in listOf("ACCESS_FINE_LOCATION", "ACCESS_COARSE_LOCATION", "POST_NOTIFICATIONS")) {
            shell("pm grant ${target.packageName} android.permission.$p")
        }
        val dir = Outings.newOutingDir(target)
        val refuge = FogTarget("refuge", "Refuge test", 6.0, 45.0 + DIST_M / M_PER_DEG, null)
        guidedBearing(dir.path, refuge, null, 0.0, Lang.FR, null, null)
        val start =
            Intent(target, GuidedTrackingService::class.java)
                .setAction(Tracking.ACTION_START)
                .putExtra(Tracking.EXTRA_PLAN_DIR, dir.path)
                .putExtra(Tracking.EXTRA_REPLAY, true)
        target.startForegroundService(start)
        val s = bind()
        runBlocking { withTimeout(TIMEOUT_MS) { Tracking.state.first { it is TrackingState.Running } } }
        val t0 = System.currentTimeMillis()
        (1..FIXES).chunked(BATCH).forEach { batch ->
            val b = ByteBuffer.allocate(batch.size * FIX_BYTES).order(ByteOrder.LITTLE_ENDIAN)
            batch.forEach { i -> b.putLong(t0 + i * STEP_MS).putDouble(6.0).putDouble(45.0 + i * STEP_M / M_PER_DEG) }
            s.inject { it.pushFixes(b.array()) }
        }
        val ended = runBlocking { withTimeout(TIMEOUT_MS) { Tracking.state.first { it is TrackingState.Ended } } } as TrackingState.Ended
        assertEquals(dir.path, ended.planDir)
        val said = s.stats.said.map { it.text }
        assertTrue(said.toString(), said.first().startsWith("Refuge test : 1,3 kilomètre, cap 0 degrés, nord"))
        assertTrue(said.toString(), said.any { it.startsWith("Arrivée : Refuge test") })
        val h = outingHistory(Outings.plansRoot(target).path)
        assertTrue(h.any { it.dir == dir.path && it.kind == "fog" })
        val r = outingRecap(dir.path, Outings.plansRoot(target).path, Outings.workDir(target).path)
        assertTrue(r.text, r.text.contains("Distance : 1,"))
        assertTrue(File(dir, "trace.gpx").isFile)
        val sm = target.getSystemService(SensorManager::class.java)
        if (sm.getDefaultSensor(Sensor.TYPE_PRESSURE) != null) {
            assertTrue("pressure recorded", File(dir, "pressure.txt").isFile)
        }
        // The compass of the device (rotation vector) gives a heading while the screen is on.
        val c = Compass(sm)
        if (c.start()) {
            val reading = runBlocking { withTimeout(TIMEOUT_MS) { c.reading.first { it.headingDeg != null } } }
            assertNotNull(reading.headingDeg)
            c.stop()
        }
    }

    private companion object {
        const val DIST_M = 1300.0
        const val M_PER_DEG = 111_195.0
        const val STEP_M = 6.0
        const val STEP_MS = 5000L
        const val FIXES = 240
        const val BATCH = 12
        const val FIX_BYTES = 24
        const val BIND_S = 30L
        const val TIMEOUT_MS = 60_000L
    }
}
