package fr.skimap.app

import android.location.Location
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.data.DomainCatalog
import fr.skimap.app.data.RustDomainCatalog
import fr.skimap.app.ui.domains.DomainPicker
import fr.skimap.app.ui.domains.DomainTags
import fr.skimap.app.ui.domains.DomainsState
import fr.skimap.app.ui.domains.DomainsViewModel
import fr.skimap.core.Bounds
import fr.skimap.core.Domain
import fr.skimap.core.DomainDistance
import fr.skimap.core.PackEntry
import fr.skimap.core.PackStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * Area picker: the real catalogue (resource `catalog.json`, 219 areas) through the Rust
 * core, the view model, the screen and its capture.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "w412dp-h915dp-hdpi")
@OptIn(ExperimentalCoroutinesApi::class)
class DomainsTest {
    @get:Rule
    val rule = createComposeRule()

    private val json = File("src/main/assets/catalog.json").readText()
    private val cervieresPack =
        PackEntry("/packs/cervieres", "cervieres", "Cervières", "v", null, Bounds(6.66, 44.77, 6.86, 44.92))

    private fun catalog(): DomainCatalog = RustDomainCatalog({ json }, Dispatchers.Unconfined)

    @Test
    fun realCatalogueThroughTheCore() =
        runTest {
            val c = catalog()
            val all = c.search("", listOf(cervieresPack))
            assertEquals(219, all.size)
            // Installed first: the pack covering Cervières.
            assertEquals(PackStatus.INSTALLED, all.first().status)
            assertEquals("/packs/cervieres", all.first().packDir)
            // Every area inside the bounds of the pack is covered by it, and only those.
            val installed = all.filter { it.status == PackStatus.INSTALLED }
            println("covered by the Cervières pack: ${installed.map { it.name }}")
            assertTrue(installed.any { it.name.contains("Cervières") })
            assertTrue(installed.all { it.lon in 6.66..6.86 && it.lat in 44.77..44.92 })
            assertTrue(all.filter { it.lon in 6.66..6.86 && it.lat in 44.77..44.92 }.all { it.status == PackStatus.INSTALLED })
            val found = c.search("chapelle bois", emptyList())
            assertTrue(found.map { it.name }.toString(), found.any { it.name.contains("Chapelle") })
            val near = c.nearest(6.1118, 46.5952, emptyList())
            assertEquals(5, near.size)
            assertTrue(near.first().km < 10.0)
            assertTrue(c.outline().size > 50)
        }

    @Test
    fun viewModelSearchesAndLocates() =
        runTest(StandardTestDispatcher().also { Dispatchers.setMain(it) }) {
            try {
                val location =
                    Location("test").apply {
                        longitude = 6.1118
                        latitude = 46.5952
                    }
                val vm = DomainsViewModel(catalog(), FakeRepository(entries = listOf(cervieresPack)), { location })
                vm.open()
                advanceUntilIdle()
                assertEquals(219, vm.state.value.all.size)
                vm.search("cervi")
                advanceUntilIdle()
                assertTrue(
                    vm.state.value.results
                        .isNotEmpty(),
                )
                assertTrue(
                    vm.state.value.results
                        .all { it.name.contains("Cervi") || it.place.contains("Cervi") },
                )
                vm.nearest()
                advanceUntilIdle()
                val s = vm.state.value
                assertEquals(5, s.results.size)
                assertTrue(s.distances.getValue(s.results.first().id) < 10.0)
                // No position: said, no crash.
                val vm2 = DomainsViewModel(catalog(), FakeRepository(), { null })
                vm2.open()
                vm2.nearest()
                advanceUntilIdle()
                assertTrue(vm2.state.value.noPosition)
            } finally {
                Dispatchers.resetMain()
            }
        }

    private fun state(): DomainsState {
        fun d(
            id: String,
            name: String,
            status: PackStatus,
            dir: String? = null,
        ) = Domain(id, name, "$name (05)", 6.7, 44.8, 40.0, status, dir)
        val results =
            listOf(
                d("a", "Cervières", PackStatus.INSTALLED, "/packs/cervieres"),
                d("b", "Les Rousses", PackStatus.AVAILABLE),
                d("c", "Plateau de Retord", PackStatus.NOT_BUILT),
            )
        return DomainsState(loading = false, results = results, all = results)
    }

    @Test
    fun pickerSaysTheStateOfEachPackAndOpensInstalledOnly() {
        val picked = mutableListOf<String>()
        val queries = mutableListOf<String>()
        rule.setContent {
            Themed {
                Surface(Modifier.fillMaxSize()) {
                    DomainPicker(state(), onQuery = { queries += it }, onNearest = {}, onPick = { picked += it }, onClose = {})
                }
            }
        }
        rule.onNodeWithText("Pack installé : toucher pour l'ouvrir").assertIsDisplayed()
        rule.onNodeWithText("Pack disponible au téléchargement (bientôt)").assertIsDisplayed()
        rule.onNodeWithText("Pack pas encore construit").assertIsDisplayed()
        rule.onNodeWithText("Plateau de Retord").performClick()
        rule.onNodeWithText("Cervières").performClick()
        assertEquals(listOf("/packs/cervieres"), picked)
        rule.onNodeWithTag(DomainTags.SEARCH).performTextInput("rou")
        assertEquals(listOf("rou"), queries)
        rule.onAllNodes(hasClickAction()).fetchSemanticsNodes().forEach {
            val h = with(rule.density) { it.size.height.toDp() }
            assertTrue("$h", h.value >= 72f)
        }
    }

    @Test
    fun pickerCapture() {
        val (outline, all) =
            runBlocking {
                val c = catalog()
                c.outline() to c.search("", listOf(cervieresPack))
            }
        val state = DomainsState(loading = false, query = "", results = all, all = all, outline = outline)
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { DomainPicker(state, {}, {}, {}, {}) } } }
        rule.onRoot().captureRoboImage("src/test/screenshots/domains.png")
    }
}
