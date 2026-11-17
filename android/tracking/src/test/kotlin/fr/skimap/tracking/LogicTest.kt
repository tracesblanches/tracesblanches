package fr.skimap.tracking

import android.view.KeyEvent
import androidx.core.location.LocationRequestCompat
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.Locale

/** The pure logic of the module: GPS requests, packing, gestures, texts, audio assembly, vibrations. */
class LogicTest {
    private fun req(
        mode: GpsMode,
        gnss: Boolean = true,
        interval: Long = 1_000,
        delay: Long = 0,
        motion: Boolean = false,
    ) = GpsRequest(mode, gnss, interval, delay, motion)

    @Test
    fun pocketBatchesAndApproachDoesNot() {
        // Pocket (report 09): a fix every 5 s, batches of 60 s, minimum interval = interval.
        val pocket = GpsPolicy.locationRequest(req(GpsMode.POCKET, interval = 5_000, delay = 60_000))
        assertEquals(5_000, pocket.intervalMillis)
        assertEquals(5_000, pocket.minUpdateIntervalMillis)
        assertEquals(60_000, pocket.maxUpdateDelayMillis)
        assertEquals(LocationRequestCompat.QUALITY_HIGH_ACCURACY, pocket.quality)
        // A batch delay under two intervals is no batch.
        assertEquals(0, GpsPolicy.locationRequest(req(GpsMode.POCKET, interval = 5_000, delay = 7_000)).maxUpdateDelayMillis)
        val approach = GpsPolicy.locationRequest(req(GpsMode.APPROACH))
        assertEquals(1_000, approach.intervalMillis)
        assertEquals(1_000, approach.minUpdateIntervalMillis)
        assertEquals(0, approach.maxUpdateDelayMillis)
        // Pause: GNSS off, passive, one fix a minute at most.
        val pause = GpsPolicy.locationRequest(req(GpsMode.PAUSE, gnss = false, interval = 0, motion = true))
        assertEquals(LocationRequestCompat.PASSIVE_INTERVAL, pause.intervalMillis)
        assertEquals(GpsPolicy.PASSIVE_MIN_INTERVAL_MS, pause.minUpdateIntervalMillis)
    }

    @Test
    fun onlyRealChangesRestartTheChip() {
        val a = req(GpsMode.POCKET, interval = 5_000, delay = 60_000)
        assertTrue(GpsPolicy.changes(null, a))
        // Same request under another mode name: nothing to do.
        assertFalse(GpsPolicy.changes(a, a.copy(mode = GpsMode.INTERACTION)))
        assertTrue(GpsPolicy.changes(a, a.copy(maxDelayMs = 30_000)))
        assertTrue(GpsPolicy.changes(a, req(GpsMode.APPROACH)))
        assertTrue(GpsPolicy.changes(a, req(GpsMode.PAUSE, gnss = false, interval = 0)))
        // Two delays that both mean « no batch » are the same request.
        assertFalse(GpsPolicy.changes(a.copy(maxDelayMs = 0), a.copy(maxDelayMs = 7_000)))
    }

    @Test
    fun fixesArePacked24BytesLittleEndian() {
        val b = FixPacker.pack(longArrayOf(1_768_000_000_123L, 5L), doubleArrayOf(6.7269, 6.1), doubleArrayOf(44.8552, 46.6))
        assertEquals(48, b.size)
        val bb = ByteBuffer.wrap(b).order(ByteOrder.LITTLE_ENDIAN)
        assertEquals(1_768_000_000_123L, bb.getLong(0))
        assertEquals(6.7269, bb.getDouble(8), 0.0)
        assertEquals(44.8552, bb.getDouble(16), 0.0)
        assertEquals(5L, bb.getLong(24))
    }

    @Test
    fun headsetPressSaysBothAndHoldBringsBack() {
        val acts = ArrayList<ButtonAction>()
        val g = ButtonGestures { acts += it }
        g.mediaKey(KeyEvent.ACTION_DOWN, 0, false, 1_000)
        g.mediaKey(KeyEvent.ACTION_UP, 0, false, 1_150)
        assertEquals(listOf(ButtonAction.BOTH), acts)
        acts.clear()
        // Held: repeats, then the long-press flag; « ramène-moi » once, nothing at release.
        g.mediaKey(KeyEvent.ACTION_DOWN, 0, false, 2_000)
        g.mediaKey(KeyEvent.ACTION_DOWN, 1, false, 2_400)
        g.mediaKey(KeyEvent.ACTION_DOWN, 2, true, 2_850)
        g.mediaKey(KeyEvent.ACTION_DOWN, 3, true, 2_900)
        g.mediaKey(KeyEvent.ACTION_UP, 0, false, 3_000)
        assertEquals(listOf(ButtonAction.BRING_ME_BACK), acts)
        acts.clear()
        // A headset without repeats: a long hold seen at release.
        g.mediaKey(KeyEvent.ACTION_DOWN, 0, false, 5_000)
        g.mediaKey(KeyEvent.ACTION_UP, 0, false, 6_000)
        assertEquals(listOf(ButtonAction.BRING_ME_BACK), acts)
    }

    @Test
    fun volumeKeysUpNextDownTimeLeftHoldBringsBack() {
        val acts = ArrayList<ButtonAction>()
        val g = ButtonGestures { acts += it }
        g.volume(1, 0)
        g.flushVolume()
        g.volume(-1, 1_000)
        g.flushVolume()
        assertEquals(listOf(ButtonAction.NEXT_DIRECTION, ButtonAction.TIME_LEFT), acts)
        acts.clear()
        // Held down: key repeats every 50 ms for a second.
        var t = 10_000L
        g.volume(-1, t)
        t += 500
        while (t < 11_000) {
            g.volume(-1, t)
            t += 50
        }
        g.flushVolume()
        assertEquals(listOf(ButtonAction.BRING_ME_BACK), acts)
        acts.clear()
        // Two separate presses up, 700 ms apart: two actions.
        g.volume(1, 20_000)
        g.volume(1, 20_700)
        g.flushVolume()
        assertEquals(listOf(ButtonAction.NEXT_DIRECTION, ButtonAction.NEXT_DIRECTION), acts)
    }

    private fun guidance(
        phase: Phase = Phase.MOVING,
        turn: Turn? = Turn.LEFT,
        dist: Double? = 243.0,
        route: String = "plan",
    ) = Guidance(
        phase = phase,
        route = route,
        label = "Le Laus – Le Bourget",
        remainingM = 8_420.0,
        lengthM = 15_800.0,
        progressM = 7_380.0,
        nextTurn = turn,
        nextDistanceM = dist,
        nextName = "Greffier",
        remainingS = 3_900.0,
        remainingText = "1 h 05",
        arrival = "15 h 20",
        arrivalMs = 0,
        arrivalRange = "15 h 05 à 15 h 40",
        limit = "16 h 30",
        offRouteM = 3.0,
        gpsMode = GpsMode.POCKET,
        proposal = null,
        switch = null,
    )

    @Test
    fun bannerAndNotificationTexts() {
        val fr = FRENCH_LABELS
        assertEquals("80 m", GuidanceText.distance(83.0, Locale.FRANCE))
        assertEquals("250 m", GuidanceText.distance(243.0, Locale.FRANCE))
        assertEquals("1,2 km", GuidanceText.distance(1_240.0, Locale.FRANCE))
        assertEquals("1.2 km", GuidanceText.distance(1_240.0, Locale.ENGLISH))
        assertEquals("12 km", GuidanceText.distance(12_400.0, Locale.FRANCE))
        val g = guidance()
        assertEquals("← 250 m à gauche · Greffier", GuidanceText.headline(g, l = fr))
        assertEquals("1 h 05 restantes · arrivée 15 h 20 (15 h 05 à 15 h 40)", GuidanceText.detail(g, fr))
        assertEquals("250 m", GuidanceText.chip(g, fr))
        assertEquals("Arrivée dans 8,4 km", GuidanceText.headline(guidance(turn = null, dist = null), l = fr))
        assertEquals(
            "Retour : ↗ 250 m légèrement à droite · Greffier",
            GuidanceText.headline(guidance(turn = Turn.SLIGHT_RIGHT, route = "back"), l = fr),
        )
        assertEquals("Pause · 1 h 05 restantes · arrivée 15 h 20 (15 h 05 à 15 h 40)", GuidanceText.detail(guidance(phase = Phase.PAUSED), fr))
        assertEquals("1 h 05", GuidanceText.chip(guidance(phase = Phase.PAUSED), fr))
        assertEquals("1 h 05 · arrivée 15 h 20", GuidanceText.short(g, fr))
        assertEquals("Au refuge · 1 h 05 · arrivée 15 h 20", GuidanceText.short(guidance(phase = Phase.REFUGE), fr))
    }

    @Test
    fun clipsAreAssembledWithTheirPauses() {
        val a = Pcm(ShortArray(4) { 1 }, 48_000, 1)
        val b = Pcm(ShortArray(2) { 2 }, 48_000, 1)
        val out = PcmAssembler.assemble(listOf(a, b), listOf(150, 0))!!
        // 4 samples, 150 ms of silence (7 200 samples), 2 samples.
        assertEquals(4 + 7_200 + 2, out.samples.size)
        assertEquals(1.toShort(), out.samples[3])
        assertEquals(0.toShort(), out.samples[4])
        assertEquals(0.toShort(), out.samples[7_203])
        assertEquals(2.toShort(), out.samples[7_204])
        assertNull(PcmAssembler.assemble(emptyList(), emptyList()))
    }

    @Test
    fun vibrationsAreToldApart() {
        assertArrayEquals(HapticPatterns.SAFETY, HapticPatterns.alert(AlertKind.OFF_ROUTE))
        assertArrayEquals(HapticPatterns.SAFETY, HapticPatterns.alert(AlertKind.TURNAROUND_NOW))
        assertArrayEquals(HapticPatterns.BACK_ON, HapticPatterns.alert(AlertKind.BACK_ON_ROUTE))
        assertArrayEquals(HapticPatterns.LEFT, HapticPatterns.turn(Turn.KEEP_LEFT))
        assertArrayEquals(HapticPatterns.RIGHT, HapticPatterns.turn(Turn.SLIGHT_RIGHT))
        assertNull(HapticPatterns.turn(Turn.STRAIGHT))
        val all = listOf(HapticPatterns.SAFETY, HapticPatterns.BACK_ON, HapticPatterns.LEFT, HapticPatterns.RIGHT, HapticPatterns.U_TURN)
        assertEquals(all.size, all.map { it.toList() }.toSet().size)
    }

    @Test
    fun stepReportsAreTenSecondWindowsWithTheSilences() {
        val r = StepReports()
        r.sample(0, 500)
        r.sample(3_000, 503)
        r.sample(8_000, 509)
        // Nothing closed before every event of the window is known.
        assertTrue(r.drain().isEmpty())
        r.sample(14_000, 515)
        assertEquals(listOf(StepReport(10_000, 9)), r.drain())
        r.close(40_000)
        assertEquals(listOf(StepReport(20_000, 6), StepReport(30_000, 0), StepReport(40_000, 0)), r.drain())
        // A counter that restarted (lower total): counted from there.
        r.sample(42_000, 3)
        r.sample(45_000, 10)
        r.close(50_000)
        assertEquals(listOf(StepReport(50_000, 7)), r.drain())
    }

    @Test
    fun earconsAreShortPannedAndDistinct() {
        fun energy(
            pcm: ShortArray,
            ch: Int,
        ) = (ch until pcm.size step 2).sumOf { pcm[it].toDouble() * pcm[it] }
        val left = EarconSynth.pcm(Earcon.LEFT, 1f)
        val right = EarconSynth.pcm(Earcon.RIGHT, 1f)
        // Left in the left ear, right in the right one.
        assertTrue(energy(left, 0) > 50 * energy(left, 1))
        assertTrue(energy(right, 1) > 50 * energy(right, 0))
        for (e in Earcon.entries) {
            val pcm = EarconSynth.pcm(e, 1f)
            val ms = pcm.size / 2 * 1000 / EarconSynth.RATE
            assertTrue("$e: $ms ms", ms in 100..500)
            // Discreet: the peak stays under a quarter of the full scale.
            assertTrue(pcm.maxOf { kotlin.math.abs(it.toInt()) } < Short.MAX_VALUE / 4)
            // Volume scales it; 0 is silence.
            assertTrue(EarconSynth.pcm(e, 0f).all { it.toInt() == 0 })
        }
        assertEquals(Earcon.LEFT, EarconSynth.turn(Turn.KEEP_LEFT))
        assertEquals(Earcon.RIGHT, EarconSynth.turn(Turn.SLIGHT_RIGHT))
        assertNull(EarconSynth.turn(Turn.STRAIGHT))
        assertEquals(Earcon.OFF_ROUTE, EarconSynth.alert(AlertKind.OFF_ROUTE))
        assertEquals(Earcon.REASSURANCE, EarconSynth.alert(AlertKind.BACK_ON_ROUTE))
        assertNull(EarconSynth.alert(AlertKind.FOG))
    }
}
