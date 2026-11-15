package fr.skimap.app

import fr.skimap.app.data.RustPackRepository
import fr.skimap.app.data.describe
import fr.skimap.core.PackException
import fr.skimap.core.coreVersion
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.test.runTest
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

/**
 * The data layer on the JVM with the real Rust core (host build of crates/ffi, loaded by
 * JNA from `jna.library.path`) and the synthetic test pack written by
 * `crates/ffi/examples/test_pack.rs` (Gradle task `testPack`).
 */
class RustPackRepositoryTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private fun packsWithTestPack(): File {
        val src = File(checkNotNull(System.getProperty("skimap.testPack")) { "skimap.testPack not set" })
        val packs = tmp.newFolder("packs")
        src.copyRecursively(File(packs, "test"))
        File(packs, "broken").mkdirs()
        File(packs, "broken/manifest.json").writeText("{")
        return packs
    }

    @Test
    fun listsOpensAndComposesTheStyle() =
        runTest {
            val packs = packsWithTestPack()
            val repo = RustPackRepository({ packs }, Dispatchers.Unconfined)
            assertTrue(coreVersion().startsWith("skimap-ffi "))

            val list = repo.list()
            assertEquals(listOf("broken", "Pack de test"), list.map { it.siteName })
            assertTrue(list[0].error!!.contains("JSON"))
            assertNull(list[1].error)

            val opened = repo.open(listOf(list[1].dir), null)
            assertEquals("Pack de test", opened.info.siteName)
            val overlays = opened.info.overlays.map { it.id }
            assertEquals(listOf("status", "pistes", "refuges-camptocamp", "refuges-osm", "refuges-info"), overlays)
            assertEquals(1u, opened.info.status!!.open)
            assertEquals(1u, opened.info.status!!.closed)
            assertEquals(10, opened.symbols.size)
            assertEquals(listOf("refuges", "water"), opened.style.toggles.map { it.id })
            assertTrue(opened.legend.any { it.label == "Rouge : difficile" })

            val style = JSONObject(opened.style.json)
            assertEquals("skimap test test", style.getString("name"))
            val url = style.getJSONObject("sources").getJSONObject("test").getString("url")
            assertEquals("pmtiles://file://${File(packs, "test/basemap/test.pmtiles").absolutePath}", url)
            val pistes = style.getJSONObject("sources").getJSONObject("skimap-pistes").getString("data")
            assertTrue(File(pistes.removePrefix("file://")).isFile)

            assertEquals("test", repo.style("test").basemap)
            try {
                repo.style("plan-ign")
                fail("unknown basemap accepted")
            } catch (e: PackException.UnknownBasemap) {
                assertEquals("Fond de carte « plan-ign » absent du pack", e.describe())
            }
        }

    @Test
    fun errorsOfTheCore() =
        runTest {
            val packs = packsWithTestPack()
            val repo = RustPackRepository({ packs }, Dispatchers.Unconfined)
            try {
                repo.open(listOf(File(packs, "broken").absolutePath), null)
                fail("broken pack opened")
            } catch (e: PackException.Invalid) {
                assertTrue(e.describe(), e.describe().startsWith("Pack invalide : "))
            }
            try {
                repo.open(listOf(File(packs, "nowhere").absolutePath), null)
                fail("missing pack opened")
            } catch (e: PackException.NotFound) {
                assertTrue(e.path.endsWith("nowhere"))
            }
            try {
                repo.style("test")
                fail("style without an open pack")
            } catch (_: IllegalStateException) {
            }
            assertEquals(emptyList<Any>(), RustPackRepository({ File(packs, "none") }).list())
        }
}
