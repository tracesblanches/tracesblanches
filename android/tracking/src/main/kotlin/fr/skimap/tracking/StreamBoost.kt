package fr.skimap.tracking

import android.annotation.SuppressLint
import android.app.NotificationManager
import android.content.Context
import android.content.SharedPreferences
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioManager
import androidx.core.content.edit
import kotlin.math.ceil

/**
 * Volume of the guidance stream while an announcement plays (#179): at the usual media
 * volume of a phone (about 25 %, 6 or 7 of 25 on a Pixel, -36 dB on the platform's
 * loudspeaker curve) the clips were « à peine audibles ». When the volume of the stream
 * the announcements play on (usage navigation guidance: the media stream on a Pixel) is
 * under [SPEAKER_FLOOR] of its maximum (-12 dB on that curve, 24 dB louder than 25 %), it
 * is raised to that floor just before the first sound of the queue and given back exactly
 * just after the last ([Announcer]: one raise for sounds that follow each other, never a
 * raise taken as the volume « before »). A volume the user changed during the announcement
 * is kept. The volume before is written ([prefs], synchronously) before the raise: an app
 * killed while raised gives it back at its next start ([recover]).
 *
 * Never raised outside an outing ([active]), in Do not disturb (any interruption filter
 * but « all »), nor above [HEADSET_FLOOR] when a headset, earphones or a hearing aid is
 * connected.
 */
class StreamBoost(
    context: Context,
    private val attributes: AudioAttributes,
    private val active: () -> Boolean,
) {
    private val manager: AudioManager? = context.getSystemService(AudioManager::class.java)
    private val notifications: NotificationManager? = context.getSystemService(NotificationManager::class.java)
    private val prefs: SharedPreferences = prefs(context)

    /** Stream of the announcements: the media stream for navigation guidance. */
    val stream: Int get() = attributes.volumeControlStream

    /** Raised now (a restore pending). */
    val raised: Boolean get() = prefs.contains(KEY_BEFORE)

    /**
     * Raises the stream to its floor if it is under it and the conditions allow; nothing
     * when already raised (the queue goes on: the volume before stays the first one).
     */
    fun raise() {
        val m = manager ?: return
        if (raised || !allowed()) return
        val s = stream
        val before = m.getStreamVolume(s)
        val floor = floorOf(m.getStreamMaxVolume(s), headset(m))
        if (before < floor) raiseTo(m, s, before, floor)
    }

    @SuppressLint("ApplySharedPref") // On disk before the raise, for a kill during it.
    private fun raiseTo(
        m: AudioManager,
        s: Int,
        before: Int,
        floor: Int,
    ) {
        // Written before the raise: a kill right after it is given back at the next start.
        prefs.edit(commit = true) {
            putInt(KEY_STREAM, s)
            putInt(KEY_BEFORE, before)
            putInt(KEY_SET, floor)
        }
        m.setStreamVolume(s, floor, 0)
        // What the system kept (a safe-volume limit may refuse part of it): compared at the end.
        prefs.edit(commit = true) { putInt(KEY_SET, m.getStreamVolume(s)) }
    }

    /** Gives the volume back after the last sound of the queue ([restore]). */
    fun restore() {
        restore(manager, prefs)
    }

    /** During an outing, outside Do not disturb. */
    private fun allowed(): Boolean = active() && !doNotDisturb()

    private fun doNotDisturb(): Boolean {
        val f = notifications?.currentInterruptionFilter ?: return false
        return f != NotificationManager.INTERRUPTION_FILTER_ALL && f != NotificationManager.INTERRUPTION_FILTER_UNKNOWN
    }

    companion object {
        /** Floor on the loudspeaker, fraction of the maximum (18 of 25 on a Pixel). */
        const val SPEAKER_FLOOR = 0.70

        /** Floor with a headset (10 of 25): never louder than that in the ears. */
        const val HEADSET_FLOOR = 0.40

        private const val PREFS = "skimap_stream_boost"
        private const val KEY_STREAM = "stream"
        private const val KEY_BEFORE = "before"
        private const val KEY_SET = "set"

        // TYPE_BLE_HEADSET (API 31) is a constant: never reported by an older system.
        @SuppressLint("InlinedApi")
        private val HEADSETS =
            setOf(
                AudioDeviceInfo.TYPE_WIRED_HEADSET,
                AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
                AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
                AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
                AudioDeviceInfo.TYPE_USB_HEADSET,
                AudioDeviceInfo.TYPE_HEARING_AID,
                AudioDeviceInfo.TYPE_BLE_HEADSET,
            )

        /** Index the stream of maximum [max] is raised to. */
        fun floorOf(
            max: Int,
            headset: Boolean,
        ): Int = ceil(max * if (headset) HEADSET_FLOOR else SPEAKER_FLOOR).toInt()

        /** A headset, earphones or a hearing aid connected. */
        fun headset(m: AudioManager): Boolean = m.getDevices(AudioManager.GET_DEVICES_OUTPUTS).any { it.type in HEADSETS }

        private fun prefs(context: Context): SharedPreferences = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

        /**
         * At the start of the app: a volume still raised when the app was killed during an
         * announcement is given back (unless the user changed it since).
         */
        fun recover(context: Context) {
            restore(context.getSystemService(AudioManager::class.java), prefs(context))
        }

        @SuppressLint("ApplySharedPref") // Cleared on disk once given back.
        private fun restore(
            m: AudioManager?,
            prefs: SharedPreferences,
        ) {
            if (!prefs.contains(KEY_BEFORE)) return
            val s = prefs.getInt(KEY_STREAM, AudioManager.STREAM_MUSIC)
            val before = prefs.getInt(KEY_BEFORE, 0)
            val set = prefs.getInt(KEY_SET, -1)
            // Still at the raised volume: back to the one before; else the user's is kept.
            if (m != null && m.getStreamVolume(s) == set) m.setStreamVolume(s, before, 0)
            prefs.edit(commit = true) {
                remove(KEY_STREAM)
                remove(KEY_BEFORE)
                remove(KEY_SET)
            }
        }
    }
}
