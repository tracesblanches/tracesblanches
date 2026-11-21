package fr.skimap.tracking

import android.app.Application
import android.media.AudioAttributes
import android.media.AudioManager
import android.os.Looper
import android.speech.tts.TextToSpeech
import android.speech.tts.Voice
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
import java.util.Locale
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** The announcements said by the phone's speech synthesis: stream, focus, gain, queue and voice check. */
@RunWith(RobolectricTestRunner::class)
class AnnouncerTest {
    private val app = ApplicationProvider.getApplicationContext<Application>()
    private val audioManager = app.getSystemService(AudioManager::class.java)
    private val opened = ArrayList<Announcer>()
    private val gains = Collections.synchronizedList(ArrayList<Pair<Int, Int>>())

    @After
    fun close() {
        opened.forEach { it.close() }
        ShadowTextToSpeech.reset()
    }

    private fun announcer(locale: Locale = Locale.FRANCE): Announcer =
        Announcer(app, locale, enhancer = { session, mb ->
            gains += session to mb
            AutoCloseable { }
        }).also { opened += it }

    private fun assertGuidance(a: AudioAttributes) {
        assertEquals(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE, a.usage)
        assertEquals(AudioAttributes.CONTENT_TYPE_SPEECH, a.contentType)
    }

    /** The engine of the test: started with [status], its utterances finished by [finish]. */
    private fun engine(status: Int = TextToSpeech.SUCCESS): TextToSpeech {
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(WAIT_S)
        var tts: TextToSpeech? = null
        while (tts == null) {
            tts = ShadowTextToSpeech.getLastTextToSpeechInstance()
            shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(IDLE_MS)
            check(System.nanoTime() < end) { "no engine" }
        }
        shadowOf(tts).onInitListener?.onInit(status)
        return tts
    }

    /**
     * Waits until the engine was given [n] texts, none of them finished: the test engine
     * says them (its progress listener) only when the main looper runs ([finishAll]).
     */
    private fun spoken(
        tts: TextToSpeech,
        n: Int,
    ): List<String> {
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(WAIT_S)
        while (shadowOf(tts).spokenTextList.size < n) {
            Thread.sleep(IDLE_MS)
            check(System.nanoTime() < end) {
                "said ${shadowOf(tts).spokenTextList}: " + ShadowLog.getLogsForTag("skimap").joinToString { it.msg }
            }
        }
        return shadowOf(tts).spokenTextList
    }

    /** The test engine says everything given to it (start, then done, on the main looper). */
    private fun finishAll() {
        shadowOf(Looper.getMainLooper()).idle()
    }

    /** Waits for everything queued. */
    private fun drain(a: Announcer) {
        val done = CountDownLatch(1)
        a.afterQueue { done.countDown() }
        check(done.await(WAIT_S, TimeUnit.SECONDS)) { "voice queue stuck" }
    }

    @Test
    fun theVoiceReadsOnTheGuidanceStreamWithItsGainAndADuckingFocus() {
        val a = announcer()
        a.say(Utterance("Il reste trois kilomètres.", safety = false, kinds = listOf("remain")))
        val tts = engine()
        assertEquals(listOf("Il reste trois kilomètres."), spoken(tts, 1))
        assertEquals(Locale.FRANCE, shadowOf(tts).currentLanguage)
        val params = ShadowTextToSpeech.getLastParams()!!
        assertEquals(1f, params.getFloat(TextToSpeech.Engine.KEY_PARAM_VOLUME), 0f)
        // Its own session, with the loudness enhancer at the measured gain.
        val session = params.getInt(TextToSpeech.Engine.KEY_PARAM_SESSION_ID)
        assertTrue(session > 0)
        assertEquals(session, a.ttsSession)
        assertEquals(listOf(session to Announcer.TTS_GAIN_MB), gains)
        // Attributes given to the engine (its own parameters, read back).
        val engineParams =
            TextToSpeech::class.java
                .getDeclaredField("mParams")
                .apply { isAccessible = true }
                .get(tts) as android.os.Bundle
        @Suppress("DEPRECATION")
        assertGuidance(engineParams.getParcelable<AudioAttributes>("audioAttributes")!!)
        val req = shadowOf(audioManager).lastAudioFocusRequest
        assertNotNull(req)
        assertEquals(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK, req.audioFocusRequest.focusGain)
        assertGuidance(req.audioFocusRequest.audioAttributes)
        finishAll()
        drain(a)
        assertEquals(1, a.spokenByTts)
        assertSame(a.focusRequest, shadowOf(audioManager).lastAbandonedAudioFocusRequest)
    }

    /**
     * Announcements that come together go to the engine's queue at once (no wait between
     * them), the focus held until the last one is said; a turn's sound plays after the words
     * queued before it, and the words after it follow it.
     */
    @Test
    fun announcementsChainInTheEnginesQueueAndSoundsWaitTheirTurn() {
        val a = announcer()
        val order = Collections.synchronizedList(ArrayList<String>())
        // The sound plays until the test lets it end: what comes after it waits.
        val playing = CountDownLatch(1)
        val release = CountDownLatch(1)
        a.output =
            PcmOutput {
                order += "earcon"
                playing.countDown()
                release.await(WAIT_S, TimeUnit.SECONDS)
            }
        val done = Collections.synchronizedList(ArrayList<Boolean>())
        a.onDone = { u, ok ->
            order += "done ${u.text}"
            done += ok
        }
        a.say(Utterance("Un.", safety = false, kinds = listOf("remain")))
        a.say(Utterance("Deux.", safety = true, kinds = listOf("fog")))
        a.earcon(Earcon.LEFT, EarconSettings.DEFAULT_VOLUME)
        a.say(Utterance("Trois.", safety = false, kinds = listOf("turn")))
        val tts = engine()
        // Both texts queued before the sound are in the engine before the first is over.
        assertEquals(listOf("Un.", "Deux."), spoken(tts, 2))
        assertTrue(order.isEmpty())
        finishAll()
        // The sound once the words before it were said; the words after it wait for it.
        assertTrue(playing.await(WAIT_S, TimeUnit.SECONDS))
        assertEquals(listOf("done Un.", "done Deux.", "earcon"), order.toList())
        assertEquals(2, shadowOf(tts).spokenTextList.size)
        // The focus is still held: the queue is not over.
        assertTrue(shadowOf(audioManager).lastAbandonedAudioFocusRequest !== a.focusRequest)
        release.countDown()
        assertEquals(listOf("Un.", "Deux.", "Trois."), spoken(tts, 3))
        finishAll()
        drain(a)
        assertEquals(listOf(true, true, true), done.toList())
        assertEquals(1, a.earcons)
        assertSame(a.focusRequest, shadowOf(audioManager).lastAbandonedAudioFocusRequest)
    }

    /**
     * The engine holds the announcement it says and the next one; the others are checked
     * when their turn comes (#183): one past its position, or the same text again, is not
     * given to the engine.
     */
    @Test
    fun staleAnnouncementsAreNotGivenToTheEngine() {
        val a = announcer()
        val start = SpokenAt(1_000_000L, 6.72, 44.85)
        var here = start
        a.freshness = Freshness({ here }, { 0L })
        val done = Collections.synchronizedList(ArrayList<Pair<String, Boolean>>())
        a.onDone = { u, ok -> done += u.text to ok }
        a.say(Utterance("Un.", safety = false, kinds = listOf("remain"), at = start))
        a.say(Utterance("Deux.", safety = false, kinds = listOf("turn"), at = start))
        a.say(Utterance("Un.", safety = false, kinds = listOf("remain"), at = start))
        a.say(Utterance("Trois.", safety = false, kinds = listOf("turn"), at = start))
        a.say(Utterance("Brouillard annoncé.", safety = true, kinds = listOf("fog"), at = start))
        val tts = engine()
        // Two in the engine, the rest waits for its turn.
        assertEquals(listOf("Un.", "Deux."), spoken(tts, 2))
        Thread.sleep(SETTLE_MS)
        assertEquals(2, shadowOf(tts).spokenTextList.size)
        // Meanwhile the skier went 200 m on.
        here = SpokenAt(start.tMs + 20_000, start.lon, start.lat + 0.0018)
        finishAll()
        // « Un. » again: said a moment ago; « Trois. »: past its position; the safety one: said.
        assertEquals(listOf("Un.", "Deux.", "Brouillard annoncé."), spoken(tts, 3))
        finishAll()
        drain(a)
        assertEquals(listOf("Un.", "Deux.", "Brouillard annoncé."), shadowOf(tts).spokenTextList.toList())
        assertEquals(2, a.stale)
        val expected = listOf("Un." to true, "Deux." to true, "Un." to false, "Trois." to false, "Brouillard annoncé." to true)
        assertEquals(expected.sortedBy { it.toString() }, done.sortedBy { it.toString() })
    }

    @Test
    fun aVoiceOfTheLanguageInstalledOfflineIsFound() {
        val a = announcer(Locale.UK)
        val seen = Collections.synchronizedList(ArrayList<VoiceStatus>())
        a.onVoiceStatus = { seen += it }
        ShadowTextToSpeech.addLanguageAvailability(Locale.UK)
        ShadowTextToSpeech.addVoice(Voice("en-gb-x-local", Locale.UK, Voice.QUALITY_HIGH, Voice.LATENCY_NORMAL, false, emptySet()))
        a.warmUp()
        engine()
        drain(a)
        assertEquals(VoiceStatus.OFFLINE, a.voiceStatus)
        assertEquals(listOf(VoiceStatus.OFFLINE), seen.toList())
    }

    @Test
    fun aVoiceThatNeedsTheNetworkOrIsNotInstalledIsNone() {
        val a = announcer(Locale.FRANCE)
        ShadowTextToSpeech.addLanguageAvailability(Locale.FRANCE)
        ShadowTextToSpeech.addVoice(Voice("fr-fr-x-network", Locale.FRANCE, Voice.QUALITY_HIGH, Voice.LATENCY_NORMAL, true, emptySet()))
        ShadowTextToSpeech.addVoice(
            Voice(
                "fr-fr-x-local",
                Locale.FRANCE,
                Voice.QUALITY_HIGH,
                Voice.LATENCY_NORMAL,
                false,
                setOf(TextToSpeech.Engine.KEY_FEATURE_NOT_INSTALLED),
            ),
        )
        a.warmUp()
        engine()
        drain(a)
        assertEquals(VoiceStatus.NONE, a.voiceStatus)
    }

    @Test
    fun withoutAnEngineNothingIsSaidAndTheVoiceIsNone() {
        val a = announcer()
        val done = Collections.synchronizedList(ArrayList<Boolean>())
        a.onDone = { _, ok -> done += ok }
        a.say(Utterance("Brouillard annoncé.", safety = true, kinds = listOf("fog")))
        engine(TextToSpeech.ERROR)
        drain(a)
        assertEquals(listOf(false), done.toList())
        assertEquals(VoiceStatus.NONE, a.voiceStatus)
        assertSame(a.focusRequest, shadowOf(audioManager).lastAbandonedAudioFocusRequest)
    }

    private companion object {
        const val WAIT_S = 20L
        const val IDLE_MS = 10L
        const val SETTLE_MS = 200L
    }
}
