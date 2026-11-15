package fr.skimap.voicecmd

/**
 * End of a spoken command from the speech probability of each frame (Silero VAD, frames
 * of [FRAME] samples at [RATE] Hz, 32 ms): pure logic, times in frames.
 *
 * Speech starts after [startFrames] frames at or above [startProb]; it ends after
 * [endFrames] frames under [endProb] (700 ms: a pause inside « Signale : trace faite »
 * does not end it). The question is cut at [maxFrames]; nothing heard for [waitFrames]
 * ends the listening empty. The audio kept starts [preRollFrames] before the speech.
 */
class Endpointer(
    private val startProb: Float = 0.5f,
    private val endProb: Float = 0.35f,
    private val startFrames: Int = 3,
    private val endFrames: Int = ms(END_MS),
    private val maxFrames: Int = ms(MAX_MS),
    private val waitFrames: Int = ms(WAIT_MS),
    private val preRollFrames: Int = ms(PRE_ROLL_MS),
) {
    enum class State { WAITING, SPEECH, DONE, NOTHING }

    var state = State.WAITING
        private set

    private val frames = ArrayList<FloatArray>()
    private var seen = 0
    private var above = 0
    private var below = 0
    private var speechAt = -1

    /** One frame and its speech probability; the new state. */
    fun push(
        frame: FloatArray,
        prob: Float,
    ): State {
        if (state == State.DONE || state == State.NOTHING) return state
        seen++
        frames += frame
        when (state) {
            State.WAITING -> {
                above = if (prob >= startProb) above + 1 else 0
                if (above >= startFrames) {
                    state = State.SPEECH
                    speechAt = frames.size - startFrames
                    below = 0
                } else {
                    if (seen >= waitFrames) state = State.NOTHING
                    // Only the pre-roll is kept while waiting.
                    if (frames.size > preRollFrames + startFrames) frames.removeAt(0)
                }
            }

            State.SPEECH -> {
                below = if (prob < endProb) below + 1 else 0
                if (below >= endFrames || frames.size - speechAt >= maxFrames) state = State.DONE
            }

            else -> {
                Unit
            }
        }
        return state
    }

    /** The question: from the pre-roll before the speech to the end (trailing silence included). */
    fun samples(): FloatArray {
        val from = (speechAt - preRollFrames).coerceAtLeast(0)
        val kept = if (speechAt < 0) emptyList() else frames.subList(from, frames.size)
        val out = FloatArray(kept.sumOf { it.size })
        var p = 0
        for (f in kept) {
            System.arraycopy(f, 0, out, p, f.size)
            p += f.size
        }
        return out
    }

    companion object {
        const val RATE = 16_000
        const val FRAME = 512
        const val END_MS = 700
        const val MAX_MS = 8000
        const val WAIT_MS = 5000
        const val PRE_ROLL_MS = 300
        private const val MS_PER_S = 1000

        /** Frames in [millis] ms. */
        fun ms(millis: Int): Int = millis * RATE / FRAME / MS_PER_S
    }
}
