package fr.skimap.voicecmd

import org.junit.Assert.assertArrayEquals
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File

/**
 * A question is heard the same whatever came before it (VO5 #87): Silero VAD keeps a state
 * from frame to frame, so engines kept loaded across questions (the bulk measurement of
 * [VoiceCommandAudioTest]) heard each one through the end of the previous one, and the
 * rates depended on the order of the files. Needs the model in `~/.cache/skimap/voicecmd/`
 * and the spoken questions; skipped otherwise.
 */
class VadResetTest {
    private val root =
        File(System.getProperty("skimap.voicecmd.dir") ?: "${System.getProperty("user.home")}/.cache/skimap/voicecmd")

    private class Recording(
        private val vad: VoiceActivity,
    ) : VoiceActivity by vad {
        val heard = mutableListOf<Float>()

        override fun probability(frame: FloatArray) = vad.probability(frame).also(heard::add)
    }

    private val silent =
        object : Transcriber {
            override fun transcribe(samples: FloatArray) = ""

            override fun close() = Unit
        }

    private fun wav(name: String) = GreetingContext.readWav(File(root, "audio/$name").inputStream())

    private fun probabilities(
        vc: VoiceCommands,
        engines: Engines,
        rec: Recording,
        samples: FloatArray,
    ): FloatArray {
        rec.heard.clear()
        vc.recognize(engines, SamplesSource(samples)::next)
        return rec.heard.toFloatArray()
    }

    @Test
    fun aQuestionIsHeardTheSameAfterAnotherOne() {
        val store = ModelStore(root)
        assumeTrue("model in ${store.dir}", store.installed)
        val first = "snr+0/001-marius.wav"
        val second = "clean/000-estelle.wav"
        assumeTrue("audio in $root", File(root, "audio/$first").isFile && File(root, "audio/$second").isFile)
        val vc = VoiceCommands({ error("not loaded") }, { t, l -> Heard(t, null, l) }, {}, { CommandLang.FR })
        val fresh =
            Recording(SileroVad(store)).let { rec ->
                Engines(rec, silent).use { probabilities(vc, it, rec, wav(second)) }
            }
        val after =
            Recording(SileroVad(store)).let { rec ->
                Engines(rec, silent).use {
                    probabilities(vc, it, rec, wav(first))
                    probabilities(vc, it, rec, wav(second))
                }
            }
        assertArrayEquals(fresh, after, 0f)
    }
}
