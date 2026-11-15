package fr.skimap.app.voicecmd

import fr.skimap.core.VoiceAnswers
import fr.skimap.voicecmd.Command
import fr.skimap.voicecmd.CommandLang
import fr.skimap.voicecmd.Endpointer
import fr.skimap.voicecmd.EngineLoader
import fr.skimap.voicecmd.Heard
import fr.skimap.voicecmd.ModelInstaller
import fr.skimap.voicecmd.ModelStore
import fr.skimap.voicecmd.SamplesSource
import fr.skimap.voicecmd.VoiceCommands
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * End to end, on this machine (VO5 #87, A11 #98): the spoken questions of the held-out
 * set (Pocket TTS, two voices per language, `crates/intent/examples/command_audio.rs`),
 * with wind mixed in at several signal-to-noise ratios, go through the phone's path:
 * frames of 32 ms, Silero VAD and the end of the question, Parakeet v3 (the model the
 * phone downloads, sherpa-onnx 1.13.8 JNI for Linux), then the core's classifier (Rust).
 * Rates per SNR and language are printed and written to `build/voicecmd-audio.tsv`.
 *
 * Needs the model in `~/.cache/skimap/voicecmd/` and the audio
 * (`-Dskimap.voicecmd.dir=…`); skipped otherwise.
 */
class VoiceCommandAudioTest {
    private val root =
        File(System.getProperty("skimap.voicecmd.dir") ?: "${System.getProperty("user.home")}/.cache/skimap/voicecmd")

    private fun readWav16k(f: File): FloatArray {
        val b = ByteBuffer.wrap(f.readBytes()).order(ByteOrder.LITTLE_ENDIAN)
        // Canonical 44-byte header written by skimap-voice's write_wav (mono, 16-bit).
        assertEquals(16_000, b.getInt(24))
        val n = (b.capacity() - 44) / 2
        return FloatArray(n) { b.getShort(44 + 2 * it) / 32768f }
    }

    private data class Row(
        val file: String,
        val label: String,
        val lang: String,
        val voice: String,
        val snr: String,
        val text: String,
    )

    @Test
    fun spokenQuestionsWithWind() {
        val store = ModelStore(root)
        val index = File(root, "audio/index.tsv")
        assumeTrue("model in ${store.dir}", store.dir.isDirectory)
        assumeTrue("audio in $index", index.isFile)
        // The files the phone downloads, checked as the phone checks them (sha256), in place.
        runBlocking { ModelInstaller(store).install() }
        assertTrue(store.installed)

        val rows =
            index.readLines().drop(1).map { l ->
                val f = l.split('\t')
                Row(f[0], f[1], f[2], f[3], f[4], f[5])
            }
        val only = System.getProperty("skimap.voicecmd.snr") ?: System.getenv("SKIMAP_VOICECMD_SNR")
        val picked = rows.filter { only == null || it.snr == only }
        val t0 = System.nanoTime()
        val engines = EngineLoader.sherpa(store, RustCommandClassifier.hotwords).load()
        val loadMs = (System.nanoTime() - t0) / 1_000_000
        val vc = VoiceCommands(EngineLoader { error("loaded once") }, RustCommandClassifier, {}, { CommandLang.FR })
        val ok = HashMap<String, IntArray>()
        val out = StringBuilder("file\tlabel\tlang\tvoice\tsnr\tgot\theard\tdecode_ms\n")
        var decodeMs = 0L
        var audioS = 0.0
        engines.use { e ->
            for (r in picked) {
                val src = SamplesSource(readWav16k(File(root, "audio/${r.file}")))
                val rec = vc.recognize(e, src::next)
                decodeMs += rec.decodeMs
                audioS += rec.audioSeconds
                val got = classified(rec.text, r.lang)
                count(ok, r, got)
                out.append("${r.file}\t${r.label}\t${r.lang}\t${r.voice}\t${r.snr}\t$got\t${rec.text ?: ""}\t${rec.decodeMs}\n")
            }
        }
        File("build/voicecmd-audio.tsv").writeText(out.toString())
        println("model loaded in $loadMs ms; ${picked.size} questions, $audioS s of audio decoded in $decodeMs ms")
        for (k in ok.keys.sorted()) {
            val (good, n) = ok.getValue(k).let { it[0] to it[1] }
            println("%-22s %4d / %4d = %5.1f %%".format(k, good, n, 100.0 * good / n))
        }
        val clean = ok["clean"]
        if (clean != null) {
            // Regression guard at the rate measured on 2026-11-15 (78.8 % with the bias; see the ticket).
            assertTrue("clean speech: ${clean[0]}/${clean[1]}", clean[0] >= 0.75 * clean[1])
        }
    }

    /** The full on-demand path, three times: load, listen, transcribe, classify, release. */
    @Test
    fun onDemandLoadAndRelease() {
        val store = ModelStore(root)
        val index = File(root, "audio/index.tsv")
        assumeTrue(store.dir.isDirectory && index.isFile)
        runBlocking { ModelInstaller(store).install() }
        val rows =
            index
                .readLines()
                .drop(1)
                .map { it.split('\t') }
                .filter { it[4] == "clean" && it[1] != "unknown" }
        val heard = mutableListOf<Heard>()
        val loader = EngineLoader.sherpa(store, RustCommandClassifier.hotwords)
        val vc = VoiceCommands(loader, RustCommandClassifier, { heard += it }, { CommandLang.FR })
        for (r in rows.take(3)) {
            val o = vc.listen(SamplesSource(readWav16k(File(root, "audio/${r[0]}"))))!!
            println("on demand: « ${o.heard?.text} » -> ${o.heard?.command}, load ${o.loadMs} ms, decode ${o.decodeMs} ms")
            assertTrue("transcribed: ${o.heard}", o.heard != null && o.heard!!.text.isNotBlank())
            assertEquals(Endpointer.State.DONE, o.ended)
        }
        assertEquals(3, heard.size)
    }

    @Test
    fun answersWithoutOuting() {
        VoiceAnswers(null).use { va ->
            val a = CommandAnswers(va, { 47.0 to null })
            val say = { c: Command?, l: CommandLang ->
                a
                    .answer(Heard("x", c, l), null, 0)
                    .said
                    .single()
                    .text
            }
            assertTrue(say(null, CommandLang.FR).startsWith("Je n'ai pas compris. Vous pouvez demander : Où suis-je ?"))
            assertEquals("Batterie à quarante pour cent.", say(Command.BATTERY, CommandLang.FR))
            assertEquals("Aucune sortie en cours.", say(Command.WHERE_AM_I, CommandLang.FR))
            assertEquals("No outing in progress.", say(Command.BRING_ME_BACK, CommandLang.EN))
            assertEquals("Rien à répéter.", say(Command.REPEAT, CommandLang.FR))
            assertTrue(say(Command.REPORT_GROOMED, CommandLang.FR).startsWith("Les signalements ne sont pas encore"))
            val recorded = CommandAnswers(va, { 0.0 to null }, { _, _ -> true })
            assertEquals(
                "Signalement enregistré : trace faite.",
                recorded
                    .answer(Heard("x", Command.REPORT_TRACKS_SET, CommandLang.FR), null, 0)
                    .said
                    .single()
                    .text,
            )
        }
    }

    private fun classified(
        text: String?,
        lang: String,
    ): String {
        val heard = text?.let { RustCommandClassifier.classify(it, if (lang == "fr") CommandLang.FR else CommandLang.EN) }
        return heard?.command?.let(::label) ?: "unknown"
    }

    private fun count(
        ok: HashMap<String, IntArray>,
        r: Row,
        got: String,
    ) {
        val kind = if (r.label == "unknown") "outside" else "commands"
        for (k in listOf(r.snr, "${r.snr}/${r.lang}", "${r.snr}/$kind")) {
            val c = ok.getOrPut(k) { IntArray(2) }
            if (got == r.label) c[0]++
            c[1]++
        }
    }

    private fun label(c: Command) =
        when (c) {
            Command.WHERE_AM_I -> "where"
            Command.TIME_LEFT -> "time_left"
            Command.NEXT_DIRECTION -> "next_direction"
            Command.BRING_ME_BACK -> "bring_back"
            Command.TURNAROUND_TIME -> "turnaround"
            Command.REPORT_TRACKS_SET -> "report_tracks"
            Command.REPORT_GROOMED -> "report_groomed"
            Command.BATTERY -> "battery"
            Command.REPEAT -> "repeat"
        }
}
