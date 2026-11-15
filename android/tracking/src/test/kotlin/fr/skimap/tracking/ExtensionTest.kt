package fr.skimap.tracking

import android.Manifest
import android.app.Application
import android.content.Context
import android.content.Intent
import android.location.Location
import android.location.LocationManager
import android.os.Handler
import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf

/** Records what the service gives it; says an alert at its first fix. */
class RecordingExtension : OutingExtension {
    var looper: Looper? = null
    var output: OutingExtension.Output? = null
    val batches = ArrayList<Pair<Batch, Location?>>()
    var stopped = 0

    override fun start(
        context: Context,
        handler: Handler,
        output: OutingExtension.Output,
    ) {
        looper = handler.looper
        this.output = output
    }

    override fun onBatch(
        batch: Batch,
        fix: Location?,
    ) {
        batches += batch to fix
        if (fix != null && batches.count { it.second != null } == 1) output?.alert("Léa : pas de nouvelles depuis 6 min")
    }

    override fun stop() {
        stopped++
    }
}

class ExtendedService : TrackingService() {
    override fun openCore(planDir: String): TrackingCore = FakeCore()

    override fun extensions(planDir: String): List<OutingExtension> {
        dir = planDir
        return listOf(extension)
    }

    companion object {
        var extension = RecordingExtension()
        var dir: String? = null
    }
}

@RunWith(RobolectricTestRunner::class)
class ExtensionTest {
    private val app = ApplicationProvider.getApplicationContext<Application>()

    @Before
    fun grant() {
        shadowOf(app).grantPermissions(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.POST_NOTIFICATIONS)
        ExtendedService.extension = RecordingExtension()
    }

    @Test
    fun extensionsRunOnTheServiceThreadWithEveryBatchAndStopAtTheEnd() {
        val intent =
            Intent(app, ExtendedService::class.java)
                .setAction(Tracking.ACTION_START)
                .putExtra(Tracking.EXTRA_PLAN_DIR, app.filesDir.absolutePath)
        val controller = Robolectric.buildService(ExtendedService::class.java, intent).create().startCommand(0, 1)
        val s = controller.get()
        shadowOf(s.looper).idle()
        val e = ExtendedService.extension
        // Started once, on the service's own thread, with the plan of the outing.
        assertEquals(s.looper, e.looper)
        assertEquals(app.filesDir.absolutePath, ExtendedService.dir)
        // The screen event's batch: no fix.
        assertNull(e.batches.single().second)
        val lm = app.getSystemService(LocationManager::class.java)
        val fix =
            Location(LocationManager.GPS_PROVIDER).apply {
                time = 1_000
                latitude = 44.85
                longitude = 6.72
            }
        shadowOf(lm).simulateLocation(s.providerName, fix)
        shadowOf(s.looper).idle()
        // The GPS batch: the core's batch and the last fix.
        val (_, last) = e.batches.last()
        assertNotNull(last)
        assertEquals(44.85, last!!.latitude, 0.0)
        // Its alert is said as a safety alert of the core, and vibrated.
        val said = s.stats.said.single { "group" in it.kinds }
        assertEquals("Léa : pas de nouvelles depuis 6 min", said.text)
        assertTrue(said.safety)
        // The end stops it.
        s.onStartCommand(Intent(app, ExtendedService::class.java).setAction(Tracking.ACTION_STOP), 0, 2)
        shadowOf(s.looper).idle()
        assertTrue(e.stopped >= 1)
    }
}
