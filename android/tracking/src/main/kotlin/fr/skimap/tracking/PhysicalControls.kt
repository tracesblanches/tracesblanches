package fr.skimap.tracking

import android.content.Context
import android.media.AudioAttributes
import android.media.VolumeProvider
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Handler
import android.view.KeyEvent

/** What a button asks (K13). */
enum class ButtonAction { NEXT_DIRECTION, TIME_LEFT, BOTH, BRING_ME_BACK }

/**
 * Gestures of the headset button and of the volume keys, screen locked (K13): pure logic,
 * times in milliseconds.
 *
 * - Headset button: a press says the next direction and the time left; held
 *   [LONG_PRESS_MS], « ramène-moi » (once per hold).
 * - Volume keys (screen off): up says the next direction, down the time left; held (key
 *   repeats for [LONG_PRESS_MS]), « ramène-moi ».
 */
class ButtonGestures(
    private val act: (ButtonAction) -> Unit,
) {
    private var downAt = -1L
    private var longDone = false
    private var volumeDir = 0
    private var volumeFirst = -1L
    private var volumeLast = -1L
    private var volumeLong = false

    /** A media key event; true when handled. */
    fun mediaKey(
        action: Int,
        repeatCount: Int,
        longPress: Boolean,
        tMs: Long,
    ): Boolean {
        when (action) {
            KeyEvent.ACTION_DOWN -> {
                if (repeatCount == 0) {
                    downAt = tMs
                    longDone = false
                } else if (!longDone && (longPress || tMs - downAt >= LONG_PRESS_MS)) {
                    longDone = true
                    act(ButtonAction.BRING_ME_BACK)
                }
            }

            KeyEvent.ACTION_UP -> {
                if (!longDone && downAt >= 0) {
                    if (tMs - downAt >= LONG_PRESS_MS) act(ButtonAction.BRING_ME_BACK) else act(ButtonAction.BOTH)
                }
                downAt = -1
                longDone = false
            }
        }
        return true
    }

    /**
     * A volume adjustment from the remote volume provider (+1, −1). Key repeats arrive as
     * adjustments every ~50 ms: within [REPEAT_GAP_MS] of the previous one they belong to
     * the same hold. The short action is taken when the next adjustment shows the hold is
     * over, or by [flushVolume].
     */
    fun volume(
        direction: Int,
        tMs: Long,
    ) {
        if (direction == 0) return
        if (volumeFirst >= 0 && (tMs - volumeLast > REPEAT_GAP_MS || direction != volumeDir)) flushVolume()
        if (volumeFirst < 0) {
            volumeFirst = tMs
            volumeDir = direction
            volumeLong = false
        }
        volumeLast = tMs
        if (!volumeLong && tMs - volumeFirst >= LONG_PRESS_MS) {
            volumeLong = true
            act(ButtonAction.BRING_ME_BACK)
        }
    }

    /** Ends the current volume press (called [REPEAT_GAP_MS] after the last adjustment). */
    fun flushVolume() {
        if (volumeFirst < 0) return
        if (!volumeLong) act(if (volumeDir > 0) ButtonAction.NEXT_DIRECTION else ButtonAction.TIME_LEFT)
        volumeFirst = -1
        volumeLast = -1
        volumeLong = false
    }

    companion object {
        const val LONG_PRESS_MS = 800L

        // Key repeats start 500 ms after the press (ViewConfiguration), then every 50 ms.
        const val REPEAT_GAP_MS = 600L
    }
}

/**
 * The media session of the guidance: headset button always, volume keys when the screen
 * is off (remote volume: the keys come to us instead of changing the volume; screen on,
 * they change the volume as usual). Framework API, no extra library.
 */
class PhysicalControls(
    context: Context,
    private val handler: Handler,
    act: (ButtonAction) -> Unit,
) : AutoCloseable {
    val gestures = ButtonGestures(act)
    private val flush = Runnable { gestures.flushVolume() }
    private val session = MediaSession(context, "skimap-guidance")
    private val volume =
        object : VolumeProvider(VOLUME_CONTROL_RELATIVE, VOLUME_MAX, VOLUME_MAX / 2) {
            override fun onAdjustVolume(direction: Int) {
                handler.removeCallbacks(flush)
                gestures.volume(direction, android.os.SystemClock.uptimeMillis())
                handler.postDelayed(flush, ButtonGestures.REPEAT_GAP_MS)
            }
        }

    init {
        session.setCallback(
            object : MediaSession.Callback() {
                override fun onMediaButtonEvent(intent: android.content.Intent): Boolean {
                    val ev =
                        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
                            intent.getParcelableExtra(android.content.Intent.EXTRA_KEY_EVENT, KeyEvent::class.java)
                        } else {
                            @Suppress("DEPRECATION")
                            intent.getParcelableExtra(android.content.Intent.EXTRA_KEY_EVENT)
                        }
                    return ev != null && ev.keyCode in MEDIA_KEYS &&
                        gestures.mediaKey(ev.action, ev.repeatCount, ev.isLongPress, ev.eventTime)
                }
            },
            handler,
        )
        // Playing: the session gets the media buttons and, screen off, the volume keys.
        session.setPlaybackState(
            PlaybackState
                .Builder()
                .setActions(PlaybackState.ACTION_PLAY_PAUSE or PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE)
                .setState(PlaybackState.STATE_PLAYING, 0, 0f)
                .build(),
        )
        session.isActive = true
    }

    /** Screen off: volume keys to the guidance; on: to the volume. */
    fun screen(on: Boolean) {
        if (on) {
            session.setPlaybackToLocal(
                AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_ASSISTANCE_NAVIGATION_GUIDANCE).build(),
            )
        } else {
            session.setPlaybackToRemote(volume)
        }
    }

    override fun close() {
        handler.removeCallbacks(flush)
        session.isActive = false
        session.release()
    }

    private companion object {
        const val VOLUME_MAX = 10
        val MEDIA_KEYS =
            setOf(
                KeyEvent.KEYCODE_HEADSETHOOK,
                KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE,
                KeyEvent.KEYCODE_MEDIA_PLAY,
                KeyEvent.KEYCODE_MEDIA_PAUSE,
            )
    }
}
