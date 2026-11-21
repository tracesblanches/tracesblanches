package fr.skimap.voicecmd

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayInputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** The French fix of VO5 (#87): level, the spoken greeting before the question, its words dropped. */
class FrenchContextTest {
    @Test
    fun quietSpeechIsBroughtToHalfScaleAndLoudSpeechIsLeft() {
        // A quiet speaker (peak 0.04, as the `marius` clips): gain 12.5.
        val quiet = floatArrayOf(0f, 0.04f, -0.02f)
        assertArrayEquals(floatArrayOf(0f, 0.5f, -0.25f), Level.normalized(quiet), 1e-6f)
        // Already loud: brought down to the same peak, never clipped.
        assertArrayEquals(floatArrayOf(0.5f, -0.25f), Level.normalized(floatArrayOf(0.9f, -0.45f)), 1e-6f)
        // Silence (or near) is not blown up past the gain cap.
        val faint = Level.normalized(floatArrayOf(1e-5f))
        assertEquals(1e-5f * Level.MAX_GAIN, faint[0], 1e-9f)
        assertEquals(0, Level.normalized(FloatArray(0)).size)
    }

    @Test
    fun wordsOfTheGreetingAreDroppedByTheirTime() {
        // « Bonjour. Où suis-je ? » with the greeting ending before 0.9 s.
        val tokens = arrayOf(" Bon", "jour", ".", " Où", " su", "is", "-", "je", " ?")
        val times = floatArrayOf(0.5f, 0.7f, 0.85f, 1.3f, 1.4f, 1.5f, 1.55f, 1.6f, 1.7f)
        assertEquals("Où suis-je ?", GreetingContext.after(tokens, times, 1.0f))
        assertEquals("Bonjour. Où suis-je ?", GreetingContext.after(tokens, times, 0f))
        assertEquals("", GreetingContext.after(tokens, times, 2f))
    }

    @Test
    fun greetingWavIsReadAsSamplesAt16k() {
        val pcm = shortArrayOf(0, 16384, -32768)
        val b = ByteBuffer.allocate(44 + 2 * pcm.size).order(ByteOrder.LITTLE_ENDIAN)
        b.put("RIFF".toByteArray()).putInt(36 + 2 * pcm.size).put("WAVEfmt ".toByteArray())
        b
            .putInt(16)
            .putShort(1)
            .putShort(1)
            .putInt(16_000)
            .putInt(32_000)
            .putShort(2)
            .putShort(16)
        b.put("data".toByteArray()).putInt(2 * pcm.size)
        pcm.forEach { b.putShort(it) }
        val s = GreetingContext.readWav(ByteArrayInputStream(b.array()))
        assertArrayEquals(floatArrayOf(0f, 0.5f, -1f), s, 1e-6f)
    }

    @Test
    fun theFrenchGreetingShipsAndEnglishHasNone() {
        val s = GreetingContext.greeting(CommandLang.FR)!!
        val seconds = s.size / Endpointer.RATE.toDouble()
        assertTrue("greeting: $seconds s", seconds in 0.2..1.0)
        assertTrue(s.maxOf { kotlin.math.abs(it) } > 0.3f)
        assertNull(GreetingContext.greeting(CommandLang.EN))
    }

    @Test
    fun theStreamIsSilenceGreetingGapThenTheQuestion() {
        val greeting = FloatArray(100) { 0.25f }
        val question = floatArrayOf(0.04f, -0.04f)
        val ctx = GreetingContext(greeting)
        val s = ctx.stream(question)
        val lead = Endpointer.RATE / 2
        val gap = Endpointer.RATE / 2
        assertEquals(lead + 100 + gap + 2, s.size)
        assertEquals(0f, s[lead - 1])
        assertEquals(0.25f, s[lead])
        assertEquals(0f, s[lead + 100 + gap - 1])
        // The question is brought to half scale.
        assertEquals(0.5f, s[lead + 100 + gap], 1e-6f)
        // Words timed after the middle of the gap are the question's.
        assertEquals((lead + 100 + gap / 2).toFloat() / Endpointer.RATE, ctx.cutSeconds, 1e-6f)
        // Without a greeting: lead silence then the question, nothing dropped.
        val none = GreetingContext(null)
        val plain = none.stream(question)
        assertEquals(lead + 2, plain.size)
        assertEquals(0.04f, plain[lead], 0f)
        assertEquals(0f, none.cutSeconds)
    }
}
