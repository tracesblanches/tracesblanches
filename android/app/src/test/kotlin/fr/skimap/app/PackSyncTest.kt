package fr.skimap.app

import fr.skimap.app.sync.FranceSyncer
import fr.skimap.app.sync.PackSyncer
import fr.skimap.app.sync.ServerCatalog
import fr.skimap.app.sync.SyncTarget
import fr.skimap.core.Lang
import fr.skimap.core.listPacks
import fr.skimap.core.openMap
import fr.skimap.core.openPacks
import fr.skimap.core.syncState
import fr.skimap.sync.HttpSource
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.ServerSocket
import java.net.URI
import java.security.MessageDigest

/**
 * Download and update of packs (K2) on the JVM against the real `skimap-server` (built by
 * Gradle, task `cargoServer`) serving a store where the test pack was published at
 * version 1 then 2 (task `testStore`), with the real Rust core installing the bundles:
 * first download, sync without change (304), delta, resume after a cut, a corrupt partial
 * file, a phone that lost contents, a basemap that does not rebuild from its patch of
 * tiles (#160), the server unreachable, and a voice pack apart (VO6).
 */
class PackSyncTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private lateinit var server: Process
    private lateinit var base: String
    private lateinit var store: File

    private fun pack(name: String) = File(tmp.root, "phone/packs/$name")

    @Before
    fun setUp() {
        store = tmp.newFolder("store")
        File(checkNotNull(System.getProperty("skimap.store"))).copyRecursively(store, overwrite = true)
        startServer()
    }

    private fun startServer() {
        val port = ServerSocket(0).use { it.localPort }
        base = "http://127.0.0.1:$port"
        server =
            ProcessBuilder(checkNotNull(System.getProperty("skimap.server")), "serve", "--addr", "127.0.0.1:$port")
                .redirectErrorStream(true)
                .redirectOutput(File(tmp.root, "server.log"))
                .apply {
                    environment()["SKIMAP_STORE"] = store.absolutePath
                    environment()["SKIMAP_SERVER_DB"] = File(tmp.root, "server.sqlite").absolutePath
                    environment()["SKIMAP_CONTACT_EMAIL"] = "test@example.org"
                }.start()
        // Bounded wait for /healthz (the server opens SQLite and the store first).
        val deadline = System.currentTimeMillis() + STARTUP_MS
        while (true) {
            val ok =
                try {
                    (URI("$base/healthz").toURL().openConnection() as HttpURLConnection).run {
                        connectTimeout = 200
                        responseCode.also { disconnect() } == 200
                    }
                } catch (_: IOException) {
                    false
                }
            if (ok) break
            check(server.isAlive && System.currentTimeMillis() < deadline) { File(tmp.root, "server.log").readText() }
            Thread.sleep(POLL_MS)
        }
    }

    @After
    fun stopServer() {
        server.destroy()
        server.waitFor()
    }

    private fun http() = HttpSource(base, "skimap-test")

    private fun target(dir: File = pack("test")) = SyncTarget("test", "/v1/sites/test/index.json", dir)

    /** SHA-256 of every file of a pack, by path (byte-for-byte comparison). */
    private fun tree(dir: File): Map<String, String> =
        dir
            .walkTopDown()
            .filter { it.isFile }
            .associate { f ->
                f.relativeTo(dir).path to MessageDigest.getInstance("SHA-256").digest(f.readBytes()).joinToString("") { "%02x".format(it) }
            }

    private fun testPack(version: Int): File {
        val v1 = File(checkNotNull(System.getProperty("skimap.testPack")))
        return if (version == 1) v1 else File(v1.parentFile.parentFile, "testPackV2/testpack")
    }

    @Test
    fun firstDownloadThenNothingThenDelta() {
        val s = PackSyncer(http())
        val progress = mutableListOf<Long>()
        val first = s.sync(target()) { done, _ -> progress += done }
        println("first download: $first")
        assertFalse(first.upToDate)
        assertNull(first.from)
        assertEquals("20261116T060000Z", first.to)
        assertEquals(tree(testPack(2)), tree(pack("test")))
        assertTrue(progress.isNotEmpty())
        assertEquals("20261116T060000Z", syncState(pack("test").absolutePath)!!.version)
        // The download leftovers are gone; only the pack is listed.
        assertEquals(listOf("test"), listPacks(pack("test").parent).map { it.siteId })
        assertFalse(File(pack("test").parentFile, ".test.download").exists())

        // Nothing new: one request, 304, no body.
        val again = s.sync(target())
        println("sync without change: $again")
        assertTrue(again.upToDate)
        assertEquals(1, again.requests)
        assertEquals(0L, again.bytes)

        // A phone at version 1 (pack copied, no state): the delta only.
        pack("test").deleteRecursively()
        File(pack("test").parentFile, ".test.sync.json").delete()
        testPack(1).copyRecursively(pack("test"))
        val delta = s.sync(target())
        println("daily update (delta 1 -> 2): $delta")
        assertEquals("20261115T060000Z", delta.from)
        assertEquals(1, delta.bundles)
        assertEquals(2, delta.requests)
        assertTrue("delta ${delta.bytes} < first ${first.bytes}", delta.bytes < first.bytes)
        assertEquals(tree(testPack(2)), tree(pack("test")))
        // And the map opens it with the validated style.
        val style = File(checkNotNull(System.getProperty("skimap.styles")), "fond-clair.json").readText()
        val view = openPacks(listOf(pack("test").absolutePath), style, false, Lang.FR)
        assertEquals(2u, view.info.status!!.open)
    }

    @Test
    fun resumesACutDownloadAndRefusesACorruptOne() {
        val index = JSONObject(http().let { (it.fetch(target().indexUrl, null) as fr.skimap.sync.Fetched.Body).bytes.decodeToString() })
        val b = index.getJSONObject("base")
        val sha = b.getString("sha256")
        val bytes = b.getLong("bytes")
        val whole = File(store, "public/bundles/$sha.skb").readBytes()
        val work = File(pack("test").parentFile, ".test.download").apply { mkdirs() }
        // Cut after half: the bytes received are kept, the rest comes with Range.
        File(work, "$sha.part").writeBytes(whole.copyOf(whole.size / 2))
        val h = http()
        val r = PackSyncer(h).sync(target())
        println("resumed: $r (bundle $bytes bytes)")
        assertTrue("received ${r.bytes} of $bytes", r.bytes < bytes)
        assertEquals(tree(testPack(2)), tree(pack("test")))

        // A partial file with wrong bytes: the bundle check fails, it is downloaded again.
        pack("test").deleteRecursively()
        File(pack("test").parentFile, ".test.sync.json").delete()
        File(work, "$sha.part").apply { parentFile.mkdirs() }.writeBytes(ByteArray(whole.size / 2) { 7 })
        val again = PackSyncer(http()).sync(target())
        println("corrupt partial file: $again")
        assertTrue("downloaded twice: ${again.bytes}", again.bytes >= bytes)
        assertEquals(tree(testPack(2)), tree(pack("test")))
    }

    @Test
    fun aPhoneThatLostContentsFallsBackToTheBase() {
        testPack(1).copyRecursively(pack("test"))
        // A file of version 1 that version 2 reuses, gone: the delta cannot be installed.
        File(pack("test"), "layers/refuges-camptocamp.geojson").delete()
        val r = PackSyncer(http()).sync(target())
        println("missing contents: $r")
        assertTrue(r.fellBack)
        assertEquals(tree(testPack(2)), tree(pack("test")))
    }

    @Test
    fun aBasemapThatDoesNotRebuildFallsBackToWholeFiles() {
        testPack(1).copyRecursively(pack("test"))
        // The basemap of version 1 altered in its tile data, same size: the file rebuilt
        // from the patch of tiles differs from the server's (#160).
        val f = File(pack("test"), "basemap/plan-ign.pmtiles")
        val b = f.readBytes()
        b[b.size - 1] = (b[b.size - 1].toInt() xor 0xff).toByte()
        f.writeBytes(b)
        val r = PackSyncer(http()).sync(target())
        println("patch failed: $r")
        assertTrue(r.fellBack)
        assertEquals(tree(testPack(2)), tree(pack("test")))
    }

    @Test
    fun serverUnreachableLeavesThePackAsItIs() {
        PackSyncer(http()).sync(target())
        val before = tree(pack("test"))
        stopServer()
        try {
            PackSyncer(http()).sync(target())
            fail("synced without a server")
        } catch (_: IOException) {
        }
        assertEquals(before, tree(pack("test")))
        val style = File(checkNotNull(System.getProperty("skimap.styles")), "srn-sombre.json").readText()
        assertEquals("skimap test srn-sombre", openPacks(listOf(pack("test").absolutePath), style, false, Lang.FR).style.name)
        startServer()
    }

    @Test
    fun serverCatalogAndAVoicePackApart() {
        // The catalog as the server publishes it (VO6): the English voice of the test site,
        // a pack of its own, listed in the `voice` field of the site, not as a site.
        val sites = JSONObject(File(store, "public/catalog.json").readText()).getJSONArray("sites")
        assertEquals(listOf("test"), (0 until sites.length()).map { sites.getJSONObject(it).getJSONObject("site").getString("id") })
        assertEquals(
            "/v1/sites/test/voice/en/index.json",
            sites
                .getJSONObject(0)
                .getJSONArray("voice")
                .getJSONObject(0)
                .getString("index_url"),
        )

        val cache = File(tmp.root, "phone/server")
        val sc = ServerCatalog({ http() }, cache)
        assertEquals(emptyList<Any>(), sc.cached())
        val fresh = checkNotNull(sc.refresh())
        val site = fresh.first { it.siteId == "test" }
        assertEquals("20261116T060000Z", site.latestVersion)
        assertEquals("en", site.voice.single().lang)
        // Again: 304, the copy is used.
        assertEquals(fresh, sc.refresh())
        // Offline: the copy.
        stopServer()
        assertNull(sc.refresh())
        assertEquals(fresh, sc.cached())
        startServer()

        val voiceDir = File(tmp.root, "phone/voice/test/en")
        val r = PackSyncer(http()).sync(SyncTarget("test", site.voice.single().indexUrl, voiceDir, "en"))
        println("voice pack en: $r")
        assertTrue(File(voiceDir, "voice/manifest.json").isFile)
        assertEquals(2, File(voiceDir, "voice/clips").list()!!.size)
        // The site pack has no voice files of its own in this test: the two are apart.
        PackSyncer(http()).sync(target())
        assertFalse(File(pack("test"), "voice/clips").exists())
    }

    /** Publishes the France basemap in [dir] into the store of the running server. */
    private fun publishFrance(dir: File) {
        val server = checkNotNull(System.getProperty("skimap.server"))
        val p =
            ProcessBuilder(server, "publish-france", dir.absolutePath, "--store", store.absolutePath)
                .redirectErrorStream(true)
                .start()
        val out = p.inputStream.bufferedReader().readText()
        assertEquals(out, 0, p.waitFor())
    }

    /** The France basemap (X20): first download, 304, new tiles only, then readable by the core. */
    @Test
    fun franceBasemapDownloadThenRevalidation() {
        val phone = File(tmp.root, "phone/france")
        val s = FranceSyncer(http())
        // Nothing published yet: an HTTP error, retried later by the work; nothing installed.
        try {
            s.sync(phone)
            fail("no France basemap published")
        } catch (e: IOException) {
            assertTrue(e.toString(), e.toString().contains("404"))
        }
        assertFalse(File(phone, "france.json").exists())

        publishFrance(FranceFixture.write(tmp.newFolder("pipeline-1"), tiles = "tiles 1", build = "20261110.pmtiles"))
        val first = s.sync(phone)
        println("France basemap, first download: $first")
        assertEquals("20261110.pmtiles", first.build)
        assertEquals(2, first.files)
        assertEquals("tiles 1", File(phone, "france.pmtiles").readText())
        assertFalse(File(phone.parentFile, ".france.download").exists())

        val again = s.sync(phone)
        println("France basemap, unchanged: $again")
        assertTrue(again.upToDate)

        // Its own work and the daily update at once (as at the first start): one at a time,
        // both succeed, the basemap stays whole.
        publishFrance(FranceFixture.write(tmp.newFolder("pipeline-race"), tiles = "tiles race", build = "20261112.pmtiles"))
        val errors = java.util.Collections.synchronizedList(mutableListOf<Throwable>())
        val threads =
            List(2) {
                Thread {
                    try {
                        FranceSyncer(http()).sync(phone)
                    } catch (e: Throwable) {
                        errors += e
                    }
                }.apply { start() }
            }
        threads.forEach { it.join() }
        assertEquals(emptyList<Throwable>(), errors.toList())
        assertEquals("tiles race", File(phone, "france.pmtiles").readText())

        publishFrance(FranceFixture.write(tmp.newFolder("pipeline-2"), tiles = "tiles 2", build = "20261117.pmtiles"))
        val next = s.sync(phone)
        println("France basemap, new build: $next")
        assertEquals(1, next.files)
        assertEquals("tiles 2", File(phone, "france.pmtiles").readText())

        val style = File(checkNotNull(System.getProperty("skimap.styles")), "fond-clair.json").readText()
        val view = openMap(emptyList(), phone.absolutePath, style, true, Lang.FR)
        assertEquals("libre", view.info.siteId)
        assertEquals("20261117", view.info.packVersion)
    }

    @Test
    fun aLargePlanWaitsForAnUnmeteredNetwork() {
        val r = PackSyncer(http()).sync(target(), maxBytes = 1_000)
        assertTrue(r.deferredBytes > 1_000)
        assertFalse(pack("test").exists())
    }

    private companion object {
        const val STARTUP_MS = 15_000L
        const val POLL_MS = 50L
    }
}
