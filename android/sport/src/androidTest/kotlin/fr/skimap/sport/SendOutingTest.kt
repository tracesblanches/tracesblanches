package fr.skimap.sport

import android.app.Application
import android.content.Intent
import android.util.Log
import androidx.core.net.toUri
import androidx.health.connect.client.HealthConnectClient
import androidx.health.connect.client.records.ExerciseRouteResult
import androidx.health.connect.client.records.ExerciseSessionRecord
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

/**
 * K11 on a device: the session really written to Health Connect (and read back), and the
 * Strava sign-in callback, the upload by WorkManager and the disconnection, against
 * [FakeStrava] in the test process (no Strava application exists yet).
 */
@RunWith(AndroidJUnit4::class)
class SendOutingTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext.applicationContext as Application

    private fun shell(cmd: String) {
        instrumentation.uiAutomation.executeShellCommand(cmd).close()
    }

    /** Waits for [ok] at most [ms] milliseconds (bounded, no background waiter). */
    private fun until(
        ms: Long,
        ok: () -> Boolean,
    ): Boolean {
        val end = System.currentTimeMillis() + ms
        while (System.currentTimeMillis() < end) {
            if (ok()) return true
            Thread.sleep(STEP_MS)
        }
        return ok()
    }

    @Test
    fun healthConnectSessionWithRoute() =
        runBlocking {
            val pkg = context.packageName
            listOf("WRITE_EXERCISE", "WRITE_EXERCISE_ROUTE", "READ_EXERCISE", "READ_EXERCISE_ROUTES")
                .forEach { shell("pm grant $pkg android.permission.health.$it") }
            val sender = HealthConnectSender(context)
            assertTrue("Health Connect on the device", sender.available())
            assertTrue("write permissions granted", sender.granted())
            val o = SportFixtures.outing(File(context.cacheDir, "hc-${System.nanoTime()}"))
            val id = sender.insert(o)
            val client = HealthConnectClient.getOrCreate(context)
            val r = client.readRecord(ExerciseSessionRecord::class, id).record
            assertEquals(ExerciseSessionRecord.EXERCISE_TYPE_SKIING, r.exerciseType)
            assertEquals("Boucle du Chaumoz", r.title)
            assertEquals(o.description, r.notes)
            val route = (r.exerciseRouteResult as ExerciseRouteResult.Data).exerciseRoute.route
            assertEquals(120, route.size)
            Log.i(Sport.TAG, "K11 health connect: session $id, ${route.size} route points, ${r.startTime} to ${r.endTime}")
            // Sent again: the same session (client record id), not a second one.
            assertEquals(id, sender.insert(o))
            client.deleteRecords(ExerciseSessionRecord::class, listOf(id), emptyList())
        }

    @Test
    fun stravaSignInUploadAndDisconnect() {
        FakeStrava().use { fake ->
            val store = SportStore(context)
            store.setEndpoints(fake.base, fake.api)
            store.setTokens(null)
            // The browser comes back from skimap-server's callback with the code and the state.
            store.setOauthState("state-1")
            val callback =
                Intent(Intent.ACTION_VIEW, "skimap://strava/callback?code=${FakeStrava.CODE}&scope=read,activity:write&state=state-1".toUri())
                    .setClass(context, SportActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            ActivityScenario.launch<SportActivity>(callback).use {
                assertTrue("tokens from the exchange", until(10_000) { store.tokens() != null })
            }
            assertEquals("Ada L.", store.tokens()!!.athlete)
            assertNull("the state is used once", store.oauthState())

            // « Envoyer »: queued, then WorkManager uploads it (the emulator has a network).
            val o = SportFixtures.outing(File(context.cacheDir, "st-${System.nanoTime()}"), SportType.BACKCOUNTRY_SKI)
            Sport.pollDelayMs = 200
            Sport.send(context, o)
            assertTrue("uploaded", until(30_000) { store.get(o.id)?.strava?.step == Step.DONE })
            val put = fake.requests.single { it.method == "PUT" }
            assertEquals("BackcountrySki", put.json().getString("sport_type"))
            assertNotNull(fake.field(fake.requests.single { it.path == "/api/v3/uploads" }, "file"))
            Log.i(Sport.TAG, "K11 strava: activity ${store.get(o.id)!!.strava.detail}, ${fake.requests.size} requests")

            // Disconnect: revoked through skimap-server and removed from the phone.
            instrumentation.runOnMainSync { SportViewModel(context).disconnect() }
            assertTrue("token removed", until(10_000) { store.tokens() == null })
            assertEquals(listOf("access-1"), fake.revoked)
            Sport.pollDelayMs = 2_000
        }
    }

    private companion object {
        const val STEP_MS = 100L
    }
}
