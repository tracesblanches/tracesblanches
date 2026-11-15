package fr.skimap.sync

import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpServer
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.IOException
import java.net.InetSocketAddress
import kotlin.random.Random

/**
 * [HttpSource] against a small server that behaves like skimap-server and Cloudflare:
 * ETag and 304, one byte range with If-Range, a 200 to a range request, 416, and a
 * connection cut in the middle of a body.
 */
class HttpSourceTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val body = Random(7).nextBytes(1_000_000)
    private val sha = "abc123"
    private lateinit var server: HttpServer

    /** Cut the next body after this many bytes (connection closed). */
    @Volatile
    private var cutAfter: Int? = null

    /** Answer the next range request with the whole file (a cache miss at Cloudflare). */
    @Volatile
    private var ignoreRange = false
    private val seen = mutableListOf<String>()

    private fun send(
        x: HttpExchange,
        status: Int,
        bytes: ByteArray,
        from: Int = 0,
    ) {
        val cut = cutAfter
        cutAfter = null
        x.sendResponseHeaders(status, bytes.size.toLong() - from)
        x.responseBody.use { out ->
            if (cut != null) {
                out.write(bytes, from, cut)
                out.flush()
                // Closing the exchange early: the client sees a truncated body.
                x.close()
                return
            }
            out.write(bytes, from, bytes.size - from)
        }
    }

    @Before
    fun start() {
        server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/v1/sites/test/index.json") { x ->
            seen += "index ${x.requestHeaders.getFirst("If-None-Match")}"
            x.responseHeaders.add("ETag", "\"v2\"")
            if (x.requestHeaders.getFirst("If-None-Match") == "\"v2\"") {
                x.sendResponseHeaders(304, -1)
                x.close()
            } else {
                send(x, 200, "{\"latest\": 2}".toByteArray())
            }
        }
        server.createContext("/v1/bundles/") { x ->
            val range = x.requestHeaders.getFirst("Range")
            val ifRange = x.requestHeaders.getFirst("If-Range")
            seen += "bundle $range $ifRange ${x.requestHeaders.getFirst("Accept-Encoding")}"
            x.responseHeaders.add("ETag", "\"$sha\"")
            val start = range?.removePrefix("bytes=")?.removeSuffix("-")?.toInt()
            when {
                start == null || ignoreRange || ifRange != "\"$sha\"" -> {
                    ignoreRange = false
                    send(x, 200, body)
                }

                start >= body.size -> {
                    x.sendResponseHeaders(416, -1)
                    x.close()
                }

                else -> {
                    x.responseHeaders.add("Content-Range", "bytes $start-${body.size - 1}/${body.size}")
                    send(x, 206, body, start)
                }
            }
        }
        server.createContext("/missing") { x ->
            x.sendResponseHeaders(404, -1)
            x.close()
        }
        server.start()
    }

    @After
    fun stop() = server.stop(0)

    private fun source() = HttpSource("http://127.0.0.1:${server.address.port}", "skimap-test")

    @Test
    fun revalidation() {
        val s = source()
        val first = s.fetch("/v1/sites/test/index.json", null) as Fetched.Body
        assertEquals("\"v2\"", first.etag)
        assertEquals("{\"latest\": 2}", first.bytes.decodeToString())
        assertEquals(Fetched.NotModified, s.fetch("/v1/sites/test/index.json", first.etag))
        assertEquals(listOf("index null", "index \"v2\""), seen)
        try {
            s.fetch("/missing", null)
            fail("404 accepted")
        } catch (e: HttpStatusException) {
            assertEquals(404, e.status)
        }
    }

    @Test
    fun downloadResumesAfterACut() {
        val s = source()
        val dest = tmp.root.resolve("b.part")
        cutAfter = 300_000
        try {
            s.download("/v1/bundles/$sha.skb", dest, sha, body.size.toLong())
            fail("a cut body was accepted")
        } catch (_: IOException) {
        }
        val kept = dest.length()
        assertTrue("$kept bytes kept", kept in 1..300_000)
        val progress = mutableListOf<Long>()
        val d = s.download("/v1/bundles/$sha.skb", dest, sha, body.size.toLong()) { n, _ -> progress += n }
        assertEquals(kept, d.resumedFrom)
        assertEquals(body.size - kept, d.received)
        assertArrayEquals(body, dest.readBytes())
        assertEquals(body.size.toLong(), progress.last())
        assertEquals("bundle bytes=$kept- \"$sha\" identity", seen.last())
        // Complete already: nothing requested.
        val n = s.requests
        assertEquals(Downloaded(0, body.size.toLong()), s.download("/v1/bundles/$sha.skb", dest, sha, body.size.toLong()))
        assertEquals(n, s.requests)
    }

    @Test
    fun wholeFileAnswerToARangeStartsAgain() {
        val s = source()
        val dest = tmp.root.resolve("b.part")
        dest.writeBytes(ByteArray(1000) { 9 })
        ignoreRange = true
        val d = s.download("/v1/bundles/$sha.skb", dest, sha, body.size.toLong())
        assertEquals(0L, d.resumedFrom)
        assertArrayEquals(body, dest.readBytes())
        // A partial file longer than the bundle: discarded, downloaded again.
        dest.writeBytes(ByteArray(body.size + 5))
        s.download("/v1/bundles/$sha.skb", dest, sha, body.size.toLong())
        assertArrayEquals(body, dest.readBytes())
        assertEquals(-1L, HttpSource.contentRangeStart(null))
        assertEquals(42L, HttpSource.contentRangeStart("bytes 42-99/100"))
    }
}
