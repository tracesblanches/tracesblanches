package fr.skimap.app

import fr.skimap.app.data.DownloadState
import fr.skimap.app.data.PackProblem
import fr.skimap.app.data.Practice
import fr.skimap.app.ui.MapUiState
import fr.skimap.app.ui.MapViewModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class MapViewModelTest {
    private val dispatcher = StandardTestDispatcher()

    @Before
    fun setUp() = Dispatchers.setMain(dispatcher)

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun ready(vm: MapViewModel) = vm.state.value as MapUiState.Ready

    private fun TestScope.vm(
        repo: FakeRepository = FakeRepository(),
        settings: MemorySettings = MemorySettings(),
        styles: FakeStyles = FakeStyles(),
        preferred: String? = null,
        dark: Boolean = false,
        downloads: FakeDownloads? = null,
        nanoTime: () -> Long = System::nanoTime,
    ) = MapViewModel(repo, settings, styles, preferred, dark, downloads, nanoTime, dispatcher).also { advanceUntilIdle() }

    @Test
    fun freeModeByDefaultWithTheValidatedStyle() =
        runTest(dispatcher) {
            val repo = FakeRepository()
            val last = MemorySettings()
            val styles = FakeStyles()
            var t = 0L
            val vm = vm(repo, last, styles, nanoTime = { t.also { t += 7_000_000 } })
            val s = ready(vm)
            // Every installed pack at once, in one call, in the style of the practice.
            assertTrue(s.free)
            assertEquals(listOf(Fixtures.CERVIERES + "+" + Fixtures.CHAPELLE), repo.opened)
            assertEquals(listOf("styles/fond-clair.json"), styles.read)
            assertEquals("skimap libre fond-clair", s.opened.style.name)
            assertEquals(Practice.FOND, s.choice.practice)
            assertEquals(2, s.packs.size)
            assertEquals(2, s.opened.packBounds.size)
            assertEquals(7L, s.openMillis)
            assertEquals("libre", last.value)
            // Snow is hidden by default (optional layers of the style), the rest shown.
            assertEquals(setOf("neige"), s.hidden)
        }

    @Test
    fun darkSystemThemeAtStartIsOneLoad() =
        runTest(dispatcher) {
            val repo = FakeRepository()
            val styles = FakeStyles()
            val vm = vm(repo, styles = styles, dark = true)
            assertEquals("skimap libre fond-sombre", ready(vm).opened.style.name)
            assertEquals(listOf("fond-sombre"), repo.styles)
            // The system switches to light: one restyle, no reopening.
            vm.setDark(false)
            advanceUntilIdle()
            assertEquals("skimap libre fond-clair", ready(vm).opened.style.name)
            assertEquals(1, repo.opened.size)
            vm.setDark(false)
            advanceUntilIdle()
            assertEquals(listOf("fond-sombre", "fond-clair"), repo.styles)
        }

    @Test
    fun practiceAndGuidanceChooseTheStyle() =
        runTest(dispatcher) {
            val repo = FakeRepository()
            val settings = MemorySettings()
            val vm = vm(repo, settings)
            vm.selectPractice(Practice.SRN)
            // The menu shows the choice at once.
            assertEquals(Practice.SRN, ready(vm).choice.practice)
            advanceUntilIdle()
            assertEquals("skimap libre srn-clair", ready(vm).opened.style.name)
            assertEquals("srn", settings.practiceValue)
            assertEquals(setOf("neige"), ready(vm).hidden)
            vm.setGuiding(true)
            advanceUntilIdle()
            assertEquals("skimap libre guidage-clair", ready(vm).opened.style.name)
            vm.setGuiding(false)
            advanceUntilIdle()
            assertEquals("skimap libre srn-clair", ready(vm).opened.style.name)
            // Same practice again: nothing.
            vm.selectPractice(Practice.SRN)
            advanceUntilIdle()
            assertEquals(listOf("fond-clair", "srn-clair", "guidage-clair", "srn-clair"), repo.styles)
            // Next launch: the practice is remembered.
            val vm2 = vm(FakeRepository(), settings)
            assertEquals("skimap libre srn-clair", ready(vm2).opened.style.name)
        }

    @Test
    fun preferredThenLastPack() =
        runTest(dispatcher) {
            val repo = FakeRepository()
            val vm = vm(repo, MemorySettings(Fixtures.CHAPELLE))
            assertEquals(Fixtures.CHAPELLE, ready(vm).opened.info.dir)
            assertFalse(ready(vm).free)

            val vm2 = vm(repo, MemorySettings(Fixtures.CHAPELLE), preferred = "cervieres")
            assertEquals(Fixtures.CERVIERES, ready(vm2).opened.info.dir)

            // A preferred or remembered pack that is gone falls back to the free mode.
            val vm3 = vm(repo, MemorySettings("/packs/gone"), preferred = "gone2")
            assertTrue(ready(vm3).free)
        }

    @Test
    fun switchesPackAndFreeMode() =
        runTest(dispatcher) {
            val repo = FakeRepository()
            val last = MemorySettings()
            val vm = vm(repo, last)
            vm.selectPack(Fixtures.CHAPELLE)
            advanceUntilIdle()
            assertEquals("Chapelle-des-Bois", ready(vm).opened.info.siteName)
            assertFalse(ready(vm).free)
            assertEquals(Fixtures.CHAPELLE, last.value)
            // The current pack again: not reopened.
            vm.selectPack(Fixtures.CHAPELLE)
            advanceUntilIdle()
            vm.selectFree()
            advanceUntilIdle()
            assertTrue(ready(vm).free)
            assertEquals("libre", last.value)
            assertEquals(
                listOf(
                    Fixtures.CERVIERES + "+" + Fixtures.CHAPELLE,
                    Fixtures.CHAPELLE,
                    Fixtures.CERVIERES + "+" + Fixtures.CHAPELLE,
                ),
                repo.opened,
            )
        }

    @Test
    fun errorsAreShownWithoutLosingTheMap() =
        runTest(dispatcher) {
            val repo =
                FakeRepository(
                    entries = listOf(Fixtures.entry(Fixtures.CERVIERES, "Cervières"), Fixtures.entry("/packs/bad", "Bad")),
                    broken = setOf("/packs/bad"),
                )
            val vm = vm(repo)
            vm.selectPack("/packs/bad")
            advanceUntilIdle()
            val s = ready(vm)
            assertTrue("the free mode stays", s.free)
            assertEquals(PackProblem(PackProblem.Kind.INVALID, "manifest.json: EOF"), s.error)
            vm.selectPractice(Practice.SRN)
            advanceUntilIdle()
            assertNull(ready(vm).error)
        }

    @Test
    fun layerGroupsAreKeptPerStyleFamily() =
        runTest(dispatcher) {
            val settings = MemorySettings()
            val vm = vm(settings = settings)
            vm.setLayerVisible("refuges", false)
            advanceUntilIdle()
            assertEquals(setOf("neige", "refuges"), ready(vm).hidden)
            assertEquals(setOf("neige", "refuges"), settings.hidden["fond"])
            // Safety groups and unknown groups: ignored.
            vm.setLayerVisible("pistes", false)
            vm.setLayerVisible("dangers", false)
            vm.setLayerVisible("nothing", false)
            assertEquals(setOf("neige", "refuges"), ready(vm).hidden)
            // SRN has its own choice; coming back to fond restores the first one.
            vm.selectPractice(Practice.SRN)
            advanceUntilIdle()
            assertEquals(setOf("neige"), ready(vm).hidden)
            vm.setLayerVisible("pentes", false)
            vm.setLayerVisible("neige", true)
            advanceUntilIdle()
            assertEquals(setOf("pentes"), settings.hidden["srn"])
            vm.selectPractice(Practice.FOND)
            advanceUntilIdle()
            assertEquals(setOf("neige", "refuges"), ready(vm).hidden)
            // A new view model (next launch) reads the stored choice.
            val vm2 = vm(settings = settings)
            assertEquals(setOf("neige", "refuges"), ready(vm2).hidden)
        }

    @Test
    fun aFinishedDownloadReopensThePacks() =
        runTest(dispatcher) {
            val repo = FakeRepository()
            val downloads = FakeDownloads()
            downloads.states.value = mapOf("old" to DownloadState.Done)
            val vm = vm(repo, downloads = downloads)
            assertEquals(1, repo.opened.size)
            // A download running, then installed: one reopening, the map kept meanwhile.
            downloads.states.value = mapOf("old" to DownloadState.Done, "les-rousses" to DownloadState.Running(1, 2))
            advanceUntilIdle()
            assertEquals(1, repo.opened.size)
            repo.entries = repo.entries + Fixtures.entry("/packs/les-rousses", "Les Rousses")
            downloads.states.value = mapOf("old" to DownloadState.Done, "les-rousses" to DownloadState.Done)
            advanceUntilIdle()
            assertEquals(2, repo.opened.size)
            assertEquals(3, ready(vm).packs.size)
            assertTrue(repo.opened.last().contains("/packs/les-rousses"))
        }

    @Test
    fun cameraIsKeptForTheNextLaunch() =
        runTest(dispatcher) {
            val settings = MemorySettings()
            val vm = vm(settings = settings)
            assertNull(vm.lastCamera)
            vm.saveCamera(
                fr.skimap.app.data
                    .CameraSpot(46.6, 6.1, 13.0),
            )
            advanceUntilIdle()
            assertEquals(13.0, settings.cameraSpot!!.zoom, 0.0)
            assertEquals(46.6, vm(settings = settings).lastCamera!!.lat, 0.0)
        }

    @Test
    fun noReadablePack() =
        runTest(dispatcher) {
            val repo = FakeRepository(entries = listOf(Fixtures.entry("/packs/x", "x", error = "JSON error")))
            val vm = vm(repo)
            val s = vm.state.value as MapUiState.NoPack
            assertEquals("/packs", s.packsDir)
            assertEquals(1, s.unreadable.size)

            repo.entries = listOf(Fixtures.entry(Fixtures.CERVIERES, "Cervières"))
            vm.reload()
            advanceUntilIdle()
            assertTrue(vm.state.value is MapUiState.Ready)
        }

    /** No pack but the France basemap (X20): the free mode opens over it, no empty screen. */
    @Test
    fun franceBasemapWithoutPack() =
        runTest(dispatcher) {
            val repo = FakeRepository(entries = emptyList(), france = true)
            val vm = vm(repo)
            val s = vm.state.value as MapUiState.Ready
            assertTrue(s.free)
            assertEquals("", repo.opened.last())

            // Without it, the empty screen as before.
            val none = vm(FakeRepository(entries = emptyList()))
            assertTrue(none.state.value is MapUiState.NoPack)
        }

    @Test
    fun firstPackBrokenGivesFailedWithTheOthers() =
        runTest(dispatcher) {
            val repo =
                FakeRepository(
                    entries = listOf(Fixtures.entry("/packs/bad", "A"), Fixtures.entry(Fixtures.CHAPELLE, "B")),
                    broken = setOf("/packs/bad"),
                )
            // The remembered area does not open.
            val vm = vm(repo, MemorySettings("/packs/bad"))
            val f = vm.state.value as MapUiState.Failed
            assertEquals("/packs/bad", f.dir)
            vm.selectPack(Fixtures.CHAPELLE)
            advanceUntilIdle()
            assertEquals(Fixtures.CHAPELLE, ready(vm).opened.info.dir)
        }
}
