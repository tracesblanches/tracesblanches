package fr.skimap.tracking

import android.app.Application
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.AudioTrack
import android.os.Looper
import android.speech.tts.TextToSpeech
import androidx.test.core.app.ApplicationProvider
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.shadows.ShadowLog
import org.robolectric.shadows.ShadowTextToSpeech
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** The voice's stream, focus and volume, and the silence between a turn's sound and its words (#179). */
@RunWith(RobolectricTestRunner::class)
class AnnouncerTest {
    private val app = ApplicationProvider.getApplicationContext<Application>()
    private val audioManager = app.getSystemService(AudioManager::class.java)
    private val opened = ArrayList<Announcer>()

    @After
    fun close() {
        opened.forEach { it.close() }
        ShadowTextToSpeech.reset()
    }

    private fun announcer(decoder: (String) -> Pcm): Announcer = Announcer(app, decoder = decoder).also { opened += it }

    private fun utterance(n: Int) =
        Utterance(
            (1..n).joinToString(" ") { "Clip $it." },
            (1..n).map { Clip("Clip $it.", "/pack/voice/clips/$it.opus", if (it < n) PAUSE_MS else 0) },
            allClips = true,
            safety = false,
            kinds = listOf("turn"),
        )

    /** Waits for everything queued, the main looper idled meanwhile (the track's callbacks). */
    private fun drain(a: Announcer) {
        val done = CountDownLatch(1)
        a.afterQueue { done.countDown() }
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(WAIT_S)
        while (!done.await(IDLE_MS, TimeUnit.MILLISECONDS)) {
            shadowOf(Looper.getMainLooper()).idle()
            check(System.nanoTime() < end) {
                "voice queue stuck: " + ShadowLog.getLogsForTag("skimap").joinToString { it.msg }
            }
        }
    }

    private fun assertGuidance(a: AudioAttributes) {
        assertEquals(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE, a.usage)
        assertEquals(AudioAttributes.CONTENT_TYPE_SPEECH, a.contentType)
    }

    @Test
    fun clipsPlayOnTheGuidanceStreamAtFullVolumeWithADuckingFocus() {
        val clip = ShortArray(RATE / 10) { i -> (i % 100).toShort() }
        val a = announcer { Pcm(clip, RATE, 1) }
        val played = Collections.synchronizedList(ArrayList<Pcm>())
        a.output = PcmOutput { played += it }
        a.say(utterance(2))
        drain(a)
        assertEquals(1, a.played)
        // One buffer for the whole announcement, the samples as decoded (no attenuation).
        assertEquals(1, played.size)
        val expected = clip + ShortArray(RATE * PAUSE_MS / 1000) + clip
        assertTrue(expected.contentEquals(played[0].samples))
        // The platform track: guidance stream, speech, full player volume (the clips carry
        // the level, -14 LUFS, and the phone's volume the rest).
        val t = a.track(played[0])
        assertGuidance(t.audioAttributes)
        assertEquals(1f, AudioTrack.getMaxVolume())
        assertEquals(1f, playerVolume(t))
        t.release()
        // The transient focus that ducks the other sounds, then given back.
        val req = shadowOf(audioManager).lastAudioFocusRequest
        assertNotNull(req)
        assertEquals(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK, req.audioFocusRequest.focusGain)
        assertGuidance(req.audioFocusRequest.audioAttributes)
        assertSame(a.focusRequest, shadowOf(audioManager).lastAbandonedAudioFocusRequest)
    }

    /** The left and right volumes the player keeps (PlayerBase), 1 when not attenuated. */
    private fun playerVolume(t: AudioTrack): Float {
        val base = Class.forName("android.media.PlayerBase")
        val left = base.getDeclaredField("mLeftVolume").apply { isAccessible = true }.getFloat(t)
        val right = base.getDeclaredField("mRightVolume").apply { isAccessible = true }.getFloat(t)
        assertEquals(left, right)
        return left
    }

    @Test
    fun theAndroidVoiceReadsOnTheGuidanceStreamAtFullVolume() {
        val a = announcer { error("no clip") }
        a.say(Utterance("Il reste trois kilomètres.", emptyList(), allClips = false, safety = false, kinds = listOf("remain")))
        // The engine binds, then reads; its callbacks come on the main looper.
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
        assertEquals("Il reste trois kilomètres.", shadowOf(tts).lastSpokenText)
        assertEquals(1f, ShadowTextToSpeech.getLastParams()!!.getFloat(TextToSpeech.Engine.KEY_PARAM_VOLUME), 0f)
        // Attributes given to the engine (its own parameters, read back).
        val params =
            TextToSpeech::class.java
                .getDeclaredField("mParams")
                .apply { isAccessible = true }
                .get(tts) as android.os.Bundle
        @Suppress("DEPRECATION")
        assertGuidance(params.getParcelable<AudioAttributes>("audioAttributes")!!)
        assertEquals(1, a.spokenByTts)
        val req = shadowOf(audioManager).lastAudioFocusRequest
        assertEquals(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK, req.audioFocusRequest.focusGain)
        assertSame(a.focusRequest, shadowOf(audioManager).lastAbandonedAudioFocusRequest)
    }

    /** One sound played: start and end, ms since the test began, and its samples. */
    private class Played(
        val startMs: Double,
        val endMs: Double,
        val samples: Int,
    )

    /**
     * A turn's sound then its words, clips decoded with the platform's cost measured on the
     * phone (#179: 0.35 to 0.45 s for three clips one after the other, 1.1 s for seven
     * under load). The clips are decoded while the sound plays: the words follow it.
     */
    private fun gapAfterTheTurnSound(
        clips: Int,
        decodeMs: Long,
    ): Double {
        val t0 = System.nanoTime()
        val now = { (System.nanoTime() - t0) / 1e6 }
        val played = Collections.synchronizedList(ArrayList<Played>())
        val abandonedBefore = shadowOf(audioManager).lastAbandonedAudioFocusRequest
        val abandonedWhilePlaying = Collections.synchronizedList(ArrayList<Any?>())
        val a =
            announcer {
                Thread.sleep(decodeMs)
                Pcm(ShortArray(RATE / 2), RATE, 1)
            }
        a.output =
            PcmOutput { pcm ->
                abandonedWhilePlaying += shadowOf(audioManager).lastAbandonedAudioFocusRequest
                val start = now()
                Thread.sleep(pcm.samples.size / pcm.channels * 1000L / pcm.rate)
                played += Played(start, now(), pcm.samples.size)
            }
        a.earcon(Earcon.LEFT, EarconSettings.DEFAULT_VOLUME)
        a.say(utterance(clips))
        drain(a)
        assertEquals(2, played.size)
        assertEquals(1, a.earcons)
        assertEquals(1, a.played)
        // The focus is held from the sound to the words (the music does not come back between).
        assertTrue(abandonedWhilePlaying.all { it === abandonedBefore })
        assertSame(a.focusRequest, shadowOf(audioManager).lastAbandonedAudioFocusRequest)
        val gap = played[1].startMs - played[0].endMs
        println("#179 gap turn sound -> words, $clips clips decoded in $decodeMs ms each: ${"%.1f".format(gap)} ms")
        return gap
    }

    @Test
    fun theWordsFollowTheTurnSoundWithoutSilence() {
        // Three clips at 130 ms each: 390 ms of silence when decoded after the sound.
        assertTrue(gapAfterTheTurnSound(3, DECODE_MS) < MAX_GAP_MS)
        // Seven clips under load (157 ms each, 1.1 s one after the other).
        assertTrue(gapAfterTheTurnSound(7, LOADED_DECODE_MS) < MAX_GAP_MS)
    }

    @Test
    fun aClipThatDoesNotDecodeFallsBackToTheAndroidVoice() {
        val a = announcer { throw IllegalStateException("codec") }
        val said = Collections.synchronizedList(ArrayList<Boolean>())
        a.onDone = { _, ok -> said += ok }
        a.output = PcmOutput { error("nothing to play") }
        a.say(utterance(2))
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(WAIT_S)
        while (said.isEmpty()) {
            ShadowTextToSpeech.getLastTextToSpeechInstance()?.let { t ->
                shadowOf(t).onInitListener?.onInit(TextToSpeech.SUCCESS)
            }
            shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(IDLE_MS)
            check(System.nanoTime() < end) { "no fallback" }
        }
        assertEquals(0, a.played)
        assertEquals("Clip 1. Clip 2.", shadowOf(ShadowTextToSpeech.getLastTextToSpeechInstance()).lastSpokenText)
    }

    private companion object {
        const val RATE = 48_000
        const val PAUSE_MS = 150
        const val WAIT_S = 20L
        const val IDLE_MS = 10L
        const val DECODE_MS = 130L
        const val LOADED_DECODE_MS = 157L
        const val MAX_GAP_MS = 40.0
    }
}
