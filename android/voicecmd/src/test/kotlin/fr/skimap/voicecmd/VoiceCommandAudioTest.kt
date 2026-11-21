package fr.skimap.voicecmd

import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File

/**
 * End to end on this machine, the phone's path (VO5 #87): the spoken questions of the
 * held-out set (`crates/intent/examples/command_audio.rs`, Pocket TTS, synthetic wind) in
 * frames of 32 ms, Silero VAD and the end of the question, Parakeet v3 (the files the
 * phone downloads, sherpa-onnx 1.13.8 JNI for Linux). What was heard goes to
 * `build/voicecmd-heard-<config>.tsv`, rated by the core's classifier with
 * `cargo run --release -p skimap-intent --example rate_heard -- <file>` (this module has
 * no Rust bindings).
 *
 * Two configurations: `before` (as on 15/11: hotwords of both languages, no greeting, no
 * level) and `after` (French: French hotwords, greeting, [Level]; English: unchanged).
 * `-Dskimap.voicecmd.snr=clean,snr+0` and `-Dskimap.voicecmd.lang=fr` pick the rows
 * (default: clean, French). Minutes of decoding: runs only with
 * `-Dskimap.voicecmd.audio=1`
 * (`./gradlew :voicecmd:testDebugUnitTest --tests '*VoiceCommandAudioTest*' -Dskimap.voicecmd.audio=1`).
 * Needs the model in `~/.cache/skimap/voicecmd/` and its audio;
 * skipped otherwise.
 */
class VoiceCommandAudioTest {
    private val root =
        File(System.getProperty("skimap.voicecmd.dir") ?: "${System.getProperty("user.home")}/.cache/skimap/voicecmd")

    private fun questions(lang: CommandLang) =
        if (lang == CommandLang.FR) {
            listOf(
                "Où suis-je",
                "Temps restant",
                "Prochaine direction",
                "Ramène-moi",
                "Heure de demi-tour",
                "Signale trace faite",
                "Signale piste damée",
                "Combien de batterie",
                "Répète",
            )
        } else {
            listOf(
                "Where am I",
                "Time left",
                "Next direction",
                "Take me back",
                "Turnaround time",
                "Report tracks set",
                "Report trail groomed",
                "How much battery",
                "Repeat",
            )
        }

    @Test
    fun spokenQuestionsBeforeAndAfter() {
        assumeTrue("-Dskimap.voicecmd.audio=1", System.getProperty("skimap.voicecmd.audio") == "1")
        val store = ModelStore(root)
        val index = File(root, "audio/index.tsv")
        assumeTrue("model in ${store.dir}", store.installed)
        assumeTrue("audio in $index", index.isFile)
        val snrs = (System.getProperty("skimap.voicecmd.snr") ?: "clean").split(',')
        val langs = (System.getProperty("skimap.voicecmd.lang") ?: "fr").split(',')
        val rows =
            index
                .readLines()
                .drop(1)
                .map { it.split('\t') }
                .filter { it[4] in snrs && it[2] in langs }
        val configs = (System.getProperty("skimap.voicecmd.config") ?: "before,after").split(',')
        for (config in configs) {
            for (lang in langs) measure(store, rows.filter { it[2] == lang }, config, lang)
        }
    }

    private fun measure(
        store: ModelStore,
        mine: List<List<String>>,
        config: String,
        lang: String,
    ) {
        val l = if (lang == "fr") CommandLang.FR else CommandLang.EN
        val both = questions(CommandLang.FR) + questions(CommandLang.EN)
        // English: no greeting, so the path of 15/11 (both languages' hotwords).
        val hot = if (config == "before" || l == CommandLang.EN) both else questions(l)
        val loader = if (config == "before") EngineLoader.sherpa(store, hot) else EngineLoader.sherpa(store, hot, l)
        val vc = VoiceCommands(loader, { t, d -> Heard(t, null, d) }, {}, { l })
        val out = StringBuilder()
        var decodeMs = 0L
        var audioS = 0.0
        loader.load().use { e ->
            for (r in mine) {
                val src = SamplesSource(GreetingContext.readWav(File(root, "audio/${r[0]}").inputStream()))
                val rec = vc.recognize(e, src::next)
                decodeMs += rec.decodeMs
                audioS += rec.audioSeconds
                out.append(r.take(5).joinToString("\t")).append('\t')
                out
                    .append(rec.text.orEmpty().replace('\t', ' '))
                    .append('\t')
                    .append(rec.decodeMs)
                    .append('\n')
            }
        }
        val f = File("build/voicecmd-heard-$config-$lang.tsv")
        f.writeText("file\tlabel\tlang\tvoice\tsnr\theard\tdecode_ms\n$out")
        println("$config/$lang: ${mine.size} questions, $audioS s of speech decoded in $decodeMs ms -> $f")
    }
}
