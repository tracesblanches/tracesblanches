package fr.skimap.app

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.location.Location
import android.location.LocationManager
import android.os.IBinder
import android.os.ParcelFileDescriptor
import android.os.Process
import android.util.Log
import androidx.core.content.ContextCompat
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import fr.skimap.app.guidance.SkiTrackingService
import fr.skimap.tracking.Activity
import fr.skimap.tracking.GpsPolicy
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingService
import fr.skimap.tracking.TrackingState
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.time.Instant
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * Q9 scenarios replayed on the phone through the real location service, the real core
 * and the real audio (K4, K5, K6, E1): the inputs of an outing simulated by the core's
 * bench (`crates/ffi/examples/q9_export.rs`, pushed to `/data/local/tmp/skimap-q9/`) are
 * given to the service in the batches the location provider would deliver for the GPS
 * request the service applies; the service must say, switch and set the GPS exactly as
 * the core run on the workstation, and every announcement must be played (pack clips or
 * Android voice). Needs the Cervières pack in `/data/local/tmp/skimap-packs/cervieres/`.
 */
@RunWith(AndroidJUnit4::class)
class TrackingReplayTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val target: Context = instrumentation.targetContext

    private fun shell(cmd: String): String =
        ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(cmd)).use {
            it.readBytes().decodeToString()
        }

    private fun prepare(name: String): File? {
        val pack = File(MainActivity.packsDir(target), "cervieres")
        if (!File(pack, "manifest.json").isFile) {
            pack.parentFile?.mkdirs()
            shell("cp -r /data/local/tmp/skimap-packs/cervieres ${pack.parentFile!!.absolutePath}/")
        }
        val q9 = File(target.getExternalFilesDir(null), "q9")
        q9.mkdirs()
        shell("cp -r /data/local/tmp/skimap-q9/$name ${q9.absolutePath}/")
        val src = File(q9, name)
        if (!File(src, "expected.json").isFile || !File(pack, "manifest.json").isFile) return null
        val dir = File(target.filesDir, "plans/q9-$name").apply { deleteRecursively() }
        dir.mkdirs()
        for (f in listOf("plan.json", "plan.trk", "scenario.jsonl", "expected.json")) File(src, f).copyTo(File(dir, f), overwrite = true)
        File(dir, "pack.txt").writeText(pack.absolutePath)
        return dir
    }

    private fun grant() {
        for (p in listOf("ACCESS_FINE_LOCATION", "ACCESS_COARSE_LOCATION", "POST_NOTIFICATIONS")) {
            shell("pm grant ${target.packageName} android.permission.$p")
        }
    }

    private fun bind(): Pair<TrackingService, ServiceConnection> {
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
        target.bindService(Intent(target, SkiTrackingService::class.java), conn, Context.BIND_AUTO_CREATE)
        assertTrue("service bound", latch.await(30, TimeUnit.SECONDS))
        return service!! to conn
    }

    /** Runs on the service's thread and waits for it. */
    private fun onService(
        s: TrackingService,
        block: () -> Unit,
    ) {
        val done = CountDownLatch(1)
        s.post {
            block()
            done.countDown()
        }
        assertTrue(done.await(60, TimeUnit.SECONDS))
    }

    private fun loc(
        t: Long,
        lon: Double,
        lat: Double,
    ) = Location(LocationManager.GPS_PROVIDER).apply {
        time = t
        longitude = lon
        latitude = lat
        accuracy = 8f
    }

    private fun ms(o: JSONObject) = Instant.parse(o.getString("t")).toEpochMilli()

    /** CPU of the threads of the outing, milliseconds, by name (from /proc). */
    private fun threadCpu(): Map<String, Long> {
        val tick = 10L
        val out = HashMap<String, Long>()
        File("/proc/self/task").listFiles().orEmpty().forEach { t ->
            val stat = runCatching { File(t, "stat").readText() }.getOrNull() ?: return@forEach
            val name = stat.substring(stat.indexOf('(') + 1, stat.lastIndexOf(')'))
            val f = stat.substring(stat.lastIndexOf(')') + 2).split(' ')
            val ms = (f[11].toLong() + f[12].toLong()) * tick
            out[name] = (out[name] ?: 0L) + ms
        }
        return out
    }

    private fun replay(name: String) {
        val dir = prepare(name)
        assumeTrue("Q9 data or pack missing on the device", dir != null)
        grant()
        // The app in the foreground: the location service may start (a gesture, report 09).
        val scenario = ActivityScenario.launch(MainActivity::class.java)
        try {
            run(name, dir!!) { scenario.close() }
        } finally {
            scenario.close()
        }
    }

    private fun run(
        name: String,
        dir: File,
        started: () -> Unit,
    ) {
        val start =
            Intent(target, SkiTrackingService::class.java)
                .setAction(Tracking.ACTION_START)
                .putExtra(Tracking.EXTRA_PLAN_DIR, dir.absolutePath)
                .putExtra(Tracking.EXTRA_REPLAY, true)
        ContextCompat.startForegroundService(target, start)
        val (s, conn) = bind()
        val deadline = System.currentTimeMillis() + 30_000
        while (Tracking.state.value !is TrackingState.Running && System.currentTimeMillis() < deadline) Thread.sleep(50)
        assertTrue("tracking running", Tracking.state.value is TrackingState.Running)
        // The foreground service runs on its own now: the map goes (its drawing is not the outing's cost).
        started()
        Thread.sleep(2000)
        val expected = JSONObject(File(dir, "expected.json").readText())
        val gpsWant = expected.getJSONArray("gps")
        val inputs = File(dir, "scenario.jsonl").readLines().filter { it.isNotBlank() }.map(::JSONObject)
        val cpu0 = Process.getElapsedCpuTime()
        val threads0 = threadCpu()
        val wall0 = System.currentTimeMillis()
        var i = 0
        var batches = 0
        val gpsMismatch = ArrayList<String>()
        while (i < inputs.size) {
            val o = inputs[i]
            val lastT: Long
            if (o.has("fix")) {
                // The batch the provider delivers for the request the service applies.
                val applied = s.stats.gpsLog.lastOrNull() ?: GpsPolicy.START
                val delay = GpsPolicy.batchDelayMs(applied)
                val first = ms(o.getJSONObject("fix"))
                val batch = ArrayList<Location>()
                while (i < inputs.size && inputs[i].has("fix")) {
                    val f = inputs[i].getJSONObject("fix")
                    val t = ms(f)
                    if (batch.isNotEmpty() && t - first > delay) break
                    val p = f.getJSONObject("pos")
                    batch += loc(t, p.getDouble("lon"), p.getDouble("lat"))
                    i++
                }
                lastT = batch.last().time
                onService(s) { s.fixes(batch) }
                batches++
            } else {
                val key = o.keys().next()
                val e = o.getJSONObject(key)
                val t = ms(e)
                lastT = t
                onService(s) {
                    s.inject { core ->
                        when (key) {
                            "activity" -> {
                                core.activity(
                                    t,
                                    when (e.getString("activity")) {
                                        "still" -> Activity.STILL
                                        "on_foot" -> Activity.ON_FOOT
                                        "in_vehicle" -> Activity.IN_VEHICLE
                                        "significant_motion" -> Activity.SIGNIFICANT_MOTION
                                        else -> Activity.UNKNOWN
                                    },
                                )
                            }

                            "closed" -> {
                                val w = e.getJSONArray("ways")
                                core.closed(t, LongArray(w.length()) { w.getLong(it) })
                            }

                            "refuse" -> {
                                core.refuse(t)
                            }

                            "answer" -> {
                                core.answer(t, e.getBoolean("accept"))
                            }

                            else -> {
                                core.screen(t, e.getBoolean("on"))
                            }
                        }
                    }
                }
                onService(s) {}
                i++
            }
            // The GPS request in force after the batch: the core's last one up to then.
            var want: JSONObject? = null
            for (k in 0 until gpsWant.length()) {
                if (gpsWant.getJSONObject(k).getLong("t_ms") <= lastT) want = gpsWant.getJSONObject(k)
            }
            val got = s.stats.gpsLog.last()
            if (want != null &&
                (
                    got.mode.name.lowercase() != want.getString("mode") ||
                        got.intervalMs != want.getLong("interval_ms") ||
                        got.maxDelayMs != want.getLong("max_delay_ms")
                )
            ) {
                gpsMismatch += "batch $batches at ${Instant.ofEpochMilli(lastT)}: got $got, want $want"
            }
        }
        val cpu = Process.getElapsedCpuTime() - cpu0
        val wall = System.currentTimeMillis() - wall0
        val threads = threadCpu().mapValues { (k, v) -> v - (threads0[k] ?: 0L) }.filterValues { it > 0 }
        val inner = { o: JSONObject -> o.getJSONObject(o.keys().next()) }
        val outing = (ms(inner(inputs.last())) - ms(inner(inputs.first()))) / 1000.0
        Log.i(
            "skimap",
            "Q9 $name: ${inputs.size} inputs, $batches batches, ${s.stats.notified} notifications, outing ${"%.0f".format(outing)} s; " +
                "replay $wall ms wall, process CPU $cpu ms (${"%.1f".format(
                    cpu * 3600.0 / outing,
                )} ms per hour of outing); threads $threads",
        )
        assertEquals("GPS requests differ: ${gpsMismatch.take(5)}", 0, gpsMismatch.size)

        // Said as the core said, in order; the pack's clips where the core has them.
        val spokenWant = expected.getJSONArray("spoken")
        val said = s.stats.said.toList()
        assertEquals(
            (0 until spokenWant.length()).map { spokenWant.getJSONObject(it).getString("text") },
            said.map { it.text },
        )
        assertEquals(
            (0 until spokenWant.length()).map { spokenWant.getJSONObject(it).getBoolean("all_clips") },
            said.map { it.allClips },
        )
        // Switches and their refusals.
        val sw = expected.getJSONArray("switches")
        val route = { v: Any -> if (v is JSONObject) v.keys().next() else v.toString() }
        assertEquals(
            (0 until sw.length()).map { route(sw.getJSONObject(it).get("to")) to sw.getJSONObject(it).getBoolean("reverted") },
            s.stats.switches.map { it.route to it.reverted },
        )
        // Proposals (C11) and the decisions of the outing.
        val pr = expected.getJSONArray("proposals")
        assertEquals((0 until pr.length()).map { pr.getJSONObject(it).getString("text") }, s.stats.proposals.map { it.text })
        val ses = expected.getJSONArray("session")
        val events =
            (0 until ses.length()).map {
                val ev = ses.getJSONObject(it).get("event")
                val k = if (ev is JSONObject) ev.keys().next() else ev.toString()
                if (k == "finished") (ev as JSONObject).getJSONObject("finished").getString("end") else k
            }
        assertEquals(events, s.stats.session)
        assertEquals(alerts(expected.getJSONArray("alerts")), s.stats.alerts.map { it.name.lowercase() })

        // Every announcement played: the pack's clips decoded and assembled, else the Android voice.
        val until = System.currentTimeMillis() + 240_000
        while (s.stats.voiced < said.size && System.currentTimeMillis() < until) Thread.sleep(200)
        Log.i(
            "skimap",
            "Q9 $name: ${said.size} said, ${s.stats.voicedClips} with the pack's clips, ${s.stats.voicedTts} with the Android voice",
        )
        assertEquals(said.size, s.stats.voiced)
        assertEquals(said.count { it.allClips }, s.stats.voicedClips)
        assertEquals(said.count { !it.allClips }, s.stats.voicedTts)

        // The end: the recap, the GPX of the outing, what the group learns.
        val ended = Tracking.state.value
        assertTrue("$ended", ended is TrackingState.Ended && ended.recap?.arrived == true)
        val gpx = File((ended as TrackingState.Ended).gpx!!).readText()
        // Every fix given to the core is in the GPX (those after the last words are dropped).
        assertEquals(s.stats.fixes, gpx.split("<trkpt").size - 1)
        assertTrue(s.stats.fixes <= inputs.count { it.has("fix") })
        assertTrue(File(dir, TrackingService.OBSERVATION_FILE).readText().contains("moving_s"))
        target.unbindService(conn)
    }

    private fun alerts(a: JSONArray): List<String> =
        (0 until a.length()).map {
            when (val v = a.getJSONObject(it).get("alert")) {
                is JSONObject -> {
                    val c = v.getString("condition")
                    if (c == "wind_rising") "wind_rising" else c
                }

                else -> {
                    v.toString()
                }
            }
        }

    @Test
    fun refugePauseAndGpsModes() = replay("refuge")

    @Test
    fun closedTrailSwitchRefused() = replay("closure_refused")

    @Test
    fun wrongTurnOffRouteAndBack() = replay("wrong_turn")

    @Test
    fun familyProposal() = replay("family")
}
