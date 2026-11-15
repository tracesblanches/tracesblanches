package fr.skimap.voicecmd

import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread

/**
 * One voice command, on demand (headset button held, microphone button): the microphone
 * opens at once and its frames are kept while the engines load (Parakeet takes seconds:
 * the skier may speak right after the gesture); the engines then find the end of the
 * question (VAD, [Endpointer]), transcribe it, and are released before the answer
 * (battery: nothing stays loaded between two commands). The transcription goes to the
 * core's classifier and the command to the app's [handler], not understood included
 * (the handler says « je n'ai pas compris » and the questions).
 *
 * [listen] blocks: call it from a background thread. One command at a time: a gesture
 * during a command is ignored.
 */
class VoiceCommands(
    private val loader: EngineLoader,
    private val classifier: CommandClassifier,
    private val handler: CommandHandler,
    private val defaultLang: () -> CommandLang,
    private val now: () -> Long = System::nanoTime,
) {
    /** What one command did, with its timings (logged; measured by the tests). */
    data class Outcome(
        val heard: Heard?,
        val ended: Endpointer.State,
        val loadMs: Long,
        val decodeMs: Long,
        val audioSeconds: Double,
    )

    private val busy = AtomicBoolean(false)

    /** Listens to one question from [source] (closed at the end); null when a command is already running. */
    fun listen(source: FrameSource): Outcome? {
        if (!busy.compareAndSet(false, true)) {
            source.close()
            return null
        }
        try {
            return run(source)
        } finally {
            busy.set(false)
        }
    }

    private fun run(source: FrameSource): Outcome {
        val t0 = now()
        var loadMs = 0L
        val r =
            Capture(source).use { capture ->
                loader.load().use { engines ->
                    loadMs = (now() - t0) / NS_PER_MS
                    recognize(engines, capture::next)
                }
            }
        val heard = r.text?.let { classifier.classify(it, defaultLang()) }
        heard?.let(handler::handle)
        return Outcome(heard, r.ended, loadMs, r.decodeMs, r.audioSeconds)
    }

    /** What the engines heard in one question. */
    data class Recognized(
        val text: String?,
        val ended: Endpointer.State,
        val decodeMs: Long,
        val audioSeconds: Double,
    )

    /**
     * The end of the question found by the VAD in the frames of [next] (null: no more),
     * then its transcription; the engines stay loaded (the bulk measurements of the tests
     * load them once).
     */
    fun recognize(
        engines: Engines,
        next: () -> FloatArray?,
    ): Recognized {
        val ep = Endpointer()
        while (ep.state == Endpointer.State.WAITING || ep.state == Endpointer.State.SPEECH) {
            val f = next() ?: break
            ep.push(f, engines.vad.probability(f))
        }
        if (ep.state == Endpointer.State.NOTHING || ep.state == Endpointer.State.WAITING) {
            return Recognized(null, ep.state, 0, 0.0)
        }
        val samples = ep.samples()
        val t1 = now()
        val text = engines.stt.transcribe(samples)
        return Recognized(text, ep.state, (now() - t1) / NS_PER_MS, samples.size / Endpointer.RATE.toDouble())
    }

    /**
     * Reads [source] on a thread of its own from the gesture on, so that nothing said
     * while the engines load is lost; closes it at the end.
     */
    private class Capture(
        private val source: FrameSource,
    ) : AutoCloseable {
        private val frames = LinkedBlockingQueue<FloatArray>()
        private val reading = AtomicBoolean(true)
        private val reader =
            thread(name = "voicecmd-mic") {
                while (reading.get()) {
                    val f = source.next() ?: break
                    frames.put(f)
                }
                frames.put(END)
            }

        fun next(): FloatArray? {
            val f = frames.poll(FRAME_WAIT_S, TimeUnit.SECONDS)
            return if (f == null || f === END) null else f
        }

        override fun close() {
            reading.set(false)
            reader.join(READER_JOIN_MS)
            source.close()
        }
    }

    private companion object {
        val END = FloatArray(0)
        const val NS_PER_MS = 1_000_000L
        const val FRAME_WAIT_S = 2L
        const val READER_JOIN_MS = 500L
    }
}
