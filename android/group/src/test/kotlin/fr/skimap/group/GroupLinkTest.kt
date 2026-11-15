package fr.skimap.group

import android.content.Context
import android.location.Location
import android.os.Handler
import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import fr.skimap.tracking.AlertKind
import fr.skimap.tracking.Batch
import fr.skimap.tracking.GpsMode
import fr.skimap.tracking.Guidance
import fr.skimap.tracking.Phase
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** A core that records what it is given and answers what the test says. */
class FakeSession : GroupSessionCore {
    val fixes = ArrayList<OwnFix?>()
    val frames = ArrayList<List<ByteArray>>()
    var alerts: List<GroupAlert> = emptyList()
    var prefix = byteArrayOf(1, 2, 3, 4, 5)
    var closed = false

    override fun tick(
        nowMs: Long,
        fix: OwnFix?,
        frames: List<ByteArray>,
    ): Tick {
        fixes += fix
        this.frames += frames
        val a = alerts
        alerts = emptyList()
        val members =
            listOf(
                Member(0, "Pierre", true, fix?.lat, fix?.lon, 0, 0, stopped = false, offRoute = false, batteryLow = false, distanceM = 0.0),
                Member(1, "Léa", false, null, null, null, null, stopped = false, offRoute = false, batteryLow = false, distanceM = null),
            )
        return Tick(fix?.let { byteArrayOf(9) }, null, listOf(prefix), a, members, frames.size)
    }

    override fun close() {
        closed = true
    }
}

class FakeBackend(
    val session: FakeSession = FakeSession(),
) : GroupBackend {
    var opened: Triple<String, Int, String?>? = null
    val created = ArrayList<List<String>>()

    override val serviceUuid = 0xFD5C

    override fun create(
        key: ByteArray,
        members: List<String>,
        planDir: String?,
        createdS: Long,
    ): GroupInfo {
        check(key.size == 32 && key.any { it != 0.toByte() })
        created += members
        return GroupInfo("SKIMAP:G1:TEST", "ab12", members, planDir != null, 0)
    }

    override fun read(qrText: String): GroupInfo {
        require(qrText.startsWith("SKIMAP:G1:")) { "not a group" }
        val member = qrText.substringAfter('/', "").toIntOrNull()
        return GroupInfo(qrText.substringBefore('/'), "ab12", listOf("Pierre", "Léa"), true, member)
    }

    override fun qr(
        qrText: String,
        member: Int?,
    ) = QrMatrix(21, BooleanArray(21 * 21) { it % 3 == (member ?: 0) % 3 })

    override fun open(
        qrText: String,
        me: Int,
        planDir: String?,
    ): GroupSessionCore {
        opened = Triple(qrText, me, planDir)
        return session
    }
}

class FakeRadio : GroupRadio {
    override var available = true
    val scans = ArrayList<List<ByteArray>>()
    val adverts = ArrayList<Pair<ByteArray?, ByteArray?>>()
    var onFrames: ((List<ByteArray>) -> Unit)? = null
    var stopped = false

    override fun scan(
        prefixes: List<ByteArray>,
        batchDelayMs: Long,
        onFrames: (List<ByteArray>) -> Unit,
    ) {
        scans += prefixes
        this.onFrames = onFrames
    }

    override fun advertise(
        own: ByteArray?,
        relay: ByteArray?,
    ) {
        adverts += own to relay
    }

    override fun stop() {
        stopped = true
    }
}

fun guidance(
    phase: Phase = Phase.MOVING,
    route: String = "plan",
    progress: Double = 1200.0,
) = Guidance(
    phase = phase,
    route = route,
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

fun batch(
    g: Guidance = guidance(),
    alerts: List<AlertKind> = emptyList(),
) = Batch(emptyList(), alerts, emptyList(), emptyList(), emptyList(), null, null, g)

fun location(
    t: Long,
    lat: Double,
    lon: Double,
) = Location("gps").apply {
    time = t
    latitude = lat
    longitude = lon
}

@RunWith(RobolectricTestRunner::class)
class GroupLinkTest {
    private val context: Context = ApplicationProvider.getApplicationContext()
    private val handler = Handler(Looper.getMainLooper())
    private val store = GroupStore(context)

    @Before
    fun clear() = store.leave()

    @Test
    fun withoutAGroupNothingRuns() {
        val radio = FakeRadio()
        val backend = FakeBackend()
        val link = GroupLink(backend, store, "/plan", { _, _ -> radio })
        link.start(context, handler) { }
        link.onBatch(batch(), location(1, 44.0, 6.0))
        assertNull(backend.opened)
        assertTrue(radio.scans.isEmpty() && radio.adverts.isEmpty())
        assertNull(Groups.live.value)
    }

    @Test
    fun fixesFramesAlertsAndStop() {
        store.join(Joined("SKIMAP:G1:TEST", 0))
        val radio = FakeRadio()
        val backend = FakeBackend()
        val said = ArrayList<String>()
        val link = GroupLink(backend, store, "/plan", { _, _ -> radio }, clock = { 1_000L })
        link.start(context, handler) { said += it }
        assertEquals(Triple("SKIMAP:G1:TEST", 0, "/plan"), backend.opened)
        // The scan starts at once on the group's prefixes; nothing to advertise before a fix.
        assertEquals(1, radio.scans.size)
        assertArrayEquals(byteArrayOf(1, 2, 3, 4, 5), radio.scans[0][0])
        assertNull(radio.adverts.last().first)

        // A GPS batch: this phone's position, its progress along the plan, the frame advertised.
        link.onBatch(batch(), location(900, 44.87, 6.73))
        val f = backend.session.fixes.last()!!
        assertEquals(44.87, f.lat, 0.0)
        assertEquals(OwnPhase.MOVING, f.phase)
        assertEquals(1200.0, f.progressM!!, 0.0)
        assertFalse(f.offRoute)
        assertArrayEquals(byteArrayOf(9), radio.adverts.last().first)
        // Same prefixes: the scan is not restarted (5 starts per 30 s at most).
        assertEquals(1, radio.scans.size)

        // The tracker says off the route, then a plan B: off-route flag kept, no progress.
        link.onBatch(batch(guidance(route = "plan_b"), listOf(AlertKind.OFF_ROUTE)), location(901, 44.87, 6.73))
        assertTrue(
            backend.session.fixes
                .last()!!
                .offRoute,
        )
        assertNull(
            backend.session.fixes
                .last()!!
                .progressM,
        )
        link.onBatch(batch(alerts = listOf(AlertKind.BACK_ON_ROUTE)), null)
        link.onBatch(batch(guidance(phase = Phase.PAUSED)), location(902, 44.87, 6.73))
        assertFalse(
            backend.session.fixes
                .last()!!
                .offRoute,
        )
        assertEquals(
            OwnPhase.PAUSED,
            backend.session.fixes
                .last()!!
                .phase,
        )

        // Frames heard: to the core; an alert: said by the service.
        backend.session.alerts = listOf(GroupAlert(GroupAlertKind.STRAGGLER, 1, "Léa : pas de nouvelles depuis 6 min"))
        radio.onFrames!!(listOf(byteArrayOf(7, 7)))
        assertArrayEquals(byteArrayOf(7, 7), backend.session.frames.last()[0])
        assertEquals(listOf("Léa : pas de nouvelles depuis 6 min"), said)
        assertEquals(1, link.told.size)
        val live = Groups.live.value
        assertNotNull(live)
        assertEquals(2, live!!.members.size)
        assertTrue(live.radio)

        // New epoch: new prefixes, the scan restarts.
        backend.session.prefix = byteArrayOf(1, 9, 9, 9, 9)
        link.onBatch(batch(), location(903, 44.87, 6.73))
        assertEquals(2, radio.scans.size)

        link.stop()
        assertTrue(radio.stopped && backend.session.closed)
        assertNull(Groups.live.value)
        assertNull(GroupLink.active)
    }

    @Test
    fun qrCodesAreDecoded() {
        val text = "SKIMAP:G1:AEBAGBAFAYDQQCIKBMGA2DQPCAIREEYUCULBOGA"
        val m = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, 300, 300)
        val y = ByteArray(300 * 300) { i -> if (m[i % 300, i / 300]) 0 else -1 }
        assertEquals(text, decodeQr(y, 300, 300))
        assertNull(decodeQr(ByteArray(300 * 300) { -1 }, 300, 300))
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun controllerCreatesJoinsAndLeaves() {
        val d = StandardTestDispatcher()
        val scope = TestScope(d)
        val backend = FakeBackend()
        var plan: String? = null
        val c = GroupController(backend, store, { plan }, scope, d, now = { 5_000L })
        c.load()
        scope.advanceUntilIdle()
        assertEquals(GroupUi.NoGroup(), c.ui.value)
        assertFalse(c.hasPlan)
        // Names trimmed, empty ones dropped, long ones cut to 24 bytes of UTF-8.
        plan = "/plans/1"
        c.create(listOf(" Pierre ", "", "Éléonore-Marie-Clémentine de B."))
        scope.advanceUntilIdle()
        val created = backend.created.single()
        assertEquals("Pierre", created[0])
        assertTrue(created[1].toByteArray().size <= 24 && created[1].startsWith("Éléonore"))
        assertEquals(Joined("SKIMAP:G1:TEST", 0), store.joined())
        assertTrue((c.ui.value as GroupUi.InGroup).info.hasPlan)

        c.leave()
        scope.advanceUntilIdle()
        assertNull(store.joined())
        c.scanned("https://example.org")
        scope.advanceUntilIdle()
        assertEquals(GroupUi.NoGroup(invalidQr = true), c.ui.value)
        // A group without a member's number: not a code to join with.
        c.scanned("SKIMAP:G1:OTHER")
        scope.advanceUntilIdle()
        assertEquals(GroupUi.NoGroup(invalidQr = true), c.ui.value)
        // Léa's code: scanning is all there is to do.
        c.scanned("SKIMAP:G1:OTHER/1")
        scope.advanceUntilIdle()
        assertEquals(Joined("SKIMAP:G1:OTHER", 1), store.joined())
        val joined = c.ui.value as GroupUi.InGroup
        assertEquals(1, joined.me)
        assertEquals(2, joined.qrs.size)
        // Reopened: the group is still there.
        val c2 = GroupController(backend, store, { null }, scope, d)
        c2.load()
        scope.advanceUntilIdle()
        assertEquals(1, (c2.ui.value as GroupUi.InGroup).me)
    }
}
