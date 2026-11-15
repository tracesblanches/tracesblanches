package fr.skimap.app

import android.location.Location
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.data.DomainCatalog
import fr.skimap.app.data.DownloadState
import fr.skimap.app.data.RustDomainCatalog
import fr.skimap.app.ui.domains.DomainActions
import fr.skimap.app.ui.domains.DomainPicker
import fr.skimap.app.ui.domains.DomainTags
import fr.skimap.app.ui.domains.DomainsState
import fr.skimap.app.ui.domains.DomainsViewModel
import fr.skimap.core.Bounds
import fr.skimap.core.Domain
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
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/**
 * Area picker and downloads: the real catalogue (resource `catalog.json`, 219 areas)
 * through the Rust core, the server sites, the view model, the screen and its capture.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
@OptIn(ExperimentalCoroutinesApi::class)
class DomainsTest {
    @get:Rule
    val rule = createComposeRule()

    private val json = File("src/main/assets/catalog.json").readText()
    private val cervieresPack =
        PackEntry("/packs/cervieres", "cervieres", "Cervières", "20261115T014538Z", null, Bounds(6.66, 44.77, 6.86, 44.92))

    /** The server publishes Cervières (newer than the one installed) and Chapelle-des-Bois. */
    private val server =
        listOf(
            Fixtures.site("cervieres"),
            Fixtures.site("chapelle-des-bois", bytes = 39_980_000).copy(bbox = listOf(6.03, 46.53, 6.19, 46.66)),
        )

    private fun catalog(): DomainCatalog = RustDomainCatalog({ json }, Dispatchers.Unconfined)

    @Test
    fun realCatalogueThroughTheCore() =
        runTest {
            val c = catalog()
            val all = c.search("", listOf(cervieresPack), server)
            assertEquals(219, all.size)
            // Installed first: the pack covering Cervières.
            assertEquals(PackStatus.INSTALLED, all.first().status)
            assertEquals("/packs/cervieres", all.first().packDir)
            val installed = all.filter { it.status == PackStatus.INSTALLED }
            assertTrue(installed.any { it.name.contains("Cervières") })
            assertTrue(installed.all { it.lon in 6.66..6.86 && it.lat in 44.77..44.92 })
            // Published by the server: the areas inside the Chapelle-des-Bois site.
            val available = all.filter { it.status == PackStatus.AVAILABLE }
            println("published by the server, not installed: ${available.map { it.name }}")
            assertTrue(available.any { it.name.contains("Chapelle") })
            assertTrue(available.all { it.serverSite == "chapelle-des-bois" && it.downloadBytes == 39_980_000uL })
            val found = c.search("chapelle bois", emptyList())
            assertTrue(found.map { it.name }.toString(), found.any { it.name.contains("Chapelle") })
            val near = c.nearest(6.1118, 46.5952, emptyList())
            assertEquals(5, near.size)
            assertTrue(near.first().km < 10.0)
            assertTrue(c.outline().size > 50)
        }

    @Test
    fun viewModelSearchesLocatesAndDownloads() =
        runTest(StandardTestDispatcher().also { Dispatchers.setMain(it) }) {
            try {
                val location =
                    Location("test").apply {
                        longitude = 6.1118
                        latitude = 46.5952
                    }
                val downloads = FakeDownloads(sites = server)
                val vm = DomainsViewModel(catalog(), FakeRepository(entries = listOf(cervieresPack)), { location }, downloads)
                vm.open()
                advanceUntilIdle()
                assertEquals(219, vm.state.value.all.size)
                assertFalse(vm.state.value.offline)
                // Cervières installed at an older version than the one published.
                assertEquals(setOf("cervieres"), vm.state.value.updates)
                vm.search("cervi")
                advanceUntilIdle()
                assertTrue(
                    vm.state.value.results
                        .all { it.name.contains("Cervi") || it.place.contains("Cervi") },
                )
                vm.nearest()
                advanceUntilIdle()
                val s = vm.state.value
                assertEquals(5, s.results.size)
                assertTrue(s.distances.getValue(s.results.first().id) < 10.0)
                // Touch on the map of France near Chapelle-des-Bois: that area first, selected.
                vm.pickOnMap(6.1, 46.6)
                advanceUntilIdle()
                assertEquals(
                    vm.state.value.selected,
                    vm.state.value.results
                        .first()
                        .id,
                )
                // Download, with mobile data, cancel, remove.
                vm.download("chapelle-des-bois")
                vm.download("chapelle-des-bois", mobileData = true)
                vm.download("nowhere")
                vm.cancel("chapelle-des-bois")
                vm.remove("cervieres")
                advanceUntilIdle()
                assertEquals(
                    listOf(
                        "download chapelle-des-bois wifi",
                        "download chapelle-des-bois mobile",
                        "cancel chapelle-des-bois",
                        "remove cervieres",
                    ),
                    downloads.calls,
                )
                downloads.states.value = mapOf("chapelle-des-bois" to DownloadState.Running(10, 100))
                advanceUntilIdle()
                assertEquals(DownloadState.Running(10, 100), vm.state.value.downloads["chapelle-des-bois"])
                // No position: said, no crash. Offline: the last server list, said.
                val vm2 = DomainsViewModel(catalog(), FakeRepository(), { null }, FakeDownloads(sites = server, online = false))
                vm2.open()
                vm2.nearest()
                advanceUntilIdle()
                assertTrue(vm2.state.value.noPosition)
                assertTrue(vm2.state.value.offline)
                assertTrue(
                    vm2.state.value.all
                        .any { it.status == PackStatus.AVAILABLE },
                )
            } finally {
                Dispatchers.resetMain()
            }
        }

    private fun state(downloads: Map<String, DownloadState> = emptyMap()): DomainsState {
        fun d(
            id: String,
            name: String,
            status: PackStatus,
            dir: String? = null,
            site: String? = null,
        ) = Domain(id, name, "$name (05)", 6.7, 44.8, 40.0, status, dir, site, if (site != null) 43_500_000uL else 0uL)
        val results =
            listOf(
                d("a", "Cervières", PackStatus.INSTALLED, "/packs/cervieres", "cervieres"),
                d("b", "Les Rousses", PackStatus.AVAILABLE, site = "les-rousses"),
                d("c", "Plateau de Retord", PackStatus.NOT_BUILT),
                d("d", "Chapelle-des-Bois", PackStatus.AVAILABLE, site = "chapelle-des-bois"),
            )
        return DomainsState(loading = false, results = results, all = results, downloads = downloads, updates = setOf("cervieres"))
    }

    @Test
    fun pickerSaysTheStateOfEachPackAndActs() {
        val calls = mutableListOf<String>()
        rule.setContent {
            Themed {
                Surface(Modifier.fillMaxSize()) {
                    DomainPicker(
                        state(mapOf("chapelle-des-bois" to DownloadState.Waiting(unmetered = true))),
                        DomainActions(
                            onQuery = { calls += "query $it" },
                            onPick = { calls += "open $it" },
                            onDownload = { s, m -> calls += "download $s $m" },
                            onCancel = { calls += "cancel $it" },
                            onRemove = { calls += "remove $it" },
                            onMapTouch = { _, _ -> calls += "map" },
                        ),
                    )
                }
            }
        }
        rule.onNodeWithText("Une version plus récente est publiée").assertIsDisplayed()
        rule.onNodeWithText("Pas encore de pack publié").performScrollTo().assertIsDisplayed()
        rule.onNodeWithText("Plateau de Retord").performClick()
        rule.onNodeWithText("Cervières").performClick()
        rule.onNodeWithText("Mettre à jour").performClick()
        rule.onNodeWithText("Supprimer").performClick()
        rule.onNodeWithText("Télécharger · ", substring = true).performScrollTo().performClick()
        rule.onNodeWithTag(DomainTags.LIST).performScrollToNode(hasText("En attente du Wi-Fi (gros pack)"))
        rule.onNodeWithText("En attente du Wi-Fi (gros pack)").assertIsDisplayed()
        rule.onNodeWithText("Utiliser les données mobiles").performClick()
        rule.onNodeWithText("Annuler").performClick()
        rule.onNodeWithTag(DomainTags.SEARCH).performTextInput("rou")
        rule.onNodeWithTag(DomainTags.MAP).performTouchInput { click() }
        assertEquals(
            listOf(
                "open /packs/cervieres",
                "download cervieres false",
                "remove cervieres",
                "download les-rousses false",
                "download chapelle-des-bois true",
                "cancel chapelle-des-bois",
                "query rou",
                "map",
            ),
            calls,
        )
        rule.onAllNodes(hasClickAction()).fetchSemanticsNodes().forEach {
            val h = with(rule.density) { it.size.height.toDp() }
            assertTrue("$h", h.value >= 72f)
        }
    }

    @Test
    fun progressOfADownload() {
        rule.setContent {
            Themed {
                Surface(Modifier.fillMaxSize()) {
                    DomainPicker(state(mapOf("les-rousses" to DownloadState.Running(12_000_000, 43_500_000))), DomainActions())
                }
            }
        }
        rule.onNodeWithText("Téléchargement : ", substring = true).assertIsDisplayed()
        rule.onRoot().captureRoboImage("src/test/screenshots/domains_progress.png")
    }

    @Test
    fun pickerCapture() {
        val (outline, all) =
            runBlocking {
                val c = catalog()
                c.outline() to c.search("", listOf(cervieresPack), server)
            }
        val state = DomainsState(loading = false, query = "", results = all, all = all, outline = outline, updates = setOf("cervieres"))
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { DomainPicker(state, DomainActions()) } } }
        rule.onRoot().captureRoboImage("src/test/screenshots/domains.png")
    }
}
