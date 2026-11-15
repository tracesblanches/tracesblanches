package fr.skimap.app

import android.content.Context
import android.location.Location
import android.location.LocationManager
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import fr.skimap.app.group.RustGroupBackend
import fr.skimap.core.Lang
import fr.skimap.group.BleGroupRadio
import fr.skimap.group.GroupAlertKind
import fr.skimap.group.GroupLink
import fr.skimap.group.GroupRadio
import fr.skimap.group.GroupStore
import fr.skimap.group.Groups
import fr.skimap.group.Joined
import fr.skimap.tracking.Batch
import fr.skimap.tracking.GpsMode
import fr.skimap.tracking.Guidance
import fr.skimap.tracking.Phase
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * The group mode (N2) on the device, two phones simulated in one process (the emulator has
 * no Bluetooth link between two instances): the real core (Rust, UniFFI) behind two
 * [GroupLink]s, each on its own thread as in the location service, their frames carried
 * by a loopback radio from one to the other while they are within 30 m (frame injection).
 * Scenario with a simulated clock: 10 min side by side, then Léa 0.5 m/s slower; Pierre's
 * phone must say one separation alert, Léa's none. Then the phone's real Bluetooth radio
 * is started and stopped (it must neither crash nor block without Bluetooth).
 */
@RunWith(AndroidJUnit4::class)
class GroupModeTest {
    private val target: Context = InstrumentationRegistry.getInstrumentation().targetContext

    /** Holds the last frames advertised; the test carries them to the other phone. */
    private class Loopback : GroupRadio {
        override val available = true
        var own: ByteArray? = null
        var relay: ByteArray? = null
        var scans = 0

        override fun scan(
            prefixes: List<ByteArray>,
            batchDelayMs: Long,
            onFrames: (List<ByteArray>) -> Unit,
        ) {
            scans++
        }

        override fun advertise(
            own: ByteArray?,
            relay: ByteArray?,
        ) {
            this.own = own ?: this.own
            this.relay = relay
        }

        override fun stop() = Unit
    }

    private class Phone(
        val link: GroupLink,
        val radio: Loopback,
        val thread: HandlerThread,
    ) {
        val handler = Handler(thread.looper)
        val said = ArrayList<String>()

        fun run(block: () -> Unit) {
            val done = CountDownLatch(1)
            handler.post {
                block()
                done.countDown()
            }
            assertTrue(done.await(10, TimeUnit.SECONDS))
        }
    }

    private fun guidance(progress: Double) =
        Guidance(
            phase = Phase.MOVING,
            route = "plan",
            label = "",
            remainingM = 0.0,
            lengthM = 0.0,
            progressM = progress,
            nextTurn = null,
            nextDistanceM = null,
            nextName = null,
            remainingS = 0.0,
            remainingText = "",
            arrival = "",
            arrivalMs = 0,
            arrivalRange = null,
            limit = null,
            offRouteM = 0.0,
            gpsMode = GpsMode.POCKET,
            proposal = null,
            switch = null,
        )

    // One scripted scenario: its checks read best in one place.
    @Suppress("CyclomaticComplexMethod", "NestedBlockDepth", "ComplexCondition")
    @Test
    fun twoPhonesSeparateAndTheLeaderIsTold() {
        val backend = RustGroupBackend { Lang.FR }
        // The common plan: both follow it, their progress along it is compared.
        val plan = java.io.File(target.filesDir, "plans/group-test").apply { mkdirs() }
        java.io.File(plan, "plan.json").writeText("{\"test\":\"group mode\"}")
        val g = backend.create(ByteArray(32) { (it * 31 + 7).toByte() }, listOf("Pierre", "Léa"), plan.absolutePath, 1_768_000_000)
        assertTrue(g.hasPlan)
        val t0 = 1_768_036_800_000L
        var now = t0
        val phones =
            (0..1).map { me ->
                val store = GroupStore(target, "skimap_group_test_$me").apply { join(Joined(g.qrText, me)) }
                val radio = Loopback()
                val thread = HandlerThread("group-$me").apply { start() }
                Phone(GroupLink(backend, store, plan.absolutePath, { _, _ -> radio }, clock = { now }), radio, thread)
            }
        phones.forEach { p -> p.run { p.link.start(target, p.handler) { p.said += it } } }
        val at = doubleArrayOf(0.0, 0.0)
        val speed = doubleArrayOf(2.0, 2.0)
        var heard = 0
        val step = 10_000L
        // 10 min together, then 25 min with Léa slower.
        for (k in 0 until 210) {
            if (k == 60) speed[1] = 1.5
            now += step
            phones.forEachIndexed { i, p ->
                at[i] += speed[i] * step / 1000
                val fix =
                    Location(LocationManager.GPS_PROVIDER).apply {
                        time = now
                        latitude = 44.87 + at[i] / 111_195.0
                        longitude = 6.73
                    }
                p.run {
                    p.link.onBatch(
                        Batch(emptyList(), emptyList(), emptyList(), emptyList(), emptyList(), null, null, guidance(at[i])),
                        fix,
                    )
                }
            }
            if (Math.abs(at[0] - at[1]) <= 30.0) {
                // In range: each hears the other's own and relayed frames (injected).
                for ((from, to) in listOf(0 to 1, 1 to 0)) {
                    val frames = listOfNotNull(phones[from].radio.own, phones[from].radio.relay)
                    phones[to].run { phones[to].link.receive(frames) }
                    heard++
                }
            }
        }
        val members = Groups.live.value?.members
        phones.forEach { p -> p.run { p.link.stop() } }
        phones.forEach { it.thread.quitSafely() }
        Log.i("skimap", "group mode: Pierre said ${phones[0].said}, Léa said ${phones[1].said}, $heard exchanges, members $members")
        val told = phones[0].link.told
        assertEquals(1, told.size)
        assertEquals(GroupAlertKind.STRAGGLER, told[0].kind)
        assertTrue(told[0].text, told[0].text.startsWith("Léa : pas de nouvelles depuis 5 min, dernière position"))
        assertEquals(told.map { it.text }, phones[0].said)
        assertTrue(phones[1].said.isEmpty())
        // The scan is started once, then again only when the tag changes (every 15 min).
        val epochs = (now / 900_000 - t0 / 900_000 + 1).toInt()
        assertTrue(phones.map { it.radio.scans }.toString(), phones.all { it.radio.scans == epochs })
    }

    // One scripted scenario: its checks read best in one place.
    @Suppress("CyclomaticComplexMethod", "NestedBlockDepth", "ComplexCondition")
    @Test
    fun theRealRadioStartsAndStops() {
        // « Nearby devices » granted as the group screen asks for it.
        val ui = InstrumentationRegistry.getInstrumentation().uiAutomation
        for (p in listOf("BLUETOOTH_SCAN", "BLUETOOTH_ADVERTISE")) {
            android.os.ParcelFileDescriptor
                .AutoCloseInputStream(
                    ui.executeShellCommand("pm grant ${target.packageName} android.permission.$p"),
                ).use {
                    it.readBytes()
                }
        }
        val thread = HandlerThread("group-ble").apply { start() }
        val handler = Handler(thread.looper)
        val done = CountDownLatch(1)
        var available = false
        lateinit var radio: BleGroupRadio
        handler.post {
            radio = BleGroupRadio(target, handler, RustGroupBackend().serviceUuid)
            available = radio.available
            radio.scan(listOf(ByteArray(5) { 1 }), 10_000, {})
            radio.advertise(ByteArray(21) { 2 }, ByteArray(21) { 3 })
            // Stopped at once, while both sets are still starting (the end of a short outing).
            radio.stop()
            done.countDown()
        }
        assertTrue(done.await(10, TimeUnit.SECONDS))
        thread.quitSafely()
        // The sets that started after the stop are stopped then (bounded wait, 5 s).
        val deadline = System.currentTimeMillis() + 5_000
        while (available && (radio.startedSets < 2 || radio.activeSets > 0) && System.currentTimeMillis() < deadline) Thread.sleep(50)
        Log.i(
            "skimap",
            "group mode: Bluetooth LE radio available: $available, sets started ${radio.startedSets}, active ${radio.activeSets}",
        )
        if (available) {
            assertEquals(2, radio.startedSets)
            assertEquals(0, radio.activeSets)
        }
    }
}
