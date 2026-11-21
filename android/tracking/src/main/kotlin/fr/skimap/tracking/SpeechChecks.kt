package fr.skimap.tracking

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import android.widget.Toast

/** What the start of an outing tells about the voice, on the screen (fr/en). */
enum class SpeechWarning {
    /** The stream of the announcements is low: they may not be heard. */
    LOW_VOLUME,

    /** No voice of the app's language speaks offline: the announcements may stay silent, the safety ones vibrate. */
    NO_OFFLINE_VOICE,
}

/**
 * Checks of the voice when an outing starts. The app never changes the volume: a low one
 * is only told ([LOW_VOLUME_FRACTION], `android/docs/volume-annonces.md`).
 */
object SpeechChecks {
    /**
     * Below this fraction of its maximum the stream of the announcements is low: on the
     * platform's loudspeaker curve (AOSP: -40.8 dB at 20 %, -17 dB at 60 %, 0 dB at 100 %)
     * a third of the maximum is about -30 dB, and the usual media volume at which the
     * announcements were « à peine audibles » (6 or 7 of 25 on a Pixel, about 25 %) is
     * under it; half the maximum (about -22 dB) is not.
     */
    const val LOW_VOLUME_FRACTION = 1.0 / 3.0

    /** Whether the stream the announcements play on ([attributes]) is low (muted included). */
    fun lowVolume(
        manager: AudioManager?,
        attributes: AudioAttributes,
    ): Boolean {
        val m = manager ?: return false
        val stream = attributes.volumeControlStream
        val max = m.getStreamMaxVolume(stream)
        return max > 0 && m.getStreamVolume(stream) < max * LOW_VOLUME_FRACTION
    }

    /** The text of [w] in the app's language. */
    fun text(
        context: Context,
        w: SpeechWarning,
    ): String =
        context.getString(
            when (w) {
                SpeechWarning.LOW_VOLUME -> R.string.speech_low_volume
                SpeechWarning.NO_OFFLINE_VOICE -> R.string.speech_no_offline_voice
            },
        )

    /** Shows [w] on the screen (a long toast: the outing was just started from the app). */
    fun show(
        context: Context,
        w: SpeechWarning,
    ) {
        val text = text(context, w)
        Handler(Looper.getMainLooper()).post { Toast.makeText(context, text, Toast.LENGTH_LONG).show() }
    }
}
