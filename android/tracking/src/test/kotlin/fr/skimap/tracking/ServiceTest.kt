package fr.skimap.tracking

import android.Manifest
import android.app.Application
import android.app.NotificationManager
import android.content.Intent
import android.hardware.Sensor
import android.hardware.SensorManager
import android.location.Location
import android.location.LocationManager
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.shadows.ShadowSensor

/** A core that answers what the test scripts: one batch per call, recorded. */
class FakeCore : TrackingCore {
    val packed = ArrayList<ByteArray>()
    val events = ArrayList<String>()
    var next: (Guidance) -> Batch = { Batch(emptyList(), emptyList(), emptyList(), emptyList(), emptyList(), null, null, it) }

    var guidance =
        Guidance(
            Phase.WAITING,
            "plan",
            "Le Laus – Le Bourget",
            15_800.0,
            15_800.0,
            0.0,
            Turn.LEFT,
            1_200.0,
            "Greffier",
            11_400.0,
            "3 h 10",
            "12 h 20",
            0,
            null,
            "16 h 30",
            Double.POSITIVE_INFINITY,
            GpsMode.IDLE,
            null,
            null,
        )

    private fun batch() = next(guidance)

    override fun pushFixes(packed: ByteArray): Batch {
        this.packed += packed
        return batch()
    }

    override fun screen(
        tMs: Long,
        on: Boolean,
    ): Batch {
        events += "screen $on"
        return batch()
    }

    override fun activity(
        tMs: Long,
        activity: Activity,
    ): Batch {
        events += "activity $activity"
        return batch()
    }

    override fun closed(
        tMs: Long,
        ways: LongArray,
    ): Batch {
        events += "closed ${ways.size}"
        return batch()
    }

    override fun answer(
        tMs: Long,
        accept: Boolean,
    ): Batch {
        events += "answer $accept"
        return batch()
    }

    override fun refuse(tMs: Long): Batch {
        events += "refuse"
        return batch()
    }

    override fun backStart(tMs: Long): Batch {
        events += "back"
        return batch()
    }

    override fun backStop(): Batch {
        events += "back stop"
        return batch()
    }

    override fun sayNext(tMs: Long): Utterance? = null

    override fun sayRemaining(tMs: Long): Utterance? = null

    override fun guidance(): Guidance = guidance

    override fun observation(): String = "{}"

    override fun writeGpx(): String = ""

    override fun close() = Unit
}

/** The service of the test, on the fake core. */
class FakeService : TrackingService() {
    override fun openCore(planDir: String): TrackingCore = core

    companion object {
        var core = FakeCore()
    }
}

@RunWith(RobolectricTestRunner::class)
class ServiceTest {
    private val app = ApplicationProvider.getApplicationContext<Application>()

    @Before
    fun grant() {
        shadowOf(app).grantPermissions(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.POST_NOTIFICATIONS)
        FakeService.core = FakeCore()
        // The Pixel has the fused provider and the significant motion sensor; Robolectric
        // has neither by default.
        shadowOf(app.getSystemService(LocationManager::class.java)).setProviderEnabled(LocationManager.FUSED_PROVIDER, true)
        shadowOf(app.getSystemService(SensorManager::class.java)).addSensor(ShadowSensor.newInstance(Sensor.TYPE_SIGNIFICANT_MOTION))
    }

    private fun start(): TrackingService {
        val intent =
            Intent(app, FakeService::class.java)
                .setAction(Tracking.ACTION_START)
                .putExtra(Tracking.EXTRA_PLAN_DIR, app.filesDir.absolutePath)
        val s =
            Robolectric
                .buildService(FakeService::class.java, intent)
                .create()
                .startCommand(0, 1)
                .get()
        shadowOf(s.looper).idle()
        return s
    }

    private fun loc(
        t: Long,
        lon: Double,
        lat: Double,
    ) = Location(LocationManager.GPS_PROVIDER).apply {
        time = t
        elapsedRealtimeNanos = t * 1_000_000
        longitude = lon
        latitude = lat
        accuracy = 5f
    }

    @Test
    fun startsInTheForegroundAppliesTheCoresGpsModeAndBatches() {
        val s = start()
        assertEquals(LocationManager.FUSED_PROVIDER, s.providerName)
        // Foreground with the guidance notification, the screen state told to the core.
        assertTrue(shadowOf(s).lastForegroundNotification != null)
        assertEquals(listOf("screen true"), FakeService.core.events)
        val lm = app.getSystemService(LocationManager::class.java)
        val first = shadowOf(lm).getLocationRequests(s.providerName)
        assertEquals(1, first.size)
        assertEquals(1_000, first[0].intervalMillis)
        // The core asks for the pocket mode: a fix every 5 s in batches of 60 s.
        FakeService.core.next = { g ->
            Batch(
                emptyList(),
                emptyList(),
                emptyList(),
                emptyList(),
                listOf("started"),
                GpsRequest(GpsMode.POCKET, true, 5_000, 60_000, false),
                null,
                g.copy(phase = Phase.MOVING),
            )
        }
        shadowOf(lm).simulateLocation(s.providerName, loc(1_000, 6.72, 44.85), loc(6_000, 6.721, 44.851))
        shadowOf(s.looper).idle()
        // One batch, one call to the core, 2 fixes packed.
        assertEquals(1, FakeService.core.packed.size)
        assertEquals(48, FakeService.core.packed[0].size)
        val r = shadowOf(lm).getLocationRequests(s.providerName).last()
        assertEquals(5_000, r.intervalMillis)
        assertEquals(5_000, r.minUpdateIntervalMillis)
        assertEquals(60_000, r.maxUpdateDelayMillis)
        assertEquals(2, s.stats.requests)
        // The same request again: the chip is not asked again.
        shadowOf(lm).simulateLocation(s.providerName, loc(11_000, 6.722, 44.852))
        shadowOf(s.looper).idle()
        assertEquals(2, s.stats.requests)
        val st = Tracking.state.value as TrackingState.Running
        assertEquals(Phase.MOVING, st.guidance.phase)
        assertEquals(GpsMode.POCKET, st.gps?.mode)
        // The notification shows the next turn and the time left.
        val n = shadowOf(app.getSystemService(NotificationManager::class.java)).getNotification(GuidanceNotifier.ID)
        assertEquals("← 1,2 km à gauche · Greffier", n.extras.getCharSequence("android.title").toString())
        assertEquals("3 h 10 restantes · arrivée 12 h 20", n.extras.getCharSequence("android.text").toString())
    }

    @Test
    fun pauseTurnsTheGnssOffAndArmsSignificantMotion() {
        val s = start()
        FakeService.core.next = { g ->
            Batch(
                emptyList(),
                emptyList(),
                emptyList(),
                emptyList(),
                listOf("paused"),
                GpsRequest(GpsMode.PAUSE, false, 0, 0, true),
                null,
                g.copy(phase = Phase.PAUSED),
            )
        }
        val lm = app.getSystemService(LocationManager::class.java)
        shadowOf(lm).simulateLocation(s.providerName, loc(1_000, 6.72, 44.85))
        shadowOf(s.looper).idle()
        val passive = shadowOf(lm).getLocationRequests(LocationManager.PASSIVE_PROVIDER)
        assertEquals(1, passive.size)
        assertEquals(GpsPolicy.PASSIVE_MIN_INTERVAL_MS, passive[0].minUpdateIntervalMillis)
        assertTrue(shadowOf(lm).getLocationRequests(s.providerName).isEmpty())
        assertEquals(1, s.stats.motionArmed)
    }

    @Test
    fun notificationActionsReachTheCore() {
        val s = start()
        for (a in listOf(Tracking.ACTION_ACCEPT, Tracking.ACTION_DECLINE, Tracking.ACTION_REFUSE_SWITCH, Tracking.ACTION_BACK)) {
            s.onStartCommand(Intent(app, FakeService::class.java).setAction(a), 0, 2)
        }
        shadowOf(s.looper).idle()
        assertEquals(listOf("screen true", "answer true", "answer false", "refuse", "back"), FakeService.core.events)
    }

    @Test
    fun alertsVibrateAndProposalsShowTheirAnswers() {
        val s = start()
        val p = Proposal(1, "short_loop", "Vous allez 40 % moins vite que prévu. Je propose la boucle courte, retour 15 h 20.")
        FakeService.core.next = { g ->
            Batch(
                emptyList(),
                listOf(AlertKind.OFF_ROUTE),
                emptyList(),
                listOf(p),
                emptyList(),
                null,
                null,
                g.copy(phase = Phase.MOVING, proposal = p),
            )
        }
        val lm = app.getSystemService(LocationManager::class.java)
        shadowOf(lm).simulateLocation(s.providerName, loc(1_000, 6.72, 44.85))
        shadowOf(s.looper).idle()
        assertEquals(listOf(AlertKind.OFF_ROUTE), s.stats.alerts)
        val n = shadowOf(app.getSystemService(NotificationManager::class.java)).getNotification(GuidanceNotifier.ID)
        val titles = n.actions.map { it.title.toString() }
        assertEquals(listOf("Accepter", "Refuser", "Ramène-moi"), titles)
        assertEquals(p.text, n.extras.getCharSequence("android.text").toString())
    }
}
