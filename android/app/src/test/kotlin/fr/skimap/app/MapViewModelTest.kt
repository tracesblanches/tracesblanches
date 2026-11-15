package fr.skimap.app

import fr.skimap.app.ui.MapUiState
import fr.skimap.app.ui.MapViewModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
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

    @Test
    fun freeModeByDefaultAndRemembered() =
        runTest(dispatcher) {
            val repo = FakeRepository()
            val last = MemorySettings()
            var t = 0L
            val vm = MapViewModel(repo, last, nanoTime = { t.also { t += 7_000_000 } }, io = dispatcher)
            assertEquals(MapUiState.Loading, vm.state.value)
            advanceUntilIdle()
            val s = ready(vm)
            // Every installed pack at once, in one call.
            assertTrue(s.free)
            assertEquals(listOf(Fixtures.CERVIERES + "+" + Fixtures.CHAPELLE), repo.opened)
            assertEquals("Tous les domaines installés", s.opened.info.siteName)
            assertEquals("plan-ign", s.opened.style.basemap)
            assertEquals(2, s.packs.size)
            assertEquals(7L, s.openMillis)
            assertEquals("libre", last.value)
        }

    @Test
    fun preferredThenLastPack() =
        runTest(dispatcher) {
            val repo = FakeRepository()
            val vm = MapViewModel(repo, MemorySettings(Fixtures.CHAPELLE), io = dispatcher)
            advanceUntilIdle()
            assertEquals(Fixtures.CHAPELLE, ready(vm).opened.info.dir)
            assertFalse(ready(vm).free)

            val vm2 = MapViewModel(repo, MemorySettings(Fixtures.CHAPELLE), preferredPack = "cervieres", io = dispatcher)
            advanceUntilIdle()
            assertEquals(Fixtures.CERVIERES, ready(vm2).opened.info.dir)

            // A preferred or remembered pack that is gone falls back to the free mode.
            val vm3 = MapViewModel(repo, MemorySettings("/packs/gone"), preferredPack = "gone2", io = dispatcher)
            advanceUntilIdle()
            assertTrue(ready(vm3).free)
        }

    @Test
    fun switchesPackAndBasemap() =
        runTest(dispatcher) {
            val repo = FakeRepository()
            val last = MemorySettings()
            val vm = MapViewModel(repo, last, io = dispatcher)
            advanceUntilIdle()
            vm.selectBasemap("osm")
            advanceUntilIdle()
            assertEquals("osm", ready(vm).opened.style.basemap)
            assertEquals(listOf("osm"), repo.styles)
            // Same basemap again: nothing to do.
            vm.selectBasemap("osm")
            advanceUntilIdle()
            assertEquals(listOf("osm"), repo.styles)

            vm.selectPack(Fixtures.CHAPELLE)
            advanceUntilIdle()
            assertEquals("Chapelle-des-Bois", ready(vm).opened.info.siteName)
            assertFalse(ready(vm).free)
            assertEquals(Fixtures.CHAPELLE, last.value)
            // The current pack again: not reopened.
            vm.selectPack(Fixtures.CHAPELLE)
            advanceUntilIdle()
            // Back to the free mode.
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
                    entries =
                        listOf(
                            Fixtures.entry(Fixtures.CERVIERES, "Cervières"),
                            Fixtures.entry("/packs/bad", "Bad"),
                        ),
                    broken = setOf("/packs/bad"),
                )
            val vm = MapViewModel(repo, MemorySettings(), io = dispatcher)
            advanceUntilIdle()
            vm.selectBasemap("missing")
            advanceUntilIdle()
            assertEquals("Fond de carte « missing » absent du pack", ready(vm).error)
            vm.selectPack("/packs/bad")
            advanceUntilIdle()
            val s = ready(vm)
            assertTrue("the free mode stays", s.free)
            assertEquals("Pack invalide : manifest.json: EOF", s.error)
            vm.selectBasemap("osm")
            advanceUntilIdle()
            assertNull(ready(vm).error)
        }

    @Test
    fun layerSwitchesAreKeptPerStyle() =
        runTest(dispatcher) {
            val settings = MemorySettings()
            val vm = MapViewModel(FakeRepository(), settings, io = dispatcher)
            advanceUntilIdle()
            // Defaults of the core: zones without network hidden, the rest shown.
            assertEquals(setOf("no-network"), ready(vm).hidden)
            vm.setLayerVisible("refuges", false)
            advanceUntilIdle()
            assertEquals(setOf("no-network", "refuges"), ready(vm).hidden)
            assertEquals(setOf("no-network", "refuges"), settings.hidden["plan-ign"])
            // Unknown switch: ignored. Same value again: nothing stored.
            vm.setLayerVisible("pistes", false)
            assertEquals(setOf("no-network", "refuges"), ready(vm).hidden)
            // Another style has its own choice; coming back restores the first one.
            vm.selectBasemap("osm")
            advanceUntilIdle()
            assertEquals(setOf("no-network"), ready(vm).hidden)
            vm.setLayerVisible("no-network", true)
            advanceUntilIdle()
            vm.selectBasemap("plan-ign")
            advanceUntilIdle()
            assertEquals(setOf("no-network", "refuges"), ready(vm).hidden)
            assertEquals(emptySet<String>(), settings.hidden["osm"])
            // A new view model (next launch) reads the stored choice.
            val vm2 = MapViewModel(FakeRepository(), settings, io = dispatcher)
            advanceUntilIdle()
            assertEquals(setOf("no-network", "refuges"), ready(vm2).hidden)
        }

    @Test
    fun noReadablePack() =
        runTest(dispatcher) {
            val repo = FakeRepository(entries = listOf(Fixtures.entry("/packs/x", "x", error = "JSON error")))
            val vm = MapViewModel(repo, MemorySettings(), io = dispatcher)
            advanceUntilIdle()
            val s = vm.state.value as MapUiState.NoPack
            assertEquals("/packs", s.packsDir)
            assertEquals(1, s.unreadable.size)

            repo.entries = listOf(Fixtures.entry(Fixtures.CERVIERES, "Cervières"))
            vm.reload()
            advanceUntilIdle()
            assertTrue(vm.state.value is MapUiState.Ready)
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
            val vm = MapViewModel(repo, MemorySettings("/packs/bad"), io = dispatcher)
            advanceUntilIdle()
            val f = vm.state.value as MapUiState.Failed
            assertEquals("/packs/bad", f.dir)
            vm.selectPack(Fixtures.CHAPELLE)
            advanceUntilIdle()
            assertEquals(Fixtures.CHAPELLE, ready(vm).opened.info.dir)
        }
}
