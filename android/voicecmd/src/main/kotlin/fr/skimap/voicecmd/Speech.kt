package fr.skimap.voicecmd

import com.k2fsa.sherpa.onnx.OfflineModelConfig
import com.k2fsa.sherpa.onnx.OfflineRecognizer
import com.k2fsa.sherpa.onnx.OfflineRecognizerConfig
import com.k2fsa.sherpa.onnx.OfflineTransducerModelConfig
import com.k2fsa.sherpa.onnx.SileroVadModelConfig
import com.k2fsa.sherpa.onnx.Vad
import com.k2fsa.sherpa.onnx.VadModelConfig

/** Speech probability of one [Endpointer.FRAME]-sample frame at 16 kHz. */
interface VoiceActivity : AutoCloseable {
    fun probability(frame: FloatArray): Float

    /** Forgets the frames heard so far: the next question is heard as by fresh engines. */
    fun reset()
}

/** A whole question → its text. */
interface Transcriber : AutoCloseable {
    fun transcribe(samples: FloatArray): String
}

/** The two engines of one command, loaded together and released together. */
class Engines(
    val vad: VoiceActivity,
    val stt: Transcriber,
) : AutoCloseable {
    override fun close() {
        vad.close()
        stt.close()
    }
}

/**
 * Silero VAD v6.2.3 through sherpa-onnx's `Vad.compute` (the raw probability; the end of
 * the command is [Endpointer]'s), as speak does: Silero v5/v6 take 64 samples of context
 * before each 512-sample window.
 */
class SileroVad(
    store: ModelStore,
) : VoiceActivity {
    private val vad =
        Vad(
            null,
            VadModelConfig(
                sileroVadModelConfig =
                    SileroVadModelConfig(
                        model = store.file(ModelSpec.VAD).path,
                        threshold = 0.5f,
                        windowSize = Endpointer.FRAME,
                    ),
                sampleRate = Endpointer.RATE,
                numThreads = 1,
                provider = "cpu",
            ),
        )
    private val window = FloatArray(CONTEXT + Endpointer.FRAME)

    override fun probability(frame: FloatArray): Float {
        require(frame.size == Endpointer.FRAME)
        System.arraycopy(frame, 0, window, CONTEXT, frame.size)
        val p = vad.compute(window)
        System.arraycopy(frame, frame.size - CONTEXT, window, 0, CONTEXT)
        return p
    }

    override fun reset() {
        vad.reset()
        window.fill(0f)
    }

    override fun close() = vad.release()

    private companion object {
        const val CONTEXT = 64
    }
}

/**
 * Parakeet-TDT-0.6B-v3 int8 through sherpa-onnx's OfflineRecognizer. With [hotwords] (the
 * questions of the closed list in the phone's language, as measured with the greeting:
 * 63.6 % of French commands without them, 71.4 % with them) and the BPE vocabulary of the store: modified beam search biased
 * towards them (contextual biasing, score 2; 4 lost 6 points), else greedy
 * search. [greeting]: the phone's language said before the question ([GreetingContext]),
 * the question at its [Level].
 */
class ParakeetTranscriber(
    store: ModelStore,
    hotwords: List<String> = emptyList(),
    greeting: FloatArray? = null,
    threads: Int = 4,
) : Transcriber {
    private val context = GreetingContext(greeting)
    private val bpe = store.file(ModelSpec.BPE_VOCAB)
    private val biased = hotwords.isNotEmpty() && bpe.isFile
    private val hot =
        hotwords
            .map { it.trim() }
            .filter { it.isNotEmpty() && '/' !in it }
            .distinct()
            .joinToString("/")
    private val rec =
        OfflineRecognizer(
            null,
            OfflineRecognizerConfig(
                modelConfig =
                    OfflineModelConfig(
                        transducer =
                            OfflineTransducerModelConfig(
                                encoder = store.file(ModelSpec.ENCODER).path,
                                decoder = store.file(ModelSpec.DECODER).path,
                                joiner = store.file(ModelSpec.JOINER).path,
                            ),
                        tokens = store.file(ModelSpec.TOKENS).path,
                        modelType = "nemo_transducer",
                        modelingUnit = if (biased) "bpe" else "",
                        bpeVocab = if (biased) bpe.path else "",
                        numThreads = threads,
                        provider = "cpu",
                    ),
                decodingMethod = if (biased) "modified_beam_search" else "greedy_search",
                maxActivePaths = BEAM,
                hotwordsScore = HOTWORDS_SCORE,
            ),
        )

    override fun transcribe(samples: FloatArray): String {
        val s = if (biased) rec.createStream(hot) else rec.createStream()
        try {
            s.acceptWaveform(context.stream(samples), Endpointer.RATE)
            rec.decode(s)
            val r = rec.getResult(s)
            return GreetingContext.after(r.tokens, r.timestamps, context.cutSeconds)
        } finally {
            s.release()
        }
    }

    override fun close() = rec.release()

    private companion object {
        const val BEAM = 4
        const val HOTWORDS_SCORE = 2.0f
    }
}

/** Loads the engines of the model in [store] (2 to 4 s for Parakeet on a Pixel). */
fun interface EngineLoader {
    fun load(): Engines

    companion object {
        /** [lang]: the phone's language, said before each question ([GreetingContext]); null: none. */
        fun sherpa(
            store: ModelStore,
            hotwords: List<String> = emptyList(),
            lang: CommandLang? = null,
        ): EngineLoader =
            EngineLoader {
                check(store.installed) { "speech model not installed in ${store.dir}" }
                val vad = SileroVad(store)
                try {
                    Engines(vad, ParakeetTranscriber(store, hotwords, lang?.let(GreetingContext::greeting)))
                } catch (e: IllegalStateException) {
                    vad.close()
                    throw e
                }
            }
    }
}
