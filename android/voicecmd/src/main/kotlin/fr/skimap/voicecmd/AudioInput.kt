package fr.skimap.voicecmd

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder

/** 16 kHz mono frames of [Endpointer.FRAME] samples, read one after the other. */
interface FrameSource : AutoCloseable {
    /** The next frame, null at the end. Blocks for the microphone. */
    fun next(): FloatArray?
}

/** Recorded audio (tests): its frames, then silence for as long as asked. */
class SamplesSource(
    private val samples: FloatArray,
    private val silenceAfter: Int = Endpointer.ms(SILENCE_AFTER_MS),
) : FrameSource {
    private var pos = 0
    private var tail = 0

    override fun next(): FloatArray? {
        if (pos < samples.size) {
            val f = FloatArray(Endpointer.FRAME)
            val n = minOf(Endpointer.FRAME, samples.size - pos)
            System.arraycopy(samples, pos, f, 0, n)
            pos += n
            return f
        }
        return if (tail++ < silenceAfter) FloatArray(Endpointer.FRAME) else null
    }

    override fun close() = Unit

    private companion object {
        const val SILENCE_AFTER_MS = 2000
    }
}

/**
 * The microphone, opened for one command: AudioRecord at 16 kHz mono, source
 * VOICE_RECOGNITION (tuned for speech recognition, no voice-call processing). The caller
 * holds RECORD_AUDIO (asked by the screen before the first command).
 */
class MicSource : FrameSource {
    private val rec: AudioRecord
    private val pcm = ShortArray(Endpointer.FRAME)

    init {
        val min =
            AudioRecord.getMinBufferSize(Endpointer.RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        rec = open(maxOf(min, Endpointer.FRAME * 2 * BUFFERED_FRAMES))
        check(rec.state == AudioRecord.STATE_INITIALIZED) { "AudioRecord init failed" }
        rec.startRecording()
    }

    @SuppressLint("MissingPermission")
    private fun open(bytes: Int) =
        AudioRecord(
            MediaRecorder.AudioSource.VOICE_RECOGNITION,
            Endpointer.RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
            bytes,
        )

    override fun next(): FloatArray? {
        var got = 0
        while (got < pcm.size) {
            val r = rec.read(pcm, got, pcm.size - got)
            if (r < 0) return null
            got += r
        }
        return FloatArray(pcm.size) { pcm[it] / PCM_SCALE }
    }

    override fun close() {
        rec.stop()
        rec.release()
    }

    private companion object {
        const val BUFFERED_FRAMES = 8
        const val PCM_SCALE = 32768f
    }
}
