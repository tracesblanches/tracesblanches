package fr.skimap.app

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.location.Location
import android.location.LocationManager
import android.location.provider.ProviderProperties
import android.os.IBinder
import android.os.ParcelFileDescriptor
import android.os.Process
import android.os.SystemClock
import android.util.Log
import androidx.core.content.ContextCompat
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import fr.skimap.app.guidance.SkiTrackingService
import fr.skimap.core.DayRequest
import fr.skimap.core.dayStarts
import fr.skimap.core.openDayPlanner
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingService
import fr.skimap.tracking.TrackingState
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.time.LocalDate
import java.time.LocalTime
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.cos
import kotlin.random.Random

/**
 * CPU of an outing followed with the screen off, in real time on the emulator (E1, K8): a
 * plan of today on the Cervières pack, the location service started as by « Valider la
 * journée », the screen off, one simulated fix a second along the plan (3 m/s, GPS noise of
 * a few metres) through a test provider in place of `fused`; the CPU of the app's process
 * and of each of its threads is read from /proc at the start and the end. Duration:
 * instrumentation argument `minutes` (10 by default; 60 for the hour of report 09). The
 * energy itself is measured on the Pixel (ODPM rails): this is indicative.
 */
@RunWith(AndroidJUnit4::class)
class TrackingEnergyTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val target: Context = instrumentation.targetContext

    private fun shell(cmd: String): String =
        ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(cmd)).use {
            it.readBytes().decodeToString()
        }

    private fun threads(): Map<String, Long> {
        val out = HashMap<String, Long>()
        File("/proc/self/task").listFiles().orEmpty().forEach { t ->
            val stat = runCatching { File(t, "stat").readText() }.getOrNull() ?: return@forEach
            val name = stat.substring(stat.indexOf('(') + 1, stat.lastIndexOf(')'))
            val f = stat.substring(stat.lastIndexOf(')') + 2).split(' ')
            out[name] = (out[name] ?: 0L) + (f[11].toLong() + f[12].toLong()) * 10
        }
        return out
    }

    // One scripted measurement, step by step.
    @Suppress("CyclomaticComplexMethod")
    @Test
    fun screenOffOutingCpu() {
        val minutes = InstrumentationRegistry.getArguments().getString("minutes")?.toInt() ?: 10
        val pack = File(MainActivity.packsDir(target), "cervieres")
        if (!File(pack, "manifest.json").isFile) {
            pack.parentFile?.mkdirs()
            shell("cp -r /data/local/tmp/skimap-packs/cervieres ${pack.parentFile!!.absolutePath}/")
        }
        assumeTrue(File(pack, "manifest.json").isFile)
        for (p in listOf("ACCESS_FINE_LOCATION", "ACCESS_COARSE_LOCATION", "POST_NOTIFICATIONS")) {
            shell("pm grant ${target.packageName} android.permission.$p")
        }
        shell("appops set ${target.packageName} android:mock_location allow")
        // Today's plan, starting now (the morning on the phone).
        val planner = openDayPlanner(pack.absolutePath, File(target.filesDir, "packview").absolutePath)
        val now = LocalTime.now()
        val req =
            DayRequest(
                "3 h",
                dayStarts("cervieres").first(),
                LocalDate.now().toString(),
                "%02d:%02d".format(now.hour, now.minute),
                "08:00",
                180.0,
                false,
                null,
                null,
            )
        val m = planner.morning(req, null)
        assumeTrue("no loop today", m.loops.isNotEmpty())
        val dir = File(target.filesDir, "plans/energy").apply { deleteRecursively() }
        val plan = planner.validate(0u, null, dir.absolutePath)
        val ui = ActivityScenario.launch(MainActivity::class.java)
        ContextCompat.startForegroundService(
            target,
            Intent(target, SkiTrackingService::class.java).setAction(Tracking.ACTION_START).putExtra(Tracking.EXTRA_PLAN_DIR, plan.planDir),
        )
        val latch = CountDownLatch(1)
        var s: TrackingService? = null
        target.bindService(
            Intent(target, SkiTrackingService::class.java),
            object : ServiceConnection {
                override fun onServiceConnected(
                    n: ComponentName?,
                    b: IBinder?,
                ) {
                    s = (b as TrackingService.Local).service
                    latch.countDown()
                }

                override fun onServiceDisconnected(n: ComponentName?) = Unit
            },
            Context.BIND_AUTO_CREATE,
        )
        assertTrue(latch.await(30, TimeUnit.SECONDS))
        val svc = s!!
        val until = SystemClock.uptimeMillis() + 30_000
        while (Tracking.state.value !is TrackingState.Running && SystemClock.uptimeMillis() < until) Thread.sleep(100)
        ui.close()
        val lm = target.getSystemService(LocationManager::class.java)
        val provider = svc.providerName
        runCatching { lm.removeTestProvider(provider) }
        lm.addTestProvider(
            provider,
            ProviderProperties
                .Builder()
                .setHasSatelliteRequirement(true)
                .setAccuracy(ProviderProperties.ACCURACY_FINE)
                .setPowerUsage(ProviderProperties.POWER_USAGE_HIGH)
                .build(),
        )
        lm.setTestProviderEnabled(provider, true)
        val line = plan.line
        val pts = (0 until line.size / 2).map { line[2 * it] to line[2 * it + 1] }
        val cum = DoubleArray(pts.size)
        for (i in 1 until pts.size) {
            val (ax, ay) = pts[i - 1]
            val (bx, by) = pts[i]
            cum[i] = cum[i - 1] + Math.hypot((bx - ax) * 111_320 * cos(Math.toRadians(ay)), (by - ay) * 110_540)
        }
        val rnd = Random(7)
        val at = { d: Double ->
            val m0 = d % cum.last()
            var i = 1
            while (i < cum.size - 1 && cum[i] < m0) i++
            val f = ((m0 - cum[i - 1]) / (cum[i] - cum[i - 1]).coerceAtLeast(1e-6)).coerceIn(0.0, 1.0)
            val (ax, ay) = pts[i - 1]
            val (bx, by) = pts[i]
            (ax + f * (bx - ax) + rnd.nextDouble(-3e-5, 3e-5)) to (ay + f * (by - ay) + rnd.nextDouble(-2e-5, 2e-5))
        }
        // Standing at the start for 20 s, then skiing; the screen goes off after 30 s.
        val total = minutes * 60
        var cpu0 = 0L
        var th0 = emptyMap<String, Long>()
        var batches0 = 0
        var fixes0 = 0
        var t0 = 0L
        for (k in 0 until total + 30) {
            if (k == 30) {
                shell("input keyevent KEYCODE_SLEEP")
                Thread.sleep(2000)
                cpu0 = Process.getElapsedCpuTime()
                th0 = threads()
                batches0 = svc.stats.batches
                fixes0 = svc.stats.fixes
                t0 = SystemClock.elapsedRealtime()
            }
            val (lon, lat) = at(if (k < 20) 0.0 else (k - 20) * 3.0)
            lm.setTestProviderLocation(
                provider,
                Location(provider).apply {
                    longitude = lon
                    latitude = lat
                    accuracy = 5f
                    time = System.currentTimeMillis()
                    elapsedRealtimeNanos = SystemClock.elapsedRealtimeNanos()
                },
            )
            Thread.sleep(1000)
        }
        val wall = SystemClock.elapsedRealtime() - t0
        val cpu = Process.getElapsedCpuTime() - cpu0
        val th = threads().mapValues { (k, v) -> v - (th0[k] ?: 0L) }.filterValues { it > 0 }
        val perHour = cpu * 3_600_000.0 / wall
        Log.i(
            "skimap",
            "energy: screen off ${wall / 1000} s, process CPU $cpu ms (${"%.0f".format(perHour)} ms per hour, " +
                "${"%.3f".format(perHour / 36_000.0)} % of one core), " +
                "${svc.stats.fixes - fixes0} fixes in ${svc.stats.batches - batches0} batches, ${svc.stats.requests} requests, " +
                "${svc.stats.notified} notifications, ${svc.stats.said.size} said, modes ${svc.stats.gpsLog.map { it.mode }.distinct()}, " +
                "session ${svc.stats.session}; threads $th",
        )
        shell("input keyevent KEYCODE_WAKEUP")
        Tracking.command(target, SkiTrackingService::class.java, Tracking.ACTION_STOP)
        val end = SystemClock.uptimeMillis() + 60_000
        while (Tracking.state.value !is TrackingState.Ended && SystemClock.uptimeMillis() < end) Thread.sleep(200)
        lm.removeTestProvider(provider)
        assertTrue("started" in svc.stats.session)
    }
}
