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
import android.media.audiofx.LoudnessEnhancer
import android.os.Bundle
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.util.Log
import java.util.Locale
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutionException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.Future
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

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

/** Plays one buffer to its end on the guidance stream; the test harness gives a fake one. */
fun interface PcmOutput {
    fun play(pcm: Pcm)
}

/**
 * Says the core's announcements (K5, J4): the pack's clips assembled and played in one
 * go, with a transient audio focus that ducks the music, held from the first sound queued
 * to the last (a turn's sound and its words: no rise of the music between them) and given
 * back at once (report 09: short playbacks, no session kept open); the Android voice reads
 * the whole text when a clip is missing (`docs/voix.md`), in the language of the
 * announcements (VO6: the phone's, English by default, or that of the voice pack in use,
 * [language]). One announcement at a time, in order, on its own thread; the clips of an
 * announcement are decoded as soon as it is queued, in parallel, while the sounds before
 * it play (#179: decoding them one after the other once the turn's sound was over left
 * 0.35 to 1.1 s of silence before the words). Everything plays on the guidance stream
 * (usage navigation guidance, content speech) at full player volume: the level is that of
 * the clips (-10 LUFS compressed and limited, `docs/voix/banque-et-clips.md`) and of the
 * stream, raised during an outing while the queue plays when it is low ([StreamBoost],
 * `android/docs/volume-annonces.md`). The Android voice, not levelled, goes through a
 * [LoudnessEnhancer] of [TTS_GAIN_MB] on its own session; the clips do not (the platform
 * effect is a 7:1 compressor above -8 dBFS: on clips that peak at -1 dBTP it lowers them,
 * measured -3 dB at a gain of 0). The text-to-speech engine starts only when first needed.
 */
@Suppress("TooManyFunctions")
class Announcer(
    private val context: Context,
    @Volatile private var locale: Locale = localeOf(phoneLang()),
    private val decoder: (String) -> Pcm = OpusDecoder::decode,
    /** During an outing: the stream is raised while the queue plays ([StreamBoost]); null: never. */
    outing: (() -> Boolean)? = null,
    /** Gain effect on an audio session; the platform's [LoudnessEnhancer] unless a test gives its own. */
    private val enhancer: (session: Int, gainMb: Int) -> AutoCloseable? = ::loudnessEnhancer,
) : AutoCloseable {
    private val audio: ExecutorService = Executors.newSingleThreadExecutor { Thread(it, "skimap-voice") }
    private val decoding: ExecutorService =
        Executors.newFixedThreadPool(DECODERS) { Thread(it, "skimap-voice-decode").apply { isDaemon = true } }
    private val manager = context.getSystemService(AudioManager::class.java)

    /** Attributes of every sound and of the Android voice: guidance, speech. */
    val attributes: AudioAttributes =
        AudioAttributes
            .Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE)
            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
            .build()

    /** Volume of the stream raised while the queue plays, during an outing. */
    val boost: StreamBoost? = outing?.let { StreamBoost(context, attributes, it) }

    init {
        // Before anything is queued: a volume left raised by an app killed during an announcement.
        if (boost != null) audio.execute { StreamBoost.recover(context) }
    }

    /** Transient focus that ducks the other sounds while the queue plays. */
    val focusRequest: AudioFocusRequest =
        AudioFocusRequest
            .Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK)
            .setAudioAttributes(attributes)
            .build()

    /** Sounds and announcements queued and not over yet; the focus is held while > 0. */
    private val queued = AtomicInteger()
    private var focused = false

    /** Plays a buffer; the platform's [AudioTrack] unless a test gives its own. */
    var output: PcmOutput = PcmOutput(::playTrack)

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

    /** Language of the Android voice (`fr`, `en`): that of the core's announcements. */
    fun language(code: String) {
        val l = localeOf(code)
        audio.execute {
            if (l != locale) {
                locale = l
                ttsConfigured = false
            }
        }
    }

    /** Language the Android voice reads now, for the tests and the logs. */
    val languageCode: String get() = locale.language

    fun say(u: Utterance) {
        // Decoding starts now, while what is queued before plays.
        val pcm: List<Future<Pcm>>? = if (u.allClips) u.clips.map { c -> decoding.submit<Pcm> { decoder(c.file!!) } } else null
        queue {
            // A clip that does not decode or play (codec, file): the Android voice says it all.
            val ok =
                if (pcm != null) {
                    runCatching { playClips(u, pcm) }.getOrElse {
                        Log.w(TAG, "clips failed, Android voice instead: ${it.message}")
                        speak(u.text)
                    }
                } else {
                    speak(u.text)
                }
            onDone?.invoke(u, ok)
        }
    }

    /** Short sounds played (K6), for the tests and the logs. */
    @Volatile var earcons = 0
        private set

    /**
     * Plays a short sound of K6 ([EarconSynth]) in turn with the announcements (a turn's
     * sound just before its words), under the same focus.
     */
    fun earcon(
        e: Earcon,
        volume: Float,
    ) {
        queue {
            runCatching { output.play(Pcm(EarconSynth.pcm(e, volume), EarconSynth.RATE, 2)) }
                .onSuccess { earcons++ }
                .onFailure { Log.w(TAG, "earcon $e: ${it.message}") }
        }
    }

    /** Runs [block] on the voice thread once everything queued before it was said. */
    fun afterQueue(block: () -> Unit) {
        audio.execute(block)
    }

    /** Runs a sound on the voice thread with the focus held; given back after the last one. */
    private fun queue(block: () -> Unit) {
        queued.incrementAndGet()
        audio.execute {
            try {
                if (!focused) {
                    manager?.requestAudioFocus(focusRequest)
                    focused = true
                    boost?.raise()
                }
                block()
            } finally {
                if (queued.decrementAndGet() == 0 && focused) {
                    boost?.restore()
                    manager?.abandonAudioFocusRequest(focusRequest)
                    focused = false
                }
            }
        }
    }

    private fun playClips(
        u: Utterance,
        pcm: List<Future<Pcm>>,
    ): Boolean {
        val decoded =
            pcm.map {
                try {
                    it.get(DECODE_MAX_S, TimeUnit.SECONDS)
                } catch (e: ExecutionException) {
                    throw e.cause ?: e
                }
            }
        val assembled = PcmAssembler.assemble(decoded, u.clips.map { it.pauseMs }) ?: return false
        output.play(assembled)
        played++
        return true
    }

    /** The platform track of [pcm]: guidance stream, full player volume. */
    fun track(pcm: Pcm): AudioTrack {
        val channelMask = if (pcm.channels == 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
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
                .setBufferSizeInBytes(pcm.samples.size * 2)
                .build()
        // No software attenuation: the clips carry the level, the phone's volume the rest.
        track.setVolume(AudioTrack.getMaxVolume())
        return track
    }

    /** Plays [pcm] in one go on its own track, released at the end. */
    private fun playTrack(pcm: Pcm) {
        val track = track(pcm)
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
        } finally {
            track.release()
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

    /** Audio session of the last reading of the Android voice, for the tests and the logs. */
    @Volatile var ttsSession = AudioManager.ERROR
        private set

    private fun speak(text: String): Boolean {
        val t = engine() ?: return false
        val session = manager?.generateAudioSessionId() ?: AudioManager.ERROR
        ttsSession = session
        val gain = if (session > 0) runCatching { enhancer(session, TTS_GAIN_MB) }.getOrNull() else null
        try {
            return read(t, text, session)
        } finally {
            gain?.close()
        }
    }

    private fun read(
        t: TextToSpeech,
        text: String,
        session: Int,
    ): Boolean {
        val done = CountDownLatch(1)
        t.setOnUtteranceProgressListener(
            object : UtteranceProgressListener() {
                override fun onStart(id: String?) = Unit

                override fun onDone(id: String?) = done.countDown()

                @Deprecated("Deprecated in Java")
                override fun onError(id: String?) = done.countDown()
            },
        )
        t.speak(text, TextToSpeech.QUEUE_FLUSH, ttsParams(session), "skimap")
        done.await(TTS_MAX_S, TimeUnit.SECONDS)
        spokenByTts++
        return true
    }

    override fun close() {
        audio.execute {
            tts?.shutdown()
            tts = null
        }
        audio.shutdown()
        decoding.shutdown()
    }

    companion object {
        private const val TAG = "skimap"
        private const val MARGIN_MS = 300L
        private const val MS_PER_S = 1000L
        private const val TTS_INIT_S = 20L
        private const val TTS_MAX_S = 30L
        private const val DECODE_MAX_S = 10L

        /** Clips decoded at the same time: all those of an announcement (up to 7 measured). */
        const val DECODERS = 8

        /**
         * Gain of the Android voice, mB: +6 dB. Measured through a port of the platform
         * effect on the clips before #179 (Pocket TTS speech at -21 LUFS, peaks -9 dBTP, as
         * a voice not levelled): +5.1 dB of loudness in median, no sample clamped; at 900 mB
         * +6.4 dB only, the 7:1 compressor working more.
         */
        const val TTS_GAIN_MB = 600

        /** Android voice at full volume, as the clips, on [session] when one was made. */
        fun ttsParams(session: Int = AudioManager.ERROR): Bundle =
            Bundle().apply {
                putFloat(TextToSpeech.Engine.KEY_PARAM_VOLUME, 1f)
                if (session > 0) putInt(TextToSpeech.Engine.KEY_PARAM_SESSION_ID, session)
            }

        /** The platform's loudness enhancer on [session], enabled at [gainMb]; released by close. */
        fun loudnessEnhancer(
            session: Int,
            gainMb: Int,
        ): AutoCloseable {
            val e = LoudnessEnhancer(session)
            e.setTargetGain(gainMb)
            e.enabled = true
            return AutoCloseable { e.release() }
        }

        /** Language of the app for the phone's: `fr` on a French phone, else `en` (VO6). */
        fun phoneLang(locale: Locale = Locale.getDefault()): String = if (locale.language == "fr") "fr" else "en"

        /** Voice locale of a language code: French of France, else British English. */
        fun localeOf(code: String): Locale = if (code.startsWith("fr")) Locale.FRANCE else Locale.UK
    }
}
