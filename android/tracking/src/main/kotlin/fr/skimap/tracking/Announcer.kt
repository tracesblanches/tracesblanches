package fr.skimap.tracking

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.util.Log
import java.util.Locale
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Decoded clip: mono or interleaved 16-bit samples at [rate]. */
class Pcm(
    val samples: ShortArray,
    val rate: Int,
    val channels: Int,
)

/**
 * Clips assembled into one buffer (K5): each clip, then its silence (150 ms inside a
 * message, 400 ms between two), as the pack's samples were assembled on the workstation.
 */
object PcmAssembler {
    private const val MS_PER_S = 1000L

    fun assemble(
        clips: List<Pcm>,
        pausesMs: List<Int>,
    ): Pcm? {
        val first = clips.firstOrNull() ?: return null
        require(clips.all { it.rate == first.rate && it.channels == first.channels }) { "clips of different formats" }
        val frames = { ms: Int -> first.rate.toLong() * ms / MS_PER_S * first.channels }
        var total = 0L
        clips.forEachIndexed { i, c -> total += c.samples.size + frames(pausesMs.getOrElse(i) { 0 }) }
        val out = ShortArray(total.toInt())
        var at = 0
        clips.forEachIndexed { i, c ->
            c.samples.copyInto(out, at)
            at += c.samples.size + frames(pausesMs.getOrElse(i) { 0 }).toInt()
        }
        return Pcm(out, first.rate, first.channels)
    }
}

/** Ogg/Opus file of the pack decoded with the platform codec. */
object OpusDecoder {
    private const val TIMEOUT_US = 10_000L

    fun decode(path: String): Pcm {
        val ex = MediaExtractor()
        try {
            ex.setDataSource(path)
            val track =
                (0 until ex.trackCount).first {
                    ex
                        .getTrackFormat(
                            it,
                        ).getString(MediaFormat.KEY_MIME)
                        ?.startsWith("audio/") == true
                }
            ex.selectTrack(track)
            val format = ex.getTrackFormat(track)
            val codec = MediaCodec.createDecoderByType(format.getString(MediaFormat.KEY_MIME)!!)
            try {
                codec.configure(format, null, null, 0)
                codec.start()
                return drain(ex, codec, format)
            } finally {
                codec.release()
            }
        } finally {
            ex.release()
        }
    }

    /** Gives the next compressed packet to the codec; true once the end was given. */
    private fun feed(
        ex: MediaExtractor,
        codec: MediaCodec,
    ): Boolean {
        val i = codec.dequeueInputBuffer(TIMEOUT_US)
        if (i < 0) return false
        val n = ex.readSampleData(codec.getInputBuffer(i)!!, 0)
        val end = n < 0
        if (end) {
            codec.queueInputBuffer(i, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
        } else {
            codec.queueInputBuffer(i, 0, n, ex.sampleTime, 0)
            ex.advance()
        }
        return end
    }

    private fun drain(
        ex: MediaExtractor,
        codec: MediaCodec,
        format: MediaFormat,
    ): Pcm {
        var rate = format.getInteger(MediaFormat.KEY_SAMPLE_RATE)
        var channels = format.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
        val chunks = ArrayList<ShortArray>()
        val info = MediaCodec.BufferInfo()
        var inputDone = false
        var outputDone = false
        while (!outputDone) {
            if (!inputDone) inputDone = feed(ex, codec)
            val o = codec.dequeueOutputBuffer(info, TIMEOUT_US)
            if (o >= 0) {
                val buf = codec.getOutputBuffer(o)!!
                val samples = ShortArray(info.size / 2)
                buf.position(info.offset)
                buf.order(java.nio.ByteOrder.nativeOrder()).asShortBuffer().get(samples)
                chunks += samples
                codec.releaseOutputBuffer(o, false)
                outputDone = info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0
            } else if (o == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                rate = codec.outputFormat.getInteger(MediaFormat.KEY_SAMPLE_RATE)
                channels = codec.outputFormat.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
            }
        }
        val out = ShortArray(chunks.sumOf { it.size })
        var at = 0
        for (c in chunks) {
            c.copyInto(out, at)
            at += c.size
        }
        return Pcm(out, rate, channels)
    }
}

/**
 * Says the core's announcements (K5, J4): the pack's clips assembled and played in one
 * go, with a transient audio focus that ducks the music and is given back at once
 * (report 09: short playbacks, no session kept open); the Android voice reads the whole
 * text when a clip is missing (`docs/voix.md`). One announcement at a time, in order, on
 * its own thread; the text-to-speech engine starts only when first needed.
 */
class Announcer(
    private val context: Context,
    private val locale: Locale = Locale.FRANCE,
) : AutoCloseable {
    private val audio: ExecutorService = Executors.newSingleThreadExecutor { Thread(it, "skimap-voice") }
    private val manager = context.getSystemService(AudioManager::class.java)
    private val attributes =
        AudioAttributes
            .Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE)
            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
            .build()
    private var tts: TextToSpeech? = null

    @Volatile private var ttsStatus = TextToSpeech.ERROR

    @Volatile private var ttsReady = CountDownLatch(1)
    private var ttsConfigured = false

    /** Announcements played (clips) and read (Android voice), for the tests and the logs. */
    @Volatile var played = 0
        private set

    @Volatile var spokenByTts = 0
        private set

    /** Called after each announcement, on the voice thread. */
    var onDone: ((Utterance, Boolean) -> Unit)? = null

    fun say(u: Utterance) {
        audio.execute {
            // A clip that does not decode or play (codec, file): the Android voice says it all.
            val ok =
                if (u.allClips) {
                    runCatching { playClips(u) }.getOrElse {
                        Log.w(TAG, "clips failed, Android voice instead: ${it.message}")
                        speak(u.text)
                    }
                } else {
                    speak(u.text)
                }
            onDone?.invoke(u, ok)
        }
    }

    /** Runs [block] on the voice thread once everything queued before it was said. */
    fun afterQueue(block: () -> Unit) {
        audio.execute(block)
    }

    private fun focus(): AudioFocusRequest {
        val req =
            AudioFocusRequest
                .Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK)
                .setAudioAttributes(attributes)
                .build()
        manager?.requestAudioFocus(req)
        return req
    }

    private fun playClips(u: Utterance): Boolean {
        val pcm =
            PcmAssembler.assemble(u.clips.map { OpusDecoder.decode(it.file!!) }, u.clips.map { it.pauseMs })
                ?: return false
        val channelMask = if (pcm.channels == 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
        val bytes = pcm.samples.size * 2
        val track =
            AudioTrack
                .Builder()
                .setAudioAttributes(attributes)
                .setAudioFormat(
                    AudioFormat
                        .Builder()
                        .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                        .setSampleRate(pcm.rate)
                        .setChannelMask(channelMask)
                        .build(),
                ).setTransferMode(AudioTrack.MODE_STATIC)
                .setBufferSizeInBytes(bytes)
                .build()
        val req = focus()
        try {
            track.write(pcm.samples, 0, pcm.samples.size)
            val frames = pcm.samples.size / pcm.channels
            val done = CountDownLatch(1)
            track.notificationMarkerPosition = frames
            track.setPlaybackPositionUpdateListener(
                object : AudioTrack.OnPlaybackPositionUpdateListener {
                    override fun onMarkerReached(t: AudioTrack) = done.countDown()

                    override fun onPeriodicNotification(t: AudioTrack) = Unit
                },
                android.os.Handler(android.os.Looper.getMainLooper()),
            )
            track.play()
            val ms = frames * MS_PER_S / pcm.rate
            done.await(ms + MARGIN_MS, TimeUnit.MILLISECONDS)
            played++
            return true
        } finally {
            track.release()
            manager?.abandonAudioFocusRequest(req)
        }
    }

    /**
     * Starts the Android voice in the background (the service does it when the outing
     * starts: the proposals of C11 and the names the pack has no clip for need it, and its
     * engine may take seconds to bind). Nothing is said.
     */
    fun warmUp() {
        audio.execute { starting() }
    }

    /** The engine being started (callback pending) or started. */
    private fun starting(): TextToSpeech {
        tts?.let { return it }
        val t =
            TextToSpeech(context) {
                ttsStatus = it
                ttsReady.countDown()
            }
        tts = t
        return t
    }

    /** The Android voice once started, or null if it cannot start. */
    private fun engine(): TextToSpeech? {
        val t = starting()
        if (!ttsReady.await(TTS_INIT_S, TimeUnit.SECONDS) || ttsStatus != TextToSpeech.SUCCESS) {
            Log.w(TAG, "Android voice unavailable (status $ttsStatus)")
            if (ttsReady.count == 0L) {
                // Failed for good: shut it, the next announcement tries again.
                t.shutdown()
                tts = null
                ttsReady = CountDownLatch(1)
            }
            return null
        }
        if (!ttsConfigured) {
            t.language = locale
            t.setAudioAttributes(attributes)
            ttsConfigured = true
        }
        return t
    }

    private fun speak(text: String): Boolean {
        val t = engine() ?: return false
        val done = CountDownLatch(1)
        t.setOnUtteranceProgressListener(
            object : UtteranceProgressListener() {
                override fun onStart(id: String?) = Unit

                override fun onDone(id: String?) = done.countDown()

                @Deprecated("Deprecated in Java")
                override fun onError(id: String?) = done.countDown()
            },
        )
        val req = focus()
        try {
            t.speak(text, TextToSpeech.QUEUE_FLUSH, null, "skimap")
            done.await(TTS_MAX_S, TimeUnit.SECONDS)
            spokenByTts++
            return true
        } finally {
            manager?.abandonAudioFocusRequest(req)
        }
    }

    override fun close() {
        audio.execute {
            tts?.shutdown()
            tts = null
        }
        audio.shutdown()
    }

    private companion object {
        const val TAG = "skimap"
        const val MARGIN_MS = 300L
        const val MS_PER_S = 1000L
        const val TTS_INIT_S = 20L
        const val TTS_MAX_S = 30L
    }
}
