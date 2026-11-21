package fr.skimap.voicecmd

import java.io.InputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.abs

/**
 * Level of a question before Parakeet (VO5 #87): peak brought to half scale, the gain
 * capped at [MAX_GAIN] so that near silence stays near silence. A quiet speaker (headset
 * microphone far from the mouth) was transcribed worse: +4 points on the French test
 * voices, the quietest one (`marius`, peak 0.04) most.
 */
object Level {
    const val TARGET_PEAK = 0.5f
    const val MAX_GAIN = 100f

    fun normalized(samples: FloatArray): FloatArray {
        val peak = samples.maxOfOrNull { abs(it) } ?: return samples
        val gain = if (peak > 0f) minOf(TARGET_PEAK / peak, MAX_GAIN) else 1f
        return FloatArray(samples.size) { samples[it] * gain }
    }
}

/**
 * What Parakeet v3 hears before the question (VO5 #87). Parakeet guesses the language
 * from the audio: a short French question (« Temps restant ? », « Ramène-moi ») came out in
 * another language (« Tämpsäistä », « Him and more »), and biasing towards the questions
 * did not cure it. A greeting in the phone's language goes first (« Bonjour. », the
 * guidance voice of the packs), then half a second of silence, then the question; the words timed before the middle of that silence are the
 * greeting's and are dropped. Measured on the phone's path (VoiceCommandAudioTest, French
 * questions of the held-out set, with the [Level] and French hotwords): good commands
 * 58.4 → 72.1 % clean, 32.5 → 51.3 % in wind at 0 dB; `marius`, another speaker than the
 * greeting, 43.8 → 60.7 % clean. A quarter of a second of silence lost 5 points.
 *
 * [greeting] null (English): half a second of silence then the question as heard (Parakeet
 * drops or garbles speech that starts abruptly), nothing dropped: the
 * path measured on 15/11.
 */
class GreetingContext(
    private val greeting: FloatArray?,
) {
    /** Words timed before this (seconds from the start of the stream) are the greeting's. */
    val cutSeconds: Float =
        if (greeting == null) 0f else (LEAD + greeting.size + GAP / 2).toFloat() / Endpointer.RATE

    /**
     * Lead silence, greeting, gap, then the question at its [Level]; without a greeting,
     * lead silence then the question as heard (in English the level lost 8 points in wind
     * at 0 dB: 93.7 → 85.2 %).
     */
    fun stream(question: FloatArray): FloatArray {
        val q = if (greeting == null) question else Level.normalized(question)
        val head = if (greeting == null) LEAD else LEAD + greeting.size + GAP
        val out = FloatArray(head + q.size)
        greeting?.copyInto(out, LEAD)
        q.copyInto(out, head)
        return out
    }

    companion object {
        /** Half a second. */
        const val LEAD = Endpointer.RATE / 2

        /** Half a second. */
        const val GAP = Endpointer.RATE / 2

        /** The text of the tokens timed at or after [cut] seconds. */
        fun after(
            tokens: Array<String>,
            timestamps: FloatArray,
            cut: Float,
        ): String =
            tokens.indices
                .filter { it < timestamps.size && timestamps[it] >= cut }
                .joinToString("") { tokens[it] }
                .trim()

        /**
         * The greeting shipped for [lang] (`greeting-fr.wav`, a synthesised « Bonjour. »): French only.
         * In English « Hello. » first lost 7 points on the clean questions (96.5 → 90.1 %)
         * without gaining in the wind: English questions keep no greeting.
         */
        fun greeting(lang: CommandLang): FloatArray? {
            if (lang != CommandLang.FR) return null
            val s = GreetingContext::class.java.getResourceAsStream("greeting-fr.wav") ?: error("missing greeting-fr.wav")
            return s.use(::readWav)
        }

        /** Samples of a canonical 16 kHz mono 16-bit WAV (44-byte header, `write_wav` of skimap-voice). */
        fun readWav(input: InputStream): FloatArray {
            val b = ByteBuffer.wrap(input.readBytes()).order(ByteOrder.LITTLE_ENDIAN)
            check(b.getShort(FORMAT_CHANNELS).toInt() == 1) { "not mono" }
            check(b.getInt(FORMAT_RATE) == Endpointer.RATE) { "not 16 kHz" }
            check(b.getShort(FORMAT_BITS).toInt() == BITS) { "not 16-bit" }
            val n = (b.capacity() - HEADER) / 2
            return FloatArray(n) { b.getShort(HEADER + 2 * it) / FULL_SCALE }
        }

        private const val HEADER = 44
        private const val FORMAT_CHANNELS = 22
        private const val FORMAT_RATE = 24
        private const val FORMAT_BITS = 34
        private const val BITS = 16
        private const val FULL_SCALE = 32768f
    }
}
