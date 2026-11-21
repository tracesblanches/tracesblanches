package fr.skimap.tracking

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.media.audiofx.LoudnessEnhancer
import android.os.Bundle
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.util.Log
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong

/** A short sound: mono or interleaved 16-bit samples at [rate]. */
class Pcm(
    val samples: ShortArray,
    val rate: Int,
    val channels: Int,
)

/** Plays one buffer to its end on the guidance stream; the test harness gives a fake one. */
fun interface PcmOutput {
    fun play(pcm: Pcm)
}

/** What the phone's speech synthesis offers for the language of the announcements. */
enum class VoiceStatus {
    /** Not known yet: the engine is starting. */
    UNKNOWN,

    /** A voice of the language that speaks without a network. */
    OFFLINE,

    /** No engine, or no voice of the language usable offline: the announcements may stay silent. */
    NONE,
}

/**
 * Says the core's announcements with the phone's own speech synthesis (Android
 * [TextToSpeech], offline voice of the language of the app, `fr` or `en`, VO6), and the
 * short sounds of K6 ([EarconSynth]) in turn with them. Everything plays on the guidance
 * stream (usage navigation guidance, content speech) under a transient audio focus that
 * ducks the music, held from the first sound queued to the last and given back at once
 * (report 09: short playbacks, no session kept open). The app never changes the volume of
 * any stream: the level is the voice's, raised by a [LoudnessEnhancer] of [TTS_GAIN_MB] on
 * the synthesis's own audio session (`android/docs/volume-annonces.md`).
 *
 * Announcements that follow each other go to the engine's queue ([TextToSpeech.QUEUE_ADD])
 * one ahead: the engine holds the one it says and the next, and says them one after the
 * other without waiting for the app between them. Each one is checked when it is given to
 * the engine ([freshness], #183): one that waited past its position, or the same text said
 * a moment ago, is not said. A short sound waits for the words queued before it, and the
 * words after it wait for the sound. The engine starts when the outing starts ([warmUp]), or at the
 * first announcement; [voiceStatus] then tells whether a voice of the language speaks
 * offline ([onVoiceStatus]).
 */
@Suppress("TooManyFunctions")
class Announcer(
    private val context: Context,
    @Volatile private var locale: Locale = localeOf(phoneLang()),
    /** Gain effect on an audio session; the platform's [LoudnessEnhancer] unless a test gives its own. */
    private val enhancer: (session: Int, gainMb: Int) -> AutoCloseable? = ::loudnessEnhancer,
) : AutoCloseable {
    private val audio: ExecutorService = Executors.newSingleThreadExecutor { Thread(it, "skimap-voice") }
    private val manager = context.getSystemService(AudioManager::class.java)

    /** Attributes of every sound and of the voice: guidance, speech. */
    val attributes: AudioAttributes =
        AudioAttributes
            .Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE)
            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
            .build()

    /** Transient focus that ducks the other sounds while the queue plays. */
    val focusRequest: AudioFocusRequest =
        AudioFocusRequest
            .Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK)
            .setAudioAttributes(attributes)
            .build()

    /** Sounds and announcements queued and not over yet; the focus is held while > 0. */
    private val queued = AtomicInteger()
    private var focused = false

    /** Plays a short sound; the platform's [AudioTrack] unless a test gives its own. */
    var output: PcmOutput = PcmOutput(::playTrack)

    private var tts: TextToSpeech? = null

    @Volatile private var ttsStatus = TextToSpeech.ERROR

    @Volatile private var ttsReady = CountDownLatch(1)
    private var ttsConfigured = false

    /** Audio session of the synthesis (its [LoudnessEnhancer]), for the tests and the logs. */
    @Volatile var ttsSession = AudioManager.ERROR
        private set
    private var gain: AutoCloseable? = null

    /** Announcements given to the engine and not finished yet, by utterance id. */
    private val inEngine = ConcurrentHashMap<String, Utterance>()
    private val engineIdle = Object()
    private val nextId = AtomicLong()

    /** Announcements said to their end, for the tests and the logs. */
    @Volatile var spokenByTts = 0
        private set

    /** Checks each announcement when it is given to the engine (#183); none: all are said. */
    var freshness: Freshness? = null

    /** Announcements not said because they were stale ([Freshness]), for the tests and the logs. */
    @Volatile var stale = 0
        private set

    /** Called after each announcement (said, or not: false), on the engine's thread. */
    var onDone: ((Utterance, Boolean) -> Unit)? = null

    /** What the synthesis offers for the language, once the engine has started. */
    @Volatile var voiceStatus = VoiceStatus.UNKNOWN
        private set

    /** Called on the voice thread when [voiceStatus] is known (each time the language is set up). */
    var onVoiceStatus: ((VoiceStatus) -> Unit)? = null

    /** Language of the voice (`fr`, `en`): that of the core's announcements. */
    fun language(code: String) {
        val l = localeOf(code)
        audio.execute {
            if (l != locale) {
                locale = l
                ttsConfigured = false
                voiceStatus = VoiceStatus.UNKNOWN
            }
        }
    }

    /** Language the voice reads now, for the tests and the logs. */
    val languageCode: String get() = locale.language

    /**
     * Says [u]: given to the engine's queue after everything queued before it, once the
     * engine holds at most the one it says, if it is still fresh then.
     */
    fun say(u: Utterance) {
        queue {
            val t = engine()
            if (t == null) {
                onDone?.invoke(u, false)
            } else {
                awaitEngine(atMost = 1)
                val why = freshness?.check(u)
                if (why == null) {
                    submit(t, u)
                } else {
                    stale++
                    Log.i(TAG, "speech: not said ($why): ${u.text}")
                    onDone?.invoke(u, false)
                }
            }
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
            awaitEngine()
            runCatching { output.play(Pcm(EarconSynth.pcm(e, volume), EarconSynth.RATE, 2)) }
                .onSuccess { earcons++ }
                .onFailure { Log.w(TAG, "earcon $e: ${it.message}") }
        }
    }

    /** Runs [block] on the voice thread once everything queued before it was said. */
    fun afterQueue(block: () -> Unit) {
        audio.execute {
            awaitEngine()
            block()
        }
    }

    /** Runs a sound on the voice thread with the focus held; given back after the last one. */
    private fun queue(block: () -> Unit) {
        queued.incrementAndGet()
        audio.execute {
            try {
                if (!focused) {
                    manager?.requestAudioFocus(focusRequest)
                    focused = true
                }
                block()
            } finally {
                if (queued.decrementAndGet() == 0) {
                    // The last words of the queue are said before the focus goes back.
                    awaitEngine()
                    if (queued.get() == 0 && focused) {
                        manager?.abandonAudioFocusRequest(focusRequest)
                        focused = false
                    }
                }
            }
        }
    }

    private fun submit(
        t: TextToSpeech,
        u: Utterance,
    ) {
        val id = "skimap-${nextId.incrementAndGet()}"
        inEngine[id] = u
        val r = t.speak(u.text, TextToSpeech.QUEUE_ADD, ttsParams(ttsSession), id)
        if (r != TextToSpeech.SUCCESS) {
            Log.w(TAG, "speech refused ($r): ${u.text}")
            finished(id, false)
        }
    }

    /** End of an utterance (said, failed or stopped), from the engine. */
    private fun finished(
        id: String?,
        ok: Boolean,
    ) {
        val u = id?.let { inEngine.remove(it) } ?: return
        if (ok) spokenByTts++
        onDone?.invoke(u, ok)
        synchronized(engineIdle) { engineIdle.notifyAll() }
    }

    /**
     * Waits until the engine holds at most [atMost] announcements not finished: all of them
     * said by default ([TTS_MAX_S] per announcement at most, then they are dropped).
     */
    private fun awaitEngine(atMost: Int = 0) {
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(TTS_MAX_S * maxOf(1, inEngine.size))
        synchronized(engineIdle) {
            while (inEngine.size > atMost) {
                val left = TimeUnit.NANOSECONDS.toMillis(end - System.nanoTime())
                if (left <= 0) {
                    Log.w(TAG, "speech engine silent: ${inEngine.size} announcements dropped")
                    inEngine.keys.toList().forEach { k -> inEngine.remove(k)?.let { onDone?.invoke(it, false) } }
                    return
                }
                engineIdle.wait(left)
            }
        }
    }

    /** The platform track of [pcm]: guidance stream, full player volume. */
    fun track(pcm: Pcm): AudioTrack {
        val channelMask = if (pcm.channels == 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
        return AudioTrack
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
     * Starts the speech synthesis in the background (the service does it when the outing
     * starts: its engine may take seconds to bind) and checks its voice ([voiceStatus]).
     * Nothing is said.
     */
    fun warmUp() {
        audio.execute { engine() }
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

    /** The engine once started and set up for the language, or null if it cannot start. */
    private fun engine(): TextToSpeech? {
        val t = starting()
        if (!ttsReady.await(TTS_INIT_S, TimeUnit.SECONDS) || ttsStatus != TextToSpeech.SUCCESS) {
            Log.w(TAG, "speech synthesis unavailable (status $ttsStatus)")
            status(VoiceStatus.NONE)
            if (ttsReady.count == 0L) {
                // Failed for good: shut it, the next announcement tries again.
                t.shutdown()
                tts = null
                ttsReady = CountDownLatch(1)
                ttsConfigured = false
            }
            return null
        }
        if (!ttsConfigured) configure(t)
        return t
    }

    private fun configure(t: TextToSpeech) {
        val lang = t.setLanguage(locale)
        t.setAudioAttributes(attributes)
        t.setOnUtteranceProgressListener(
            object : UtteranceProgressListener() {
                override fun onStart(id: String?) = Unit

                override fun onDone(id: String?) = finished(id, true)

                @Deprecated("Deprecated in Java")
                override fun onError(id: String?) = finished(id, false)

                override fun onError(
                    id: String?,
                    errorCode: Int,
                ) = finished(id, false)

                override fun onStop(
                    id: String?,
                    interrupted: Boolean,
                ) = finished(id, false)
            },
        )
        if (gain == null) {
            val session = manager?.generateAudioSessionId() ?: AudioManager.ERROR
            if (session > 0) {
                ttsSession = session
                gain = runCatching { enhancer(session, TTS_GAIN_MB) }.getOrNull()
            }
        }
        ttsConfigured = true
        status(offline(t, lang))
    }

    /** Whether a voice of the language speaks without a network (its data installed). */
    private fun offline(
        t: TextToSpeech,
        setLanguage: Int,
    ): VoiceStatus {
        if (setLanguage < TextToSpeech.LANG_AVAILABLE) return VoiceStatus.NONE
        // An engine that lists no voice at all (older engines) speaks the language it accepted.
        val voices = runCatching { t.voices }.getOrNull().orEmpty()
        Log.i(
            TAG,
            "voices ${locale.language}: " +
                voices.filter { it.locale.language == locale.language }.joinToString {
                    "${it.name} network ${it.isNetworkConnectionRequired} ${it.features.orEmpty()}"
                },
        )
        val usable =
            voices.any { v ->
                v.locale.language == locale.language &&
                    !v.isNetworkConnectionRequired &&
                    TextToSpeech.Engine.KEY_FEATURE_NOT_INSTALLED !in v.features.orEmpty()
            }
        return if (usable || voices.isEmpty()) VoiceStatus.OFFLINE else VoiceStatus.NONE
    }

    private fun status(s: VoiceStatus) {
        val changed = voiceStatus != s
        voiceStatus = s
        if (changed) {
            Log.i(TAG, "speech synthesis ${locale.toLanguageTag()}: $s")
            onVoiceStatus?.invoke(s)
        }
    }

    override fun close() {
        audio.execute {
            tts?.shutdown()
            tts = null
            gain?.close()
            gain = null
        }
        audio.shutdown()
    }

    companion object {
        private const val TAG = "skimap"
        private const val MARGIN_MS = 300L
        private const val MS_PER_S = 1000L
        private const val TTS_INIT_S = 20L
        private const val TTS_MAX_S = 30L

        /**
         * Gain of the voice on its session, mB: +6 dB at the input of the platform effect,
         * then its 7:1 compressor above -8 dBFS and its hard clamp at full scale. Measured
         * through a port of the effect on the voices of the phone's engine (Google, fr-FR
         * and en-GB, 12 announcements of the bank, -18.3 LUFS in median, peaks -2.3 to
         * -4.3 dBFS): +3.4 dB of loudness in median (+2.7 to +4.0), no sample clamped; at
         * 900 mB, +4.4 dB but samples clamped (`android/docs/volume-annonces.md`).
         */
        const val TTS_GAIN_MB = 600

        /** Voice at full volume, on [session] when one was made (its [LoudnessEnhancer]). */
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
