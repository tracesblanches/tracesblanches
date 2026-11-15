package fr.skimap.sport

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.work.Configuration
import androidx.work.NetworkType
import androidx.work.WorkInfo
import androidx.work.WorkManager
import androidx.work.testing.SynchronousExecutor
import androidx.work.testing.WorkManagerTestInitHelper
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import java.io.File

/**
 * The queue (K11): an outing waits for the network, goes when it is back, the token is
 * refreshed, a revoked authorisation turns Strava off, Health Connect is written once
 * allowed. WorkManager runs synchronously ([WorkManagerTestInitHelper]); Strava and
 * skimap-server are [FakeStrava]; Health Connect is [FakeHealth].
 */
@RunWith(AndroidJUnit4::class)
class SportQueueTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val context: Context = ApplicationProvider.getApplicationContext()
    private val fake = FakeStrava()
    private val store = SportStore(context)
    private val health = FakeHealth()

    class FakeHealth : HealthWriter {
        var allowed = true
        val written = ArrayList<Outing>()

        override fun available() = true

        override suspend fun granted() = allowed

        override suspend fun insert(outing: Outing): String {
            written += outing
            return "hc-${written.size}"
        }
    }

    @Before
    fun setUp() {
        WorkManagerTestInitHelper.initializeTestWorkManager(
            context,
            Configuration.Builder().setExecutor(SynchronousExecutor()).build(),
        )
        store.setEndpoints(fake.base, fake.api)
        Sport.health = { health }
        Sport.pollDelayMs = 1
    }

    @After
    fun tearDown() {
        fake.close()
        Sport.health = { HealthConnectSender(it) }
        Sport.pollDelayMs = 2_000
    }

    private fun connect() = store.setTokens(StravaClient(fake.base, fake.api).exchange(FakeStrava.CODE))

    private fun stravaWork(): WorkInfo =
        WorkManager
            .getInstance(context)
            .getWorkInfosForUniqueWork(Sport.WORK_STRAVA)
            .get()
            .last()

    /** CoroutineWorker runs on its own dispatcher: waits (10 s at most) until the unique work [name] is over. */
    private fun await(name: String): WorkInfo =
        runBlocking {
            withTimeout(10_000) {
                WorkManager
                    .getInstance(context)
                    .getWorkInfosForUniqueWorkFlow(name)
                    .first { infos -> infos.isNotEmpty() && infos.all { it.state.isFinished } }
                    .last()
            }
        }

    @Test
    fun waitsForTheNetworkThenUploadsWithTheSportType() {
        connect()
        val o = SportFixtures.outing(tmp.root, SportType.BACKCOUNTRY_SKI)
        val q = Sport.send(context, o)
        assertEquals(Step.PENDING, q.strava.step)
        assertTrue("the queue keeps its own copy of the trace", q.outing.gpx.startsWith(context.filesDir.path))
        // Health Connect is written at once (on the phone, no network needed).
        assertEquals(WorkInfo.State.SUCCEEDED, await(Sport.WORK_HEALTH).state)
        assertEquals(listOf(o.id), health.written.map { it.id })
        assertEquals(Step.DONE, store.get(o.id)!!.health.step)

        // Airplane mode: the Strava work waits for a network, nothing is sent.
        val w = stravaWork()
        assertEquals(WorkInfo.State.ENQUEUED, w.state)
        assertEquals(NetworkType.CONNECTED, w.constraints.requiredNetworkType)
        assertTrue(fake.requests.none { it.path.startsWith("/api/v3") })

        // The network is back.
        WorkManagerTestInitHelper.getTestDriver(context)!!.setAllConstraintsMet(w.id)
        assertEquals(WorkInfo.State.SUCCEEDED, await(Sport.WORK_STRAVA).state)
        val done = store.get(o.id)!!
        assertEquals(Step.DONE, done.strava.step)
        assertEquals(FakeStrava.ACTIVITY_ID.toString(), done.strava.detail)
        val put = fake.requests.single { it.method == "PUT" }
        assertEquals("BackcountrySki", put.json().getString("sport_type"))
        assertTrue(put.json().getString("description").contains("Conditions : Neige fraîche"))
        assertEquals(1, fake.requests.count { it.path == "/api/v3/uploads" })
    }

    @Test
    fun noNetworkAfterAllIsRetriedAndNotLost() =
        runBlocking {
            connect()
            val o = SportFixtures.outing(tmp.root)
            Sport.send(context, o)
            // The server goes away (no coverage): the run asks to be retried, the outing waits.
            val port = fake.base
            fake.close()
            assertTrue(Sport.runStrava(context))
            assertEquals(Step.PENDING, store.get(o.id)!!.strava.step)
            // Back: a new server on another port stands for the network coming back.
            FakeStrava().use { back ->
                back.access = "access-1"
                store.setEndpoints(back.base, back.api)
                assertTrue(port != back.base)
                assertEquals(false, Sport.runStrava(context))
                assertEquals(Step.DONE, store.get(o.id)!!.strava.step)
                assertEquals(
                    "NordicSki",
                    back.requests
                        .single { it.method == "PUT" }
                        .json()
                        .getString("sport_type"),
                )
            }
        }

    @Test
    fun stillProcessingIsAskedAgainNextRun() =
        runBlocking {
            connect()
            fake.processing = 100
            Sport.polls = 2
            val o = SportFixtures.outing(tmp.root)
            Sport.send(context, o)
            assertTrue(Sport.runStrava(context))
            assertEquals(Step.UPLOADING, store.get(o.id)!!.strava.step)
            assertEquals(FakeStrava.UPLOAD_ID.toString(), store.get(o.id)!!.strava.detail)
            fake.processing = 0
            assertEquals(false, Sport.runStrava(context))
            assertEquals(Step.DONE, store.get(o.id)!!.strava.step)
            assertEquals("uploaded once", 1, fake.requests.count { it.path == "/api/v3/uploads" })
            Sport.polls = 8
        }

    @Test
    fun anExpiringTokenIsRefreshedFirst() =
        runBlocking {
            connect()
            store.setTokens(store.tokens()!!.copy(expiresAt = System.currentTimeMillis() / 1000 + 30))
            val o = SportFixtures.outing(tmp.root)
            Sport.send(context, o)
            Sport.runStrava(context)
            assertEquals("access-2", store.tokens()!!.access)
            assertEquals("Bearer access-2", fake.requests.first { it.path == "/api/v3/uploads" }.headers["authorization"])
            assertEquals(Step.DONE, store.get(o.id)!!.strava.step)
        }

    @Test
    fun revokedOnStravaTurnsStravaOffAndConnectingAgainResumes() =
        runBlocking {
            connect()
            val o = SportFixtures.outing(tmp.root)
            Sport.send(context, o)
            fake.access = "access-9" // the athlete revoked skimap on strava.com
            Sport.runStrava(context)
            assertNull(store.tokens())
            assertEquals(Delivery(Step.FAILED, Sport.NOT_CONNECTED), store.get(o.id)!!.strava)
            // Connected again: the outing that waited goes.
            fake.access = "access-1"
            connect()
            Sport.resume(context)
            // Both works of the chain (the first one still waits for the network) get it.
            WorkManager.getInstance(context).getWorkInfosForUniqueWork(Sport.WORK_STRAVA).get().forEach {
                WorkManagerTestInitHelper.getTestDriver(context)!!.setAllConstraintsMet(it.id)
            }
            await(Sport.WORK_STRAVA)
            assertEquals(Step.DONE, store.get(o.id)!!.strava.step)
        }

    @Test
    fun nothingSetUpThenAllowed() {
        health.allowed = false
        val o = SportFixtures.outing(tmp.root)
        val q = Sport.send(context, o)
        assertEquals(Delivery(Step.OFF, Sport.NOT_CONNECTED), q.strava)
        await(Sport.WORK_HEALTH)
        assertEquals(Delivery(Step.OFF, Sport.NOT_ALLOWED), store.get(o.id)!!.health)
        assertTrue(health.written.isEmpty())
        health.allowed = true
        Sport.resume(context)
        await(Sport.WORK_HEALTH)
        assertEquals(Step.DONE, store.get(o.id)!!.health.step)
        assertEquals(1, health.written.size)
        // Sent again: what is done stays done (no second session, no second upload).
        Sport.send(context, o)
        await(Sport.WORK_HEALTH)
        assertEquals(1, health.written.size)
    }

    @Test
    fun autoSendOnlyWhenAsked() {
        val o = SportFixtures.outing(tmp.root)
        assertNull(Sport.sendIfAuto(context, o))
        store.setAutoSend(true)
        assertEquals(o.id, Sport.sendIfAuto(context, o)!!.outing.id)
        assertTrue(File(store.get(o.id)!!.outing.gpx).isFile)
    }

    @Test
    fun badFileIsAFailureNotARetry() =
        runBlocking {
            connect()
            fake.uploadError = "Improperly formatted data."
            val o = SportFixtures.outing(tmp.root)
            Sport.send(context, o)
            assertEquals(false, Sport.runStrava(context))
            assertEquals(Delivery(Step.FAILED, "Improperly formatted data."), store.get(o.id)!!.strava)
        }
}
