package fr.skimap.tracking

import android.content.Context
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager

/**
 * Vibrations (K6, F25): felt through a jacket, told apart without looking. Safety: three
 * long pulses; back on the route: one short; left: two short; right: one long; turn back:
 * three short; a proposal, a closed trail or a protected zone entered off the trails: two
 * medium.
 */
object HapticPatterns {
    /** Off and on durations from the start, milliseconds (waveform timings). */
    val SAFETY = longArrayOf(0, 600, 200, 600, 200, 600)
    val BACK_ON = longArrayOf(0, 80)
    val LEFT = longArrayOf(0, 120, 150, 120)
    val RIGHT = longArrayOf(0, 450)
    val U_TURN = longArrayOf(0, 100, 120, 100, 120, 100)
    val PROPOSAL = longArrayOf(0, 250, 150, 250)

    fun alert(kind: AlertKind): LongArray =
        when (kind) {
            AlertKind.BACK_ON_ROUTE -> BACK_ON
            AlertKind.CLOSED, AlertKind.PROTECTED_ZONE -> PROPOSAL
            else -> SAFETY
        }

    fun turn(t: Turn?): LongArray? =
        when (t) {
            Turn.LEFT, Turn.SLIGHT_LEFT, Turn.KEEP_LEFT -> LEFT
            Turn.RIGHT, Turn.SLIGHT_RIGHT, Turn.KEEP_RIGHT -> RIGHT
            Turn.U_TURN -> U_TURN
            Turn.STRAIGHT, null -> null
        }
}

/** Plays [HapticPatterns] on the phone's vibrator. */
class Haptics(
    context: Context,
) {
    private val vibrator: Vibrator? =
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.S) {
            context.getSystemService(VibratorManager::class.java)?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            context.getSystemService(Vibrator::class.java)
        }

    /** Patterns played, newest last (tests). */
    val log = ArrayList<LongArray>()

    fun play(pattern: LongArray) {
        log += pattern
        val v = vibrator ?: return
        if (!v.hasVibrator()) return
        val effect = VibrationEffect.createWaveform(pattern, -1)
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
            // Alarm usage: felt even when the phone is on « vibrate only » or silent.
            v.vibrate(effect, VibrationAttributes.createForUsage(VibrationAttributes.USAGE_ALARM))
        } else {
            @Suppress("DEPRECATION")
            v.vibrate(effect)
        }
    }
}
