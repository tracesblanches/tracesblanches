package fr.skimap.sport

import android.content.Intent
import android.net.Uri
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultRegistry
import androidx.activity.result.contract.ActivityResultContract
import androidx.compose.foundation.layout.Column
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.lightColorScheme
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.core.app.ActivityOptionsCompat
import androidx.health.connect.client.records.DistanceRecord
import androidx.health.connect.client.records.ElevationGainedRecord
import androidx.health.connect.client.records.ExerciseSessionRecord
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import kotlinx.coroutines.Job
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The share of an outing (#63) with a fake Health Connect: asked at the first share only,
 * written as allowed, the heart rate of the outing read into a second export, then the
 * file in the share menu (ACTION_SEND through the FileProvider, read grant).
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class ShareFlowTest {
    @get:Rule
    val rule = createComposeRule()

    /** The permission dialog of Health Connect, answered by the test. */
    private class Answer(
        val granted: Set<String>,
    ) : ActivityResultRegistry() {
        val asked = ArrayList<Any?>()

        override fun <I, O> onLaunch(
            requestCode: Int,
            contract: ActivityResultContract<I, O>,
            input: I,
            options: ActivityOptionsCompat?,
        ) {
            asked += input
            dispatchResult(requestCode, granted)
        }
    }

    /**
     * FileProvider keeps the path of its root per authority for the life of the process;
     * Robolectric gives every test a new cache directory: the cache is emptied first.
     */
    @Before
    fun freshFileProvider() {
        val f = androidx.core.content.FileProvider::class.java.getDeclaredField("sCache")
        f.isAccessible = true
        (f.get(null) as MutableMap<*, *>).clear()
    }

    private class Run(
        val activity: ComponentActivity,
        val flow: ShareFlow,
    )

    private fun start(
        health: FakeHealth,
        answer: Answer,
    ): Run {
        val c = Robolectric.buildActivity(ComponentActivity::class.java).create()
        val flow = ShareFlow(c.get(), health, answer)
        c.start().resume()
        return Run(c.get(), flow)
    }

    /** Runs the main looper until [job] is over (bounded). */
    private fun finish(job: Job) {
        val end = System.currentTimeMillis() + 10_000
        while (!job.isCompleted && System.currentTimeMillis() < end) {
            shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(5)
        }
        assertTrue("share finished", job.isCompleted)
    }

    private fun sent(r: Run): Intent {
        val chooser = shadowOf(r.activity).nextStartedActivity
        assertNotNull("share menu opened", chooser)
        assertEquals(Intent.ACTION_CHOOSER, chooser.action)
        @Suppress("DEPRECATION")
        return chooser.getParcelableExtra(Intent.EXTRA_INTENT)!!
    }

    @Test
    fun firstShareAsksWritesReadsHeartAndShares() {
        val heart = (0 until 60).map { HeartSample(SportFixtures.START_MS + it * 10_000L, 130 + it % 20) }
        val health = FakeHealth(heartSamples = heart)
        val answer = Answer(HealthPermissions.ALL)
        val r = start(health, answer)
        val exports = ArrayList<List<HeartSample>>()
        var result: ShareResult? = null
        val job =
            r.flow.share(ShareFormat.FIT, { h ->
                exports += h
                SportFixtures.outing(OutingShare.dir(r.activity), h)
            }) { result = it }
        finish(job)
        // Asked once, everything at once.
        assertEquals(listOf<Any?>(HealthPermissions.ALL), answer.asked)
        // Exported, then again with the heart rate of the outing's hours.
        assertEquals(2, exports.size)
        assertTrue(exports[0].isEmpty())
        assertEquals(heart.filter { it.timeMs <= SportFixtures.START_MS + 119 * 5_000L }, exports[1])
        assertEquals(SportFixtures.START_MS to SportFixtures.START_MS + 119 * 5_000L, health.heartAsked.single())
        // Health Connect: the session with its route, the distance, the climb.
        assertEquals(3, health.written.size)
        val s = health.written[0] as ExerciseSessionRecord
        assertEquals(ExerciseSessionRecord.EXERCISE_TYPE_SKIING, s.exerciseType)
        assertTrue(health.written[1] is DistanceRecord)
        assertTrue(health.written[2] is ElevationGainedRecord)
        assertEquals(ShareResult(3, true), result)
        // The FIT file in the share menu, readable by the app chosen.
        val send = sent(r)
        assertEquals(Intent.ACTION_SEND, send.action)
        assertEquals("application/vnd.ant.fit", send.type)
        assertTrue(send.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION != 0)

        @Suppress("DEPRECATION")
        val uri = send.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)!!
        assertEquals("content", uri.scheme)
        assertEquals("${r.activity.packageName}.sport.files", uri.authority)
        assertEquals("/share/skimap-2026-01-15-1030-boucle.fit", uri.path)
        assertEquals(uri, send.clipData!!.getItemAt(0).uri)
        assertEquals("Boucle du Chaumoz", send.getStringExtra(Intent.EXTRA_SUBJECT))
    }

    @Test
    fun declinedIsNotAskedAgainAndTheFileIsSharedAllTheSame() {
        val health = FakeHealth()
        val answer = Answer(emptySet())
        val r = start(health, answer)
        repeat(2) {
            var result: ShareResult? = null
            finish(r.flow.share(ShareFormat.GPX, { SportFixtures.outing(OutingShare.dir(r.activity)) }) { result = it })
            assertEquals(ShareResult(0, true), result)
            val send = sent(r)
            assertEquals("application/gpx+xml", send.type)

            @Suppress("DEPRECATION")
            val uri = send.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)!!
            assertTrue(uri.path!!.endsWith(".gpx"))
        }
        // Asked at the first share only; nothing written, no heart rate read.
        assertEquals(1, answer.asked.size)
        assertTrue(health.written.isEmpty())
        assertTrue(health.heartAsked.isEmpty())
        // Allowed later in Health Connect's settings: written at the next share, not asked.
        health.have = setOf(HealthPermissions.SESSION, HealthPermissions.ROUTE)
        var result: ShareResult? = null
        finish(r.flow.share(ShareFormat.FIT, { SportFixtures.outing(OutingShare.dir(r.activity)) }) { result = it })
        assertEquals(ShareResult(1, true), result)
        assertEquals(1, answer.asked.size)
        assertTrue(health.heartAsked.isEmpty())
    }

    @Test
    fun withoutHealthConnectOnlyTheShare() {
        val health = FakeHealth(present = false)
        val answer = Answer(HealthPermissions.ALL)
        val r = start(health, answer)
        var result: ShareResult? = null
        finish(r.flow.share(ShareFormat.FIT, { SportFixtures.outing(OutingShare.dir(r.activity)) }) { result = it })
        assertEquals(ShareResult(0, true), result)
        assertTrue(answer.asked.isEmpty())
        assertEquals(Intent.ACTION_SEND, sent(r).action)
        // Nothing to export (no trace): no menu.
        finish(r.flow.share(ShareFormat.FIT, { null }) { result = it })
        assertEquals(ShareResult(0, false), result)
        assertFalse(shadowOf(r.activity).nextStartedActivity != null)
    }

    @Test
    fun buttonsAndRationale() {
        val formats = ArrayList<ShareFormat>()
        rule.setContent {
            MaterialTheme(colorScheme = lightColorScheme()) {
                Surface {
                    Column {
                        ShareOuting(onShare = { formats += it })
                        HealthRationale(onClose = {})
                    }
                }
            }
        }
        rule.onRoot().captureRoboImage("src/test/screenshots/sport_share.png")
        rule.onNodeWithTag(SportTags.SHARE_FIT).performClick()
        rule.onNodeWithTag(SportTags.SHARE_GPX).performClick()
        assertEquals(listOf(ShareFormat.FIT, ShareFormat.GPX), formats)
    }
}
