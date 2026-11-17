package fr.skimap.app.community

import fr.skimap.app.day.DayStore
import fr.skimap.community.CommunityApi
import fr.skimap.community.ReportKind
import fr.skimap.community.ReportOutbox
import fr.skimap.community.ReportUploader
import fr.skimap.core.Lang
import fr.skimap.core.challengesCompute
import fr.skimap.core.osmNoteDraft
import fr.skimap.core.outingHistory
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.ServerSocket
import java.net.URI

/**
 * The module :community against the real skimap-server (task cargoServer) and the real
 * core (host library): a report made offline sent when the server comes back with the
 * time it was seen, listed, confirmed by another phone; challenges and the OSM note through
 * the core; the Strava imports in the app's history of the outings (one history).
 */
class CommunityServerTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private var server: Process? = null
    private val port = ServerSocket(0).use { it.localPort }
    private val base = "http://127.0.0.1:$port"

    private fun startServer() {
        server =
            ProcessBuilder(checkNotNull(System.getProperty("skimap.server")), "serve", "--addr", "127.0.0.1:$port")
                .redirectErrorStream(true)
                .redirectOutput(File(tmp.root, "server.log"))
                .apply {
                    environment()["SKIMAP_STORE"] = tmp.newFolder("store").absolutePath
                    environment()["SKIMAP_SERVER_DB"] = File(tmp.root, "server.sqlite").absolutePath
                    environment()["SKIMAP_COLLAB_DB"] = File(tmp.root, "collab.sqlite").absolutePath
                    environment()["SKIMAP_CONTACT_EMAIL"] = "test@example.org"
                }.start()
        val deadline = System.currentTimeMillis() + STARTUP_MS
        while (true) {
            val ok =
                try {
                    (URI("$base/v1/collab/rules").toURL().openConnection() as HttpURLConnection).run {
                        connectTimeout = 200
                        responseCode.also { disconnect() } == 200
                    }
                } catch (_: IOException) {
                    false
                }
            if (ok) break
            check(server!!.isAlive && System.currentTimeMillis() < deadline) { File(tmp.root, "server.log").readText() }
            Thread.sleep(POLL_MS)
        }
    }

    @After
    fun stop() {
        server?.destroy()
        server?.waitFor()
    }

    @Test
    fun offlineReportSentWhenTheServerIsBackThenConfirmed() {
        val outbox = ReportOutbox(tmp.newFolder("outbox"))
        val seen = System.currentTimeMillis() - 2 * 3_600_000
        outbox.add(ReportKind.WIND_BLOWN, 6.7712, 44.8456, seen)
        val a = "0f1e2d3c4b5a69788796a5b4c3d2e1f0"
        // Server down: nothing sent, still queued.
        assertTrue(ReportUploader(outbox, CommunityApi(base), { a }).drain().retry)
        assertEquals(1, outbox.size())
        startServer()
        val r = ReportUploader(outbox, CommunityApi(base), { a }).drain()
        assertEquals(ReportUploader.Result(1, 0, false), r)
        val listed = CommunityApi(base).list(doubleArrayOf(6.66, 44.77, 6.86, 44.92))
        assertEquals(1, listed.size)
        assertEquals(ReportKind.WIND_BLOWN, listed[0].kind)
        assertEquals(seen / 1000, listed[0].seenAtMs / 1000)
        assertEquals(seen / 1000 + 24 * 3600, listed[0].expiresAtMs / 1000)
        // Another phone confirms; the author cannot.
        assertEquals(null, CommunityApi(base).vote(listed[0].id, a, true))
        val v = CommunityApi(base).vote(listed[0].id, "1f1e2d3c4b5a69788796a5b4c3d2e1f0", true)
        assertEquals(1, v?.confirmations)
        assertTrue(CommunityApi(base).delete(listed[0].id, a))
    }

    @Test
    fun outingsChallengesAndOsmNoteThroughTheCore() {
        val plan = File(tmp.newFolder("plans"), "2026-01-10-1").also { it.mkdirs() }
        val gpx =
            buildString {
                append("<gpx><trk><trkseg>")
                for (i in 0 until 600) {
                    append("<trkpt lat=\"${44.85 + i * 5.0 / 111_195.0}\" lon=\"6.77\"><time>2026-01-10T09:")
                    append("%02d:%02dZ</time></trkpt>".format(i * 2 / 60, i * 2 % 60))
                }
                append("</trkseg></trk></gpx>")
            }
        File(plan, "trace.gpx").writeText(gpx)
        // The challenges read the app's history of the outings: no list of their own.
        val roots = listOf(plan.parentFile!!.absolutePath)
        val routes = """{"routes":[{"id":"r","name":"R","publisher":"P","licence":"L","line":[[6.77,44.85],[6.77,44.877]]}]}"""
        val c = challengesCompute(roots, routes, emptyList(), 1_768_100_000_000)
        assertEquals("r", c[0].id)
        assertEquals(1.0, c[0].share, 0.01)
        assertEquals("2026-01-10", c[0].doneOn)
        val n = osmNoteDraft("wrong_name", "Les Fonts", 42L, "Panneau : Fonts", 6.77, 44.85, Lang.FR)
        assertTrue(n.text.startsWith("Le nom de cette piste est faux.\nPiste : « Les Fonts »"))
        assertTrue(n.formBody.startsWith("lat=44.85&lon=6.77&text="))
    }

    @Test
    fun stravaImportsAreInTheAppHistory() {
        val files = tmp.newFolder("files")
        // Imported before the history was the only storage: moved into it once.
        val old = File(files, "outings/strava-103").also { it.mkdirs() }
        File(old, "outing.json").writeText(
            """{"id":"strava-103","source":"strava","name":"Risoux","activity":"Nordic Ski",
            "start":"2026-01-10T09:00:00Z","elapsed_s":8400.0,"moving_s":7920.0,"distance_m":21400.0,
            "gain_m":310.0,"track":[[6.0,46.6],[6.01,46.61]]}""",
        )
        val core =
            RustCommunityCore(
                { tmp.newFolder("packs") },
                { files },
                { "{}" },
                DayStoreLearning(DayStore({ error("no preferences in this test") }, { files })),
            )
        val root = core.outingsRoot()
        assertEquals(File(files, "plans"), root)
        assertTrue(File(root, "strava-103/outing.json").isFile)
        assertTrue(!File(files, "outings").exists())
        val h = outingHistory(root.absolutePath, Lang.FR)
        assertEquals(1, h.size)
        assertEquals("strava", h[0].kind)
        assertEquals("Risoux", h[0].title)
        assertEquals(21.4, h[0].distanceKm, 1e-9)
    }

    private companion object {
        const val STARTUP_MS = 10_000L
        const val POLL_MS = 50L
    }
}
