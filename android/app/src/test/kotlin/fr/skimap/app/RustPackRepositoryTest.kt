package fr.skimap.app

import fr.skimap.app.data.PackProblem
import fr.skimap.app.data.RustPackRepository
import fr.skimap.app.data.StyleChoice
import fr.skimap.app.data.problem
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
 * JNA from `jna.library.path`), the synthetic test pack written by
 * `crates/ffi/examples/test_pack.rs` (Gradle task `testPack`) and the validated styles
 * of `styles/` (what the app embeds as assets).
 */
class RustPackRepositoryTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private fun style(choice: StyleChoice): String =
        File(checkNotNull(System.getProperty("skimap.styles")), choice.asset.removePrefix("styles/")).readText()

    private fun packsWithTestPack(): File {
        val src = File(checkNotNull(System.getProperty("skimap.testPack")) { "skimap.testPack not set" })
        val packs = tmp.newFolder("packs")
        src.copyRecursively(File(packs, "test"))
        File(packs, "broken").mkdirs()
        File(packs, "broken/manifest.json").writeText("{")
        return packs
    }

    @Test
    fun opensTheTestPackInEveryValidatedStyle() =
        runTest {
            val packs = packsWithTestPack()
            val repo = RustPackRepository({ packs }, Dispatchers.Unconfined)
            assertTrue(coreVersion().startsWith("skimap-ffi "))

            val list = repo.list()
            assertEquals(listOf("broken", "Pack de test"), list.map { it.siteName })
            assertTrue(list[0].error!!.contains("JSON"))
            assertNull(list[1].error)

            val opened = repo.open(listOf(list[1].dir), style(StyleChoice()), free = false)
            assertEquals("Pack de test", opened.info.siteName)
            assertEquals(1u, opened.info.status!!.open)
            assertEquals(1u, opened.info.status!!.closed)
            assertEquals("skimap test fond-clair", opened.style.name)
            val json = JSONObject(opened.style.json)
            val url = json.getJSONObject("sources").getJSONObject("ign").getString("url")
            assertEquals("pmtiles://file://${File(packs, "test/basemap/plan-ign.pmtiles").absolutePath}", url)
            val pistes = json.getJSONObject("sources").getJSONObject("pistes").getString("data")
            assertTrue(File(pistes.removePrefix("file://")).isFile)
            // The test pack has no OSM basemap and no snow raster: no empty group; it has fee zones.
            assertEquals(
                listOf("refuges", "noms", "redevance"),
                opened.style.groups
                    .filter { it.toggleable }
                    .map { it.id },
            )
            assertTrue(opened.style.legend.any { it.key == "intermediate" })

            for (choice in listOf(StyleChoice(dark = true), StyleChoice(fr.skimap.app.data.Practice.SRN), StyleChoice(guiding = true))) {
                val st = repo.style(style(choice))
                assertEquals("skimap test ${choice.family}-${if (choice.dark) "sombre" else "clair"}", st.name)
            }
        }

    @Test
    fun errorsOfTheCore() =
        runTest {
            val packs = packsWithTestPack()
            val repo = RustPackRepository({ packs }, Dispatchers.Unconfined)
            try {
                repo.open(listOf(File(packs, "broken").absolutePath), style(StyleChoice()), free = false)
                fail("broken pack opened")
            } catch (e: PackException.Invalid) {
                assertEquals(PackProblem.Kind.INVALID, e.problem().kind)
            }
            try {
                repo.open(listOf(File(packs, "nowhere").absolutePath), style(StyleChoice()), free = false)
                fail("missing pack opened")
            } catch (e: PackException.NotFound) {
                assertTrue(e.path.endsWith("nowhere"))
                assertEquals(PackProblem.Kind.NOT_FOUND, e.problem().kind)
            }
            try {
                repo.style(style(StyleChoice()))
                fail("style without an open pack")
            } catch (_: IllegalStateException) {
            }
            assertEquals(emptyList<Any>(), RustPackRepository({ File(packs, "none") }).list())
        }
}
