package fr.skimap.tracking

import kotlin.math.PI
import kotlin.math.min
import kotlin.math.sin

/** The short sounds of K6 (F23–F25): left, right, off the route, reassurance. */
enum class Earcon { LEFT, RIGHT, OFF_ROUTE, REASSURANCE }

/** Settings of the short sounds (K6): on or off, and their volume (0 to 1). */
data class EarconSettings(
    val enabled: Boolean,
    val volume: Float,
) {
    companion object {
        /** Discreet by default: half of the full scale of the sounds (about −20 dBFS at the peak). */
        const val DEFAULT_VOLUME = 0.5f
    }
}

/**
 * The short sounds synthesised once (no file): interleaved stereo 16-bit at [RATE], each a
 * few hundred milliseconds, faded in and out (no click), panned in the headset: left is
 * heard on the left, right on the right; off the route and reassurance in the middle.
 *
 * - left / right: two short high beeps (1 320 then 1 760 Hz, 70 ms each), 90 % on one side;
 * - off the route: three low falling pulses (440 → 330 Hz, 110 ms), clearly not a turn;
 * - reassurance: one soft chime (990 Hz, 140 ms), quieter than the others.
 */
object EarconSynth {
    const val RATE = 24_000
    private const val MS = 1000.0
    private const val FADE_MS = 8.0
    private const val FULL = 0.2 * Short.MAX_VALUE

    /** Pan: gains of the left and right channels. */
    private fun pan(e: Earcon): Pair<Double, Double> =
        when (e) {
            Earcon.LEFT -> 1.0 to SIDE_OTHER
            Earcon.RIGHT -> SIDE_OTHER to 1.0
            else -> CENTRE to CENTRE
        }

    private const val SIDE_OTHER = 0.1
    private const val CENTRE = 0.8

    /** One tone: start and length, milliseconds; frequency at its start and end, Hz; level 0–1. */
    private class Tone(
        val startMs: Double,
        val lenMs: Double,
        val fromHz: Double,
        val toHz: Double,
        val level: Double,
    )

    /** The sound design: the tones of each sound. */
    @Suppress("MagicNumber")
    private fun tones(e: Earcon): List<Tone> =
        when (e) {
            Earcon.LEFT, Earcon.RIGHT -> listOf(Tone(0.0, 70.0, 1320.0, 1320.0, 1.0), Tone(110.0, 70.0, 1760.0, 1760.0, 1.0))
            Earcon.OFF_ROUTE -> listOf(0.0, 160.0, 320.0).map { Tone(it, 110.0, 440.0, 330.0, 1.0) }
            Earcon.REASSURANCE -> listOf(Tone(0.0, 140.0, 990.0, 990.0, 0.6))
        }

    /** Interleaved stereo samples of [e] at [volume] (0 to 1). */
    fun pcm(
        e: Earcon,
        volume: Float,
    ): ShortArray {
        val ts = tones(e)
        val lenMs = ts.maxOf { it.startMs + it.lenMs }
        val frames = (lenMs * RATE / MS).toInt()
        val out = ShortArray(frames * 2)
        val (gl, gr) = pan(e)
        val v = volume.coerceIn(0f, 1f) * FULL
        for (t in ts) {
            val start = (t.startMs * RATE / MS).toInt()
            val n = (t.lenMs * RATE / MS).toInt()
            val fade = (FADE_MS * RATE / MS).toInt()
            var phase = 0.0
            for (i in 0 until n) {
                val f = t.fromHz + (t.toHz - t.fromHz) * i / n
                phase += 2 * PI * f / RATE
                val env = min(1.0, min(i.toDouble(), (n - 1 - i).toDouble()) / fade)
                val s = sin(phase) * env * t.level * v
                val k = (start + i) * 2
                out[k] = (out[k] + s * gl).toInt().coerceIn(Short.MIN_VALUE.toInt(), Short.MAX_VALUE.toInt()).toShort()
                out[k + 1] = (out[k + 1] + s * gr).toInt().coerceIn(Short.MIN_VALUE.toInt(), Short.MAX_VALUE.toInt()).toShort()
            }
        }
        return out
    }

    /** The sound of a turn (left or right; none straight on). */
    fun turn(t: Turn?): Earcon? =
        when (t) {
            Turn.LEFT, Turn.SLIGHT_LEFT, Turn.KEEP_LEFT -> Earcon.LEFT
            Turn.RIGHT, Turn.SLIGHT_RIGHT, Turn.KEEP_RIGHT -> Earcon.RIGHT
            else -> null
        }

    /** The sound of an alert: off the route, back on it (reassurance). */
    fun alert(a: AlertKind): Earcon? =
        when (a) {
            AlertKind.OFF_ROUTE -> Earcon.OFF_ROUTE
            AlertKind.BACK_ON_ROUTE -> Earcon.REASSURANCE
            else -> null
        }
}
