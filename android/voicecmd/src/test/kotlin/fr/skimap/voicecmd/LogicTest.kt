package fr.skimap.voicecmd

import com.sun.net.httpserver.HttpServer
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.File
import java.io.IOException
import java.net.InetSocketAddress
import java.nio.file.Files
import java.security.MessageDigest
import java.util.concurrent.atomic.AtomicInteger

class LogicTest {
    private fun frame(v: Float) = FloatArray(Endpointer.FRAME) { v }

    @Test
    fun endpointerKeepsPreRollAndEndsAfterSilence() {
        val ep = Endpointer()
        repeat(30) { ep.push(frame(0f), 0.1f) }
        assertEquals(Endpointer.State.WAITING, ep.state)
        repeat(20) { ep.push(frame(1f), 0.9f) }
        assertEquals(Endpointer.State.SPEECH, ep.state)
        // A short pause inside the question does not end it.
        repeat(10) { ep.push(frame(0f), 0.1f) }
        repeat(5) { ep.push(frame(1f), 0.9f) }
        assertEquals(Endpointer.State.SPEECH, ep.state)
        var n = 0
        while (ep.push(frame(0f), 0.1f) == Endpointer.State.SPEECH) n++
        assertEquals(Endpointer.State.DONE, ep.state)
        assertEquals(Endpointer.ms(700) - 1, n)
        val s = ep.samples()
        val pre = Endpointer.ms(300)
        // Pre-roll of silence, then the speech, then the pauses.
        assertEquals((pre + 20 + 10 + 5 + Endpointer.ms(700)) * Endpointer.FRAME, s.size)
        assertEquals(0f, s[0])
        assertEquals(1f, s[pre * Endpointer.FRAME])
    }

    @Test
    fun endpointerGivesUpWithoutSpeechAndCutsLongQuestions() {
        val quiet = Endpointer()
        repeat(Endpointer.ms(5000)) { quiet.push(frame(0f), 0.2f) }
        assertEquals(Endpointer.State.NOTHING, quiet.state)
        assertEquals(0, quiet.samples().size)
        val long = Endpointer()
        var k = 0
        while (long.push(frame(1f), 0.9f) != Endpointer.State.DONE) k++
        assertTrue("cut at 8 s: $k", k in Endpointer.ms(8000) - 1..Endpointer.ms(8000) + 3)
    }

    @Test
    fun bpeVocabInIdOrderAndCheckedAgainstTokens() {
        val d = Files.createTempDirectory("bpe").toFile()
        val tj = File(d, "tokenizer.json").apply { writeText("""{"model":{"vocab":{"<unk>":0,"▁a":2,"b":1}}}""") }
        val tokens = File(d, "tokens.txt").apply { writeText("<unk> 0\nb 1\n▁a 2\n<blk> 3\n") }
        val out = File(d, "bpe.vocab")
        BpeVocab.write(tj, tokens, out)
        assertEquals("<unk>\t0.0\nb\t-1.0\n▁a\t-2.0\n", out.readText())
        tokens.writeText("<unk> 0\nb 2\n▁a 1\n")
        try {
            BpeVocab.write(tj, tokens, out)
            fail("disagreeing tokens accepted")
        } catch (e: IOException) {
            assertTrue(e.message!!.contains("disagree"))
        }
    }

    @Test
    fun specSizes() {
        assertEquals(674_064_268L, ModelSpec.PARAKEET_V3_INT8.totalBytes)
        assertEquals(6, ModelSpec.PARAKEET_V3_INT8.files.size)
    }

    private fun sha(b: ByteArray) = MessageDigest.getInstance("SHA-256").digest(b).joinToString("") { "%02x".format(it) }

    /** A server of two files with Range support; counts the requests and the bytes sent. */
    private class Server(
        val files: Map<String, ByteArray>,
    ) : AutoCloseable {
        val requests = AtomicInteger()
        val sent = AtomicInteger()
        val ranges = mutableListOf<String?>()
        val http: HttpServer = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)

        init {
            http.createContext("/") { ex ->
                requests.incrementAndGet()
                val body = files[ex.requestURI.path.removePrefix("/")]
                val range = ex.requestHeaders.getFirst("Range")
                synchronized(ranges) { ranges += range }
                if (body == null) {
                    ex.sendResponseHeaders(404, -1)
                } else if (range != null) {
                    val from = range.removePrefix("bytes=").removeSuffix("-").toInt()
                    ex.sendResponseHeaders(206, (body.size - from).toLong())
                    ex.responseBody.use { it.write(body, from, body.size - from) }
                    sent.addAndGet(body.size - from)
                } else {
                    ex.sendResponseHeaders(200, body.size.toLong())
                    ex.responseBody.use { it.write(body) }
                    sent.addAndGet(body.size)
                }
                ex.close()
            }
            http.start()
        }

        val base get() = "http://127.0.0.1:${http.address.port}"

        override fun close() = http.stop(0)
    }

    @Test
    fun installerResumesChecksAndMarks() {
        val a = ByteArray(300_000) { (it * 7).toByte() }
        val b = ByteArray(1234) { (it * 3).toByte() }
        Server(mapOf("a.onnx" to a, "b.txt" to b)).use { srv ->
            val spec =
                ModelSpec(
                    "m",
                    listOf(
                        ModelFile("a.onnx", "${srv.base}/a.onnx", a.size.toLong(), sha(a)),
                        ModelFile("b.txt", "${srv.base}/b.txt", b.size.toLong(), sha(b)),
                    ),
                )
            val root = Files.createTempDirectory("voicecmd").toFile()
            val store = ModelStore(root, spec)
            assertFalse(store.installed)
            // A download cut at 100 000 bytes resumes there.
            store.dir.mkdirs()
            File(store.dir, "a.onnx.part").writeBytes(a.copyOf(100_000))
            var last = 0L
            runBlocking {
                ModelInstaller(store).install { done, total ->
                    last = done
                    assertEquals(spec.totalBytes, total)
                }
            }
            assertTrue(store.installed)
            assertEquals(spec.totalBytes, last)
            assertArrayEquals(a, store.file("a.onnx").readBytes())
            assertEquals(listOf("bytes=100000-", null), srv.ranges)
            assertEquals(200_000 + b.size, srv.sent.get())
            // Installed: nothing fetched again.
            runBlocking { ModelInstaller(store).install() }
            assertEquals(2, srv.requests.get())
            // A file cut by the system is no longer installed.
            store.file("b.txt").writeBytes(b.copyOf(10))
            assertFalse(store.installed)
            store.delete()
            assertEquals(0L, store.bytesOnDisk)
        }
    }

    @Test
    fun installerRejectsWrongContent() {
        val a = ByteArray(5000) { 1 }
        Server(mapOf("a.onnx" to a)).use { srv ->
            val spec = ModelSpec("m", listOf(ModelFile("a.onnx", "${srv.base}/a.onnx", a.size.toLong(), "00".repeat(32))))
            val store = ModelStore(Files.createTempDirectory("voicecmd").toFile(), spec)
            try {
                runBlocking { ModelInstaller(store).install() }
                fail("wrong sha256 accepted")
            } catch (e: IOException) {
                assertTrue(e.message!!.contains("sha256"))
            }
            assertFalse(store.installed)
            assertFalse(store.file("a.onnx").exists())
            assertFalse(File(store.dir, "a.onnx.part").exists())
        }
    }

    /** Engines that hear energy and write what they were told. */
    private class FakeEngines(
        val text: String,
    ) {
        var loads = 0
        var closed = 0

        fun loader() =
            EngineLoader {
                loads++
                Engines(
                    object : VoiceActivity {
                        override fun probability(frame: FloatArray) = if (frame.any { it != 0f }) 0.9f else 0.05f

                        override fun reset() = Unit

                        override fun close() {
                            closed++
                        }
                    },
                    object : Transcriber {
                        override fun transcribe(samples: FloatArray) = text

                        override fun close() {
                            closed++
                        }
                    },
                )
            }
    }

    @Test
    fun oneCommandLoadsTranscribesClassifiesAndReleases() {
        val fake = FakeEngines("Où suis-je ?")
        val handled = mutableListOf<Heard>()
        val vc =
            VoiceCommands(
                fake.loader(),
                { t, l -> Heard(t, if (t.startsWith("Où")) Command.WHERE_AM_I else null, l) },
                { handled += it },
                { CommandLang.FR },
            )
        val speech = FloatArray(Endpointer.RATE) { 0.3f }
        val out = vc.listen(SamplesSource(FloatArray(Endpointer.RATE / 2) + speech))!!
        assertEquals(Command.WHERE_AM_I, out.heard?.command)
        assertEquals(Endpointer.State.DONE, out.ended)
        assertEquals(listOf(Heard("Où suis-je ?", Command.WHERE_AM_I, CommandLang.FR)), handled)
        // Both engines released after the command (battery).
        assertEquals(1, fake.loads)
        assertEquals(2, fake.closed)
        // 300 ms of pre-roll + 1 s of speech + 700 ms of silence.
        assertEquals(2.0, out.audioSeconds, 0.05)
    }

    @Test
    fun silenceIsNoCommand() {
        val fake = FakeEngines("x")
        val handled = mutableListOf<Heard>()
        val vc = VoiceCommands(fake.loader(), { t, l -> Heard(t, null, l) }, { handled += it }, { CommandLang.EN })
        val out = vc.listen(SamplesSource(FloatArray(0), silenceAfter = Endpointer.ms(6000)))!!
        assertNull(out.heard)
        assertEquals(Endpointer.State.NOTHING, out.ended)
        assertTrue(handled.isEmpty())
        assertEquals(2, fake.closed)
    }
}
