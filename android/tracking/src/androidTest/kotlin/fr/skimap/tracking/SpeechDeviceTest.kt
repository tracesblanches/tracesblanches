package fr.skimap.tracking

import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * The phone's own speech synthesis on a device (an emulator of the developer, never a
 * phone in use): an announcement of the core said to its end by [TextToSpeech] through
 * [Announcer], in French and in English, and the voice checked offline. With the
 * instrumentation argument `skimap.speech.wav=1`, the announcements are also synthesised
 * to WAV files in the app's cache (`speech/`), for the measure of the gain of the
 * [android.media.audiofx.LoudnessEnhancer] (`android/docs/volume-annonces.md`).
 */
@RunWith(AndroidJUnit4::class)
class SpeechDeviceTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    private fun sayOn(
        lang: String,
        text: String,
    ): Pair<Announcer, Boolean> {
        val a = Announcer(context, Announcer.localeOf(lang))
        val done = CountDownLatch(1)
        val ok = Collections.synchronizedList(ArrayList<Boolean>())
        a.onDone = { _, said ->
            ok += said
            done.countDown()
        }
        a.say(Utterance(text, safety = true, kinds = listOf("fog")))
        assertTrue("$lang: no end of the announcement", done.await(WAIT_S, TimeUnit.SECONDS))
        Log.i(TAG, "speech $lang: « $text » said ${ok.single()}, voice ${a.voiceStatus}, session ${a.ttsSession}")
        return a to ok.single()
    }

    @Test
    fun anAnnouncementIsSaidByTheSpeechSynthesis() {
        for ((lang, text) in SAMPLES.map { it.first to it.second.first() }) {
            val (a, ok) = sayOn(lang, text)
            try {
                assumeTrue("$lang: no offline voice on this device", a.voiceStatus == VoiceStatus.OFFLINE)
                assertTrue("$lang: not said", ok)
                assertEquals(1, a.spokenByTts)
                assertTrue(a.ttsSession > 0)
            } finally {
                a.close()
            }
        }
    }

    /** The announcements synthesised to WAV (`skimap.speech.wav=1`), as the voice of the device says them. */
    @Test
    fun announcementsToWav() {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("skimap.speech.wav") == "1")
        val out = File(context.cacheDir, "speech").apply { mkdirs() }
        for ((lang, texts) in SAMPLES) {
            val ready = CountDownLatch(1)
            var status = TextToSpeech.ERROR
            val tts =
                TextToSpeech(context) {
                    status = it
                    ready.countDown()
                }
            assertTrue(ready.await(WAIT_S, TimeUnit.SECONDS) && status == TextToSpeech.SUCCESS)
            tts.language = Announcer.localeOf(lang)
            val left = CountDownLatch(texts.size)
            tts.setOnUtteranceProgressListener(
                object : UtteranceProgressListener() {
                    override fun onStart(id: String?) = Unit

                    override fun onDone(id: String?) = left.countDown()

                    @Deprecated("Deprecated in Java")
                    override fun onError(id: String?) = left.countDown()
                },
            )
            texts.forEachIndexed { i, t ->
                tts.synthesizeToFile(t, null, File(out, "$lang-$i.wav"), "$lang-$i")
            }
            assertTrue(left.await(WAIT_S * texts.size, TimeUnit.SECONDS))
            Log.i(TAG, "speech $lang: ${texts.size} WAV in ${out.absolutePath} (${tts.voice?.name})")
            tts.shutdown()
        }
    }

    private companion object {
        const val TAG = "skimap"
        const val WAIT_S = 60L

        /** Announcements of the phrase bank, as the core says them. */
        val SAMPLES =
            listOf(
                "fr" to
                    listOf(
                        "Brouillard annoncé. Restez sur l'itinéraire.",
                        "Dans deux cents mètres, tournez à gauche. Greffier, piste bleue.",
                        "Il reste quatre kilomètres. Environ une heure.",
                        "Vous avez quitté l'itinéraire. À trois cents mètres.",
                        "Faites demi-tour maintenant pour rentrer avant la nuit.",
                        "Vous arrivez à destination.",
                    ),
                "en" to
                    listOf(
                        "Fog forecast. Stay on the route.",
                        "In two hundred metres, turn left. Greffier, blue trail.",
                        "Four kilometres to go. About one hour.",
                        "You have left the route. Three hundred metres away.",
                        "Turn back now to be back before dark.",
                        "You have arrived at your destination.",
                    ),
            )
    }
}
