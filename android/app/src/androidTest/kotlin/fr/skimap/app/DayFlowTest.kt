package fr.skimap.app

import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.location.Location
import android.location.LocationManager
import android.location.provider.ProviderProperties
import android.os.IBinder
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.util.Log
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.isEnabled
import androidx.compose.ui.test.junit4.v2.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.UiDevice
import fr.skimap.app.day.DayActivity
import fr.skimap.app.day.DayTags
import fr.skimap.app.guidance.SkiTrackingService
import fr.skimap.core.readDayPlan
import fr.skimap.tracking.GpsMode
import fr.skimap.tracking.GuidanceNotifier
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingService
import fr.skimap.tracking.TrackingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.cos

/**
 * The day on the emulator, in real time (J5, K4, K10, K13): the morning computed on the
 * phone from the Chapelle-des-Bois pack, a new group typed in its dialog, the contact,
 * « Valider la journée » (the plan, the location service, the SMS pre-filled), then
 * simulated positions along the plan through the platform location provider (a test
 * provider replacing `fused`), the screen off, the volume keys and the headset button,
 * « ramène-moi », the lock-screen notification, the stop, the GPX. Captures in
 * `/sdcard/Android/data/fr.skimap.app/files/shots/`. Needs the pack in
 * `/data/local/tmp/skimap-packs/chapelle-des-bois/`.
 */
@RunWith(AndroidJUnit4::class)
@OptIn(ExperimentalTestApi::class)
class DayFlowTest {
    @get:Rule
    val rule = createEmptyComposeRule()

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val target: Context = instrumentation.targetContext
    private val device = UiDevice.getInstance(instrumentation)
    private val shots = File(target.getExternalFilesDir(null), "shots").apply { mkdirs() }

    private fun shell(cmd: String): String =
        ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(cmd)).use {
            it.readBytes().decodeToString()
        }

    private fun shot(name: String) {
        shell("screencap -p ${shots.absolutePath}/$name.png")
    }

    private fun bind(): TrackingService {
        val latch = CountDownLatch(1)
        var service: TrackingService? = null
        target.bindService(
            Intent(target, SkiTrackingService::class.java),
            object : ServiceConnection {
                override fun onServiceConnected(
                    name: ComponentName?,
                    binder: IBinder?,
                ) {
                    service = (binder as TrackingService.Local).service
                    latch.countDown()
                }

                override fun onServiceDisconnected(name: ComponentName?) = Unit
            },
            Context.BIND_AUTO_CREATE,
        )
        assertTrue(latch.await(30, TimeUnit.SECONDS))
        return service!!
    }

    private fun waitFor(
        what: String,
        timeoutMs: Long,
        cond: () -> Boolean,
    ) {
        // The test drives the Compose clock: waiting must let the app's coroutines run.
        runCatching { rule.waitUntil(timeoutMs) { cond() } }
        assertTrue(what, cond())
    }

    @Test
    fun theDayFromTheMorningToTheEnd() {
        val pack = File(MainActivity.packsDir(target), "chapelle-des-bois")
        if (!File(pack, "manifest.json").isFile) {
            pack.parentFile?.mkdirs()
            shell("cp -r /data/local/tmp/skimap-packs/chapelle-des-bois ${pack.parentFile!!.absolutePath}/")
        }
        assumeTrue("pack missing on the device", File(pack, "manifest.json").isFile)
        for (p in listOf("ACCESS_FINE_LOCATION", "ACCESS_COARSE_LOCATION", "POST_NOTIFICATIONS")) {
            shell("pm grant ${target.packageName} android.permission.$p")
        }
        shell("appops set ${target.packageName} android:mock_location allow")
        target
            .getSharedPreferences("skimap_day", Context.MODE_PRIVATE)
            .edit()
            .clear()
            .commit()
        shell("input keyevent KEYCODE_WAKEUP")
        shell("wm dismiss-keyguard")

        // The morning, computed on the phone.
        val t0 = SystemClock.uptimeMillis()
        val scenario =
            ActivityScenario.launch<DayActivity>(
                Intent(target, DayActivity::class.java).putExtra(DayActivity.EXTRA_SITE, "chapelle-des-bois"),
            )
        rule.waitUntil(180_000) {
            runCatching { rule.onNode(hasTestTag(DayTags.VALIDATE) and isEnabled()).fetchSemanticsNode() }.isSuccess
        }
        Log.i("skimap", "day flow: morning on screen in ${SystemClock.uptimeMillis() - t0} ms")
        shot("01-morning")
        // A new group, typed in its dialog (gloves off, in the morning).
        rule.onNodeWithTag(DayTags.LIST).performScrollToNode(hasTestTag(DayTags.GROUP_NEW))
        rule.onNodeWithTag(DayTags.GROUP_NEW).performClick()
        rule.onNode(hasText("Nom") and hasAnyAncestor(isDialog())).performTextInput("Avec Léa")
        rule.onNode(hasText("Âges des enfants (ex. 6, 9)") and hasAnyAncestor(isDialog())).performTextInput("8, 11")
        shot("02-group-dialog")
        rule.onNode(hasText("Enregistrer") and hasAnyAncestor(isDialog())).performClick()
        rule.waitUntil(180_000) {
            runCatching { rule.onNodeWithText("1 adulte, enfants de 8 et 11 ans", substring = true).fetchSemanticsNode() }.isSuccess &&
                runCatching { rule.onNode(hasTestTag(DayTags.VALIDATE) and isEnabled()).fetchSemanticsNode() }.isSuccess
        }
        // The contact.
        rule.onNodeWithTag(DayTags.LIST).performScrollToNode(hasTestTag(DayTags.CONTACT))
        rule.onNodeWithTag(DayTags.CONTACT).performClick()
        rule.onNode(hasText("Nom") and hasAnyAncestor(isDialog())).performTextInput("Marie")
        rule.onNode(hasText("Téléphone") and hasAnyAncestor(isDialog())).performTextInput("0612345678")
        rule.onNode(hasText("Enregistrer") and hasAnyAncestor(isDialog())).performClick()
        rule.onNodeWithTag(DayTags.LIST).performScrollToNode(hasTestTag(DayTags.LOOP + 0))
        shot("03-loops")

        // « Valider la journée »: one gesture.
        rule.onNodeWithTag(DayTags.VALIDATE).performClick()
        waitFor("tracking", 60_000) { Tracking.state.value is TrackingState.Running }
        val planDir = (Tracking.state.value as TrackingState.Running).planDir
        // The SMS app (or the share sheet) came over the app with the plan: the user sends it.
        rule.waitForIdle()
        Thread.sleep(3000)
        shot("04-sms")
        Log.i("skimap", "day flow: over the app after validation: ${shell("dumpsys activity activities | grep -m1 mResumedActivity")}")
        device.pressBack()
        device.pressBack()
        val s = bind()
        val plan = readDayPlan(planDir)
        assertTrue(plan.sms, plan.sms.contains("112"))

        // Simulated positions through the platform: a test provider in place of `fused`.
        val lm = target.getSystemService(LocationManager::class.java)
        val provider = s.providerName
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
        var seg = 0
        var along = 0.0
        val speed = 4.0
        val fixAt = { meters: Double ->
            // Position along the polyline of the plan, metres from the start.
            var rem = meters
            var i = 0
            var p = pts[0]
            while (i + 1 < pts.size) {
                val (ax, ay) = pts[i]
                val (bx, by) = pts[i + 1]
                val dx = (bx - ax) * 111_320 * cos(Math.toRadians(ay))
                val dy = (by - ay) * 110_540
                val d = Math.hypot(dx, dy)
                if (rem <= d) {
                    val f = if (d > 0) rem / d else 0.0
                    p = (ax + f * (bx - ax)) to (ay + f * (by - ay))
                    break
                }
                rem -= d
                i++
                p = pts[i]
            }
            p
        }
        val send = { lon: Double, lat: Double ->
            val l =
                Location(provider).apply {
                    longitude = lon
                    latitude = lat
                    accuracy = 6f
                    time = System.currentTimeMillis()
                    elapsedRealtimeNanos = SystemClock.elapsedRealtimeNanos()
                }
            lm.setTestProviderLocation(provider, l)
        }
        val saidBefore = { s.stats.said.size }
        val events = ArrayList<String>()
        for (k in 0 until 150) {
            if (k >= 15) along += speed
            val (lon, lat) = fixAt(along)
            send(lon, lat)
            when (k) {
                60 -> {
                    shell("input keyevent KEYCODE_SLEEP")
                    events += "screen off"
                }

                75 -> {
                    val n = saidBefore()
                    shell("input keyevent KEYCODE_VOLUME_DOWN")
                    events += "volume down ($n said before)"
                    shell("dumpsys media_session")
                        .lines()
                        .filter {
                            it.contains("skimap") || it.contains("Media button session") || it.contains("active=") ||
                                it.contains("state=PlaybackState") || it.contains("volumeType")
                        }.take(20)
                        .forEach { Log.i("skimap", "day flow media_session: $it") }
                }

                90 -> {
                    val n = saidBefore()
                    shell("input keyevent KEYCODE_HEADSETHOOK")
                    events += "headset ($n said before)"
                }

                110 -> {
                    shell("input keyevent KEYCODE_WAKEUP")
                    shell("wm dismiss-keyguard")
                    events += "screen on"
                }
            }
            rule.mainClock.advanceTimeBy(1000)
            Thread.sleep(1000)
            seg++
        }
        val running = Tracking.state.value as TrackingState.Running
        Log.i(
            "skimap",
            "day flow: ${s.stats.fixes} fixes in ${s.stats.batches} batches, ${s.stats.requests} requests, " +
                "modes ${s.stats.gpsLog.map { it.mode }.distinct()}, session ${s.stats.session}, events $events",
        )
        s.stats.said.forEach { Log.i("skimap", "day flow said [${it.kinds}] ${it.text}") }
        assertTrue("started: ${s.stats.session}", "started" in s.stats.session)
        assertTrue(s.stats.batches > 10)
        val modes =
            s.stats.gpsLog
                .map { it.mode }
                .toSet()
        assertTrue("$modes", modes.any { it != GpsMode.IDLE })
        assertTrue(running.guidance.progressM > 100)
        // The buttons: the time left (volume down) and the next direction (headset) were said.
        val kinds = s.stats.said.flatMap { it.kinds }
        Log.i("skimap", "day flow kinds $kinds")
        assertTrue("time left by the volume key: $kinds", "remaining" in kinds)
        assertTrue("next direction by the headset button: $kinds", "next" in kinds)
        // The lock-screen notification.
        val n = target.getSystemService(NotificationManager::class.java).activeNotifications.firstOrNull { it.id == GuidanceNotifier.ID }
        assertNotNull(n)
        val title =
            n!!
                .notification.extras
                .getCharSequence("android.title")
                .toString()
        val text =
            n.notification.extras
                .getCharSequence("android.text")
                .toString()
        Log.i("skimap", "day flow notification: « $title » « $text », promoted=${n.notification.flags and 0x40000 != 0}")
        // The next direction and its distance; the time left, or the switch to refuse in one gesture.
        assertTrue(title, title.contains(" m ") || title.startsWith("Arrivée"))
        assertTrue(text, text.contains("restantes") || text.startsWith("Itinéraire changé") || text.startsWith("Vous"))
        val actions =
            n.notification.actions
                .orEmpty()
                .map { it.title.toString() }
        Log.i("skimap", "day flow notification actions $actions")
        assertTrue("$actions", "Ramène-moi" in actions)
        // The lock screen: screen off, then on without unlocking.
        shell("input keyevent KEYCODE_SLEEP")
        Thread.sleep(2000)
        shell("input keyevent KEYCODE_WAKEUP")
        Thread.sleep(2500)
        shot("05-lock-screen")
        shell("wm dismiss-keyguard")
        Thread.sleep(1000)

        // The map with the thin banner.
        val map =
            ActivityScenario.launch<MainActivity>(
                Intent(target, MainActivity::class.java).putExtra(MainActivity.EXTRA_PACK, "chapelle-des-bois"),
            )
        // Software rendering on the emulator: the first frame of the map takes a while.
        val mapUntil = SystemClock.uptimeMillis() + 60_000
        while (SystemClock.uptimeMillis() < mapUntil && fr.skimap.app.ui.map.MapProbe.rendered.value == null) Thread.sleep(500)
        Thread.sleep(3000)
        shot("06-map-guidance")
        map.close()

        // « Ramène-moi », then back to the plan.
        Tracking.command(target, SkiTrackingService::class.java, Tracking.ACTION_BACK)
        waitFor("back", 10_000) { (Tracking.state.value as? TrackingState.Running)?.guidance?.route == "back" }
        Thread.sleep(1500)
        val back = s.stats.said.last { "back" in it.kinds }
        Log.i("skimap", "day flow back: ${back.text}")
        assertTrue(back.text, back.text.startsWith("Il reste"))
        Tracking.command(target, SkiTrackingService::class.java, Tracking.ACTION_BACK)
        waitFor("back stop", 10_000) { (Tracking.state.value as? TrackingState.Running)?.guidance?.route != "back" }

        // The plan screen during the outing, then the stop.
        val planScreen =
            ActivityScenario.launch<DayActivity>(
                Intent(target, DayActivity::class.java).putExtra(DayActivity.EXTRA_SITE, "chapelle-des-bois"),
            )
        rule.waitUntil(30_000) {
            runCatching { rule.onNodeWithText("Plan du jour").fetchSemanticsNode() }.isSuccess
        }
        Thread.sleep(1500)
        shot("07-plan")
        Tracking.command(target, SkiTrackingService::class.java, Tracking.ACTION_STOP)
        waitFor("ended", 30_000) { Tracking.state.value is TrackingState.Ended }
        val ended = Tracking.state.value as TrackingState.Ended
        val gpx = File(ended.gpx!!).readText()
        val points = gpx.split("<trkpt").size - 1
        Log.i(
            "skimap",
            "day flow: GPX $points points, voiced ${s.stats.voiced} (${s.stats.voicedClips} clips, ${s.stats.voicedTts} Android voice)",
        )
        assertTrue(points >= 100)
        assertTrue(File(planDir, TrackingService.OBSERVATION_FILE).isFile)
        assertEquals(null, Tracking.activePlan(target))
        lm.removeTestProvider(provider)
        planScreen.close()
        scenario.close()
    }
}
