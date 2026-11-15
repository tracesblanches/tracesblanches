package fr.skimap.community

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Color
import androidx.test.core.app.ApplicationProvider
import androidx.work.ListenableWorker
import androidx.work.testing.TestListenableWorkerBuilder
import com.sun.net.httpserver.HttpServer
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import java.io.File
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.time.Instant
import java.util.concurrent.CopyOnWriteArrayList

/** A server answering like skimap-server's /v1/collab/ (status chosen by the test). */
class FakeCollab {
    val bodies = CopyOnWriteArrayList<String>()
    val photos = CopyOnWriteArrayList<ByteArray>()

    @Volatile
    var status = 201
    private val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
    val url get() = "http://127.0.0.1:${server.address.port}"

    init {
        server.createContext("/v1/collab/reports") { ex ->
            val body = ex.requestBody.readBytes()
            val (code, out) =
                when {
                    ex.requestURI.path.endsWith("/photo") -> {
                        photos.add(body)
                        204 to ""
                    }

                    ex.requestMethod == "POST" -> {
                        bodies.add(body.decodeToString())
                        status to if (status == 201) """{"id":"0123456789abcdef0123"}""" else """{"error":"x"}"""
                    }

                    else -> {
                        404 to ""
                    }
                }
            val bytes = out.toByteArray()
            ex.sendResponseHeaders(code, if (bytes.isEmpty()) -1 else bytes.size.toLong())
            if (bytes.isNotEmpty()) ex.responseBody.use { it.write(bytes) }
            ex.close()
        }
        server.start()
    }

    fun stop() = server.stop(0)
}

@RunWith(RobolectricTestRunner::class)
class QueueTest {
    private val context: Context = ApplicationProvider.getApplicationContext()
    private lateinit var dir: File
    private lateinit var server: FakeCollab

    @Before
    fun setUp() {
        dir = File(context.cacheDir, "outbox-test").also { it.deleteRecursively() }
        server = FakeCollab()
    }

    @After
    fun tearDown() = server.stop()

    private fun closedPort(): String = ServerSocket(0).use { "http://127.0.0.1:${it.localPort}" }

    @Test
    fun reportMadeOfflineIsSentWithTheRealTimeWhenTheNetworkComesBack() {
        val outbox = ReportOutbox(dir)
        val seen = Instant.parse("2026-01-10T09:12:00Z").toEpochMilli()
        val q = outbox.add(ReportKind.ICE, 6.7712, 44.8456, seen)
        assertTrue(outbox.attachPhoto(q.localId, byteArrayOf(1, 2, 3)))
        // No network (nothing listens): stays in the queue, retry.
        val offline = ReportUploader(outbox, CommunityApi(closedPort()), { "a".repeat(32) }).drain()
        assertEquals(ReportUploader.Result(0, 0, true), offline)
        assertEquals(1, ReportOutbox(dir).size())
        // Network back, an hour later: sent with the time it was seen, then its photo.
        val r = ReportUploader(ReportOutbox(dir), CommunityApi(server.url), { "a".repeat(32) }).drain()
        assertEquals(ReportUploader.Result(1, 0, false), r)
        val sent = JSONObject(server.bodies.single())
        assertEquals("2026-01-10T09:12:00Z", sent.getString("seen_at"))
        assertEquals("ice", sent.getString("kind"))
        assertEquals(3, server.photos.single().size)
        assertEquals(0, outbox.size())
    }

    @Test
    fun expiredOnArrivalIsDroppedServerErrorsAreRetried() {
        val outbox = ReportOutbox(dir)
        outbox.add(ReportKind.GROOMED, 6.0, 46.0, 0L)
        server.status = 503
        assertTrue(ReportUploader(outbox, CommunityApi(server.url), { "b".repeat(32) }).drain().retry)
        assertEquals(1, outbox.size())
        server.status = 410
        assertEquals(ReportUploader.Result(0, 1, false), ReportUploader(outbox, CommunityApi(server.url), { "b".repeat(32) }).drain())
        assertEquals(0, outbox.size())
    }

    @Test
    fun workerRetriesWithoutNetworkAndSucceedsAfter() {
        val prefs = context.getSharedPreferences("skimap", Context.MODE_PRIVATE)
        prefs.edit().putString("server_url", closedPort()).commit()
        ReportSync.outbox(context).add(ReportKind.OBSTACLE, 6.77, 44.85, System.currentTimeMillis())
        val w = TestListenableWorkerBuilder<ReportUploadWorker>(context).build()
        assertEquals(ListenableWorker.Result.retry(), runBlocking { w.doWork() })
        prefs.edit().putString("server_url", server.url).commit()
        val w2 = TestListenableWorkerBuilder<ReportUploadWorker>(context).build()
        assertEquals(ListenableWorker.Result.success(), runBlocking { w2.doWork() })
        assertEquals(0, ReportSync.outbox(context).size())
        assertTrue("0123456789abcdef0123" in ReportSync.mine(context).all())
    }

    @Test
    fun deviceIdIsRandomAndRotatesEveryThirtyDays() {
        var now = 0L
        val prefs = context.getSharedPreferences("device-test", Context.MODE_PRIVATE)
        val d = DeviceId({ prefs }, { now })
        val a = d.current()
        assertEquals(32, a.length)
        assertEquals(a, d.current())
        now += DeviceId.ROTATION_MS - 1
        assertEquals(a, d.current())
        now += 1
        assertNotEquals(a, d.current())
    }

    @Test
    fun photoIsReducedWithoutExif() {
        val big = Bitmap.createBitmap(4000, 3000, Bitmap.Config.ARGB_8888)
        for (x in 0 until 4000 step 7) for (y in 0 until 3000 step 5) big.setPixel(x, y, Color.rgb(x % 255, y % 255, (x * y) % 255))
        val jpeg = PhotoReducer.reduce(big)
        assertTrue(jpeg.size <= PhotoReducer.MAX_BYTES)
        assertEquals(0xFF.toByte(), jpeg[0])
        assertEquals(0xD8.toByte(), jpeg[1])
        assertFalse(String(jpeg, Charsets.ISO_8859_1).contains("Exif"))
    }

    @Test
    fun osmNoteSentToTheNotesApiOfTheServerGiven() {
        val osm = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        val got = CopyOnWriteArrayList<String>()
        osm.createContext("/api/0.6/notes.json") { ex ->
            got.add(ex.requestMethod + " " + ex.requestHeaders.getFirst("Content-Type") + " " + ex.requestBody.readBytes().decodeToString())
            val b = """{"type":"Feature","properties":{"id":4242,"status":"open"}}""".toByteArray()
            ex.sendResponseHeaders(200, b.size.toLong())
            ex.responseBody.use { it.write(b) }
        }
        osm.start()
        try {
            val base = "http://127.0.0.1:${osm.address.port}"
            val note = OsmNote(44.85, 6.77, "Nom faux", "lat=44.85&lon=6.77&text=Nom%20faux")
            assertEquals("$base/note/4242", OsmNotes(base).send(note).getOrThrow())
            assertEquals("POST application/x-www-form-urlencoded lat=44.85&lon=6.77&text=Nom%20faux", got.single())
            assertEquals("https://www.openstreetmap.org", OsmNotes.webUrl(OsmNotes.PRODUCTION))
        } finally {
            osm.stop(0)
        }
    }
}
