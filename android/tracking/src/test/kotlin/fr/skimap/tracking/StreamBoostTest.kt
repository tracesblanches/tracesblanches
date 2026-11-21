package fr.skimap.tracking

import android.app.Application
import android.app.NotificationManager
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Looper
import android.speech.tts.TextToSpeech
import androidx.test.core.app.ApplicationProvider
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.shadows.AudioDeviceInfoBuilder
import org.robolectric.shadows.ShadowLog
import org.robolectric.shadows.ShadowTextToSpeech
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * The guidance stream raised while an announcement plays and given back exactly (#179),
 * and the loudness enhancer of the Android voice on its own session.
 */
@RunWith(RobolectricTestRunner::class)
class StreamBoostTest {
    private val app = ApplicationProvider.getApplicationContext<Application>()
    private val audio = app.getSystemService(AudioManager::class.java)
    private val opened = ArrayList<Announcer>()
    private var outing = true

    @Before
    fun pixel() {
        // The media stream of a Pixel: 25 steps, at the usual 7 (about 25 %).
        shadowOf(audio).setStreamMaxVolume(MAX)
        audio.setStreamVolume(AudioManager.STREAM_MUSIC, USUAL, 0)
    }

    @After
    fun close() {
        opened.forEach { it.close() }
        ShadowTextToSpeech.reset()
    }

    private fun announcer(
        enhancer: (Int, Int) -> AutoCloseable? = { _, _ -> null },
        onPlay: () -> Unit = {},
    ): Pair<Announcer, MutableList<Int>> {
        val a = Announcer(app, decoder = { Pcm(ShortArray(RATE / 10), RATE, 1) }, outing = { outing }, enhancer = enhancer)
        opened += a
        val volumes = Collections.synchronizedList(ArrayList<Int>())
        a.output =
            PcmOutput {
                volumes += music()
                onPlay()
            }
        return a to volumes
    }

    private fun music() = audio.getStreamVolume(AudioManager.STREAM_MUSIC)

    private fun utterance(text: String = "Tournez à gauche.") =
        Utterance(text, listOf(Clip(text, "/pack/voice/clips/1.opus", 0)), allClips = true, safety = false, kinds = listOf("turn"))

    private fun drain(a: Announcer) {
        val done = CountDownLatch(1)
        a.afterQueue { done.countDown() }
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(WAIT_S)
        while (!done.await(IDLE_MS, TimeUnit.MILLISECONDS)) {
            shadowOf(Looper.getMainLooper()).idle()
            check(System.nanoTime() < end) { "voice queue stuck: " + ShadowLog.getLogsForTag("skimap").joinToString { it.msg } }
        }
    }

    @Test
    fun theStreamIsRaisedToTheFloorDuringTheAnnouncementAndGivenBackExactly() {
        val (a, volumes) = announcer()
        assertEquals(AudioManager.STREAM_MUSIC, a.boost!!.stream)
        a.say(utterance())
        drain(a)
        // 70 % of 25, rounded up: 18 while it plays, then the 7 of before.
        assertEquals(listOf(SPEAKER), volumes)
        assertEquals(USUAL, music())
        assertFalse(a.boost!!.raised)
    }

    @Test
    fun announcementsThatFollowEachOtherAreRaisedOnceAndGiveBackTheFirstVolume() {
        val (a, volumes) = announcer()
        a.earcon(Earcon.LEFT, 1f)
        a.say(utterance())
        a.say(utterance("Puis à droite."))
        drain(a)
        // The turn's sound and both announcements at the floor, under one raise.
        assertEquals(listOf(SPEAKER, SPEAKER, SPEAKER), volumes)
        assertEquals(USUAL, music())
        // A later one starts again from the volume given back, never from the raised one.
        a.say(utterance())
        drain(a)
        assertEquals(SPEAKER, volumes.last())
        assertEquals(USUAL, music())
    }

    @Test
    fun aVolumeTheUserChangesDuringTheAnnouncementIsKept() {
        val (a, _) = announcer(onPlay = { audio.setStreamVolume(AudioManager.STREAM_MUSIC, USER, 0) })
        a.say(utterance())
        drain(a)
        assertEquals(USER, music())
        assertFalse(a.boost!!.raised)
        // Lowered by the user during it, too: kept.
        audio.setStreamVolume(AudioManager.STREAM_MUSIC, USUAL, 0)
        val (b, _) = announcer(onPlay = { audio.setStreamVolume(AudioManager.STREAM_MUSIC, 2, 0) })
        b.say(utterance())
        drain(b)
        assertEquals(2, music())
    }

    @Test
    fun aVolumeLeftRaisedByAKilledAppIsGivenBackAtTheNextStart() {
        val boost = StreamBoost(app, Announcer(app).also { opened += it }.attributes) { true }
        boost.raise()
        assertEquals(SPEAKER, music())
        assertTrue(boost.raised)
        // Killed here: nothing restored. At the next start:
        StreamBoost.recover(app)
        assertEquals(USUAL, music())
        assertFalse(boost.raised)
        // Recovering again changes nothing.
        StreamBoost.recover(app)
        assertEquals(USUAL, music())
    }

    @Test
    fun aKilledAppKeepsAVolumeTheUserChangedSince() {
        val boost = StreamBoost(app, Announcer(app).also { opened += it }.attributes) { true }
        boost.raise()
        audio.setStreamVolume(AudioManager.STREAM_MUSIC, USER, 0)
        StreamBoost.recover(app)
        assertEquals(USER, music())
        assertFalse(boost.raised)
    }

    @Test
    fun theNextOutingGivesBackAVolumeLeftRaisedBeforeAnythingPlays() {
        val boost = StreamBoost(app, Announcer(app).also { opened += it }.attributes) { true }
        boost.raise()
        // The service starts again: its announcer recovers first, then raises from 7.
        val (a, volumes) = announcer()
        a.say(utterance())
        drain(a)
        assertEquals(listOf(SPEAKER), volumes)
        assertEquals(USUAL, music())
    }

    @Test
    fun neverRaisedOutsideAnOutingInDoNotDisturbOrAboveTheFloor() {
        outing = false
        val (a, volumes) = announcer()
        a.say(utterance())
        drain(a)
        assertEquals(listOf(USUAL), volumes)
        // Do not disturb.
        outing = true
        val nm = app.getSystemService(NotificationManager::class.java)
        nm.setInterruptionFilter(NotificationManager.INTERRUPTION_FILTER_PRIORITY)
        a.say(utterance())
        drain(a)
        assertEquals(USUAL, volumes.last())
        nm.setInterruptionFilter(NotificationManager.INTERRUPTION_FILTER_ALL)
        // Already loud enough: untouched.
        audio.setStreamVolume(AudioManager.STREAM_MUSIC, LOUD, 0)
        a.say(utterance())
        drain(a)
        assertEquals(LOUD, volumes.last())
        assertEquals(LOUD, music())
    }

    @Test
    fun withAHeadsetTheFloorIsLower() {
        val headset = AudioDeviceInfoBuilder.newBuilder().setType(AudioDeviceInfo.TYPE_WIRED_HEADSET).build()
        shadowOf(audio).setOutputDevices(listOf(headset))
        val (a, volumes) = announcer()
        a.say(utterance())
        drain(a)
        assertEquals(listOf(HEADSET), volumes)
        assertEquals(USUAL, music())
        // Already above the headset floor: untouched (no raise to the loudspeaker's).
        audio.setStreamVolume(AudioManager.STREAM_MUSIC, HEADSET + 1, 0)
        a.say(utterance())
        drain(a)
        assertEquals(HEADSET + 1, volumes.last())
    }

    @Test
    fun theAndroidVoiceReadsThroughAnEnhancerOnItsOwnSessionAndTheClipsDoNot() {
        val attached = Collections.synchronizedList(ArrayList<Pair<Int, Int>>())
        val released = CountDownLatch(1)
        val (a, _) =
            announcer(enhancer = { s, g ->
                attached += s to g
                AutoCloseable { released.countDown() }
            })
        a.say(utterance())
        drain(a)
        assertTrue("no enhancer on the clips", attached.isEmpty())
        a.say(Utterance("Il reste trois kilomètres.", emptyList(), allClips = false, safety = false, kinds = listOf("remain")))
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(WAIT_S)
        var tts: TextToSpeech? = null
        while (tts == null || shadowOf(tts).lastSpokenText == null) {
            tts = ShadowTextToSpeech.getLastTextToSpeechInstance()
            tts?.let { shadowOf(it).onInitListener?.onInit(TextToSpeech.SUCCESS) }
            shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(IDLE_MS)
            check(System.nanoTime() < end) { "Android voice never spoke" }
        }
        drain(a)
        val session = ShadowTextToSpeech.getLastParams()!!.getInt(TextToSpeech.Engine.KEY_PARAM_SESSION_ID)
        assertTrue(session > 0)
        assertEquals(session, a.ttsSession)
        assertEquals(listOf(session to Announcer.TTS_GAIN_MB), attached.toList())
        assertTrue("released after the reading", released.await(WAIT_S, TimeUnit.SECONDS))
    }

    private companion object {
        const val MAX = 25
        const val USUAL = 7
        const val SPEAKER = 18
        const val HEADSET = 10
        const val USER = 21
        const val LOUD = 20
        const val RATE = 48_000
        const val WAIT_S = 10L
        const val IDLE_MS = 5L
    }
}
