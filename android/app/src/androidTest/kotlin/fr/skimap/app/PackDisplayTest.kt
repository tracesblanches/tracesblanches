package fr.skimap.app

import android.content.Context
import android.content.Intent
import android.graphics.RectF
import android.os.ParcelFileDescriptor
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.center
import androidx.compose.ui.test.click
import androidx.compose.ui.test.doubleClick
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.junit4.accessibility.enableAccessibilityChecks
import androidx.compose.ui.test.junit4.v2.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.tryPerformAccessibilityChecks
import androidx.test.core.app.ActivityScenario
import androidx.test.espresso.Espresso
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.UiDevice
import fr.skimap.app.ui.Tags
import fr.skimap.app.ui.map.MapProbe
import fr.skimap.app.ui.map.MapRendered
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.maplibre.android.camera.CameraUpdateFactory
import org.maplibre.android.geometry.LatLng
import org.maplibre.android.maps.MapLibreMap
import java.io.File

/**
 * On a device or emulator: opens packs with the real Rust core and MapLibre in the styles
 * validated by the user (`styles/`), waits for the map to be fully rendered and checks
 * that the layers of the style drew features. The synthetic test pack
 * (crates/ffi/examples/test_pack.rs, test assets) always runs; the real packs run when
 * copied to `/data/local/tmp/skimap-packs/<site>/` (android/docs/verifier.md).
 */
@RunWith(AndroidJUnit4::class)
@OptIn(ExperimentalTestApi::class)
class PackDisplayTest {
    @get:Rule
    val rule = createEmptyComposeRule()

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val target: Context = instrumentation.targetContext
    private val packs = File(target.getExternalFilesDir(null), "packs")

    private fun copyAssets(
        path: String,
        dest: File,
    ) {
        val assets = instrumentation.context.assets
        val children = assets.list(path).orEmpty()
        if (children.isEmpty()) {
            dest.parentFile?.mkdirs()
            assets.open(path).use { input -> dest.outputStream().use { input.copyTo(it) } }
        } else {
            children.forEach { copyAssets("$path/$it", File(dest, it)) }
        }
    }

    private fun shell(cmd: String): String =
        ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(cmd)).use {
            it.readBytes().decodeToString()
        }

    @Before
    fun lightThemeAndFond() {
        shell("cmd uimode night no")
        // Practice back to cross-country (fond), last mode forgotten.
        target
            .getSharedPreferences("skimap", Context.MODE_PRIVATE)
            .edit()
            .clear()
            .commit()
    }

    /**
     * Waits (bounded) for the first full rendering of [styleName]. `waitUntil` keeps the
     * Compose test clock running: a plain sleep would freeze the UI under the test rule.
     */
    private fun awaitRendered(
        styleName: String,
        timeoutMs: Long = 120_000,
    ): MapRendered {
        rule.waitUntil(timeoutMs) { MapProbe.rendered.value?.styleName == styleName }
        return checkNotNull(MapProbe.rendered.value)
    }

    private fun zoom(): Double {
        var z = 0.0
        instrumentation.runOnMainSync { z = checkNotNull(MapProbe.map).cameraPosition.zoom }
        return z
    }

    private fun awaitZoom(expected: Double) {
        rule.waitUntil(ANIMATION_TIMEOUT_MS) { kotlin.math.abs(zoom() - expected) < ZOOM_TOLERANCE }
    }

    /** Features drawn on screen by each style layer. */
    private fun renderedCounts(layers: List<String>): Map<String, Int> {
        var counts = emptyMap<String, Int>()
        instrumentation.runOnMainSync {
            val map: MapLibreMap = checkNotNull(MapProbe.map)
            val w =
                target.resources.displayMetrics.widthPixels
                    .toFloat()
            val h =
                target.resources.displayMetrics.heightPixels
                    .toFloat()
            counts = layers.associateWith { map.queryRenderedFeatures(RectF(0f, 0f, w, h), it).size }
        }
        return counts
    }

    private fun awaitLayers(layers: List<String>) {
        try {
            rule.waitUntil(LABEL_TIMEOUT_MS) { renderedCounts(layers).values.all { it > 0 } }
        } catch (e: androidx.compose.ui.test.ComposeTimeoutException) {
            shell("screencap -p /data/local/tmp/skimap-failure.png")
            throw AssertionError("layers not drawn: ${renderedCounts(layers)} at zoom ${zoom()}", e)
        }
    }

    private fun doubleTapMap() {
        rule.onNodeWithTag(Tags.MAP).performTouchInput { doubleClick(center) }
    }

    private fun moveTo(
        lat: Double,
        lon: Double,
        zoom: Double,
    ) {
        instrumentation.runOnMainSync { checkNotNull(MapProbe.map).moveCamera(CameraUpdateFactory.newLatLngZoom(LatLng(lat, lon), zoom)) }
    }

    private fun launch(pack: String): ActivityScenario<MainActivity> =
        ActivityScenario.launch(Intent(target, MainActivity::class.java).putExtra(MainActivity.EXTRA_PACK, pack))

    private fun assertTouchTargets() {
        val nodes = rule.onAllNodes(hasClickAction()).fetchSemanticsNodes()
        val density = target.resources.displayMetrics.density
        assertTrue(nodes.isNotEmpty())
        nodes.forEach {
            val w = it.size.width / density
            val h = it.size.height / density
            assertTrue("target $w × $h dp", w >= 48f && h >= 48f)
        }
    }

    private fun openSheetAndClick(text: String) {
        rule.onNodeWithTag(Tags.PRIMARY).performClick()
        rule.onNodeWithText(text).performScrollTo().performClick()
    }

    @Test
    fun testPackInTheValidatedStyles() {
        File(packs, "test").deleteRecursively()
        copyAssets("testpack", File(packs, "test"))
        launch("test").use {
            val r = awaitRendered("skimap test fond-clair")
            // Basemap (synthetic Plan IGN: forest, lake, road), pistes and their state,
            // guarded refuges at this zoom (dots of z11-13).
            // Basemap (synthetic Plan IGN: forest, roads), pistes and the closed one, faded.
            val layers = listOf("ign-forest", "ign-road-major", "pistes-casing", "pistes-line", "pistes-closed")
            awaitLayers(layers)
            val counts = renderedCounts(layers)
            android.util.Log.i("skimap-test", "test pack fond: $r, rendered $counts at zoom ${zoom()}")
            rule.onNodeWithText("Pack de test", substring = true).assertExists()
            assertTouchTargets()
            rule.enableAccessibilityChecks()
            rule.onRoot().tryPerformAccessibilityChecks()

            // Gestures move the map, then "Vue auto" brings the automatic view back,
            // without reloading the style.
            val loads = MapProbe.styleLoads
            val before = zoom()
            rule.onNodeWithTag(Tags.RECENTER).assertDoesNotExist()
            doubleTapMap()
            awaitZoom(before + 1)
            rule.waitUntil(ANIMATION_TIMEOUT_MS) { rule.onAllNodesWithTag(Tags.RECENTER).fetchSemanticsNodes().isNotEmpty() }
            rule.onNodeWithTag(Tags.RECENTER).performClick()
            awaitZoom(before)
            val w = target.resources.displayMetrics.widthPixels
            val h = target.resources.displayMetrics.heightPixels
            UiDevice.getInstance(instrumentation).swipe(w * 3 / 4, h / 2, w / 4, h / 2, DRAG_STEPS)
            rule.waitUntil(ANIMATION_TIMEOUT_MS) { rule.onAllNodesWithTag(Tags.RECENTER).fetchSemanticsNodes().isNotEmpty() }
            assertEquals("style reloaded by camera actions", loads, MapProbe.styleLoads)
            rule.onNodeWithTag(Tags.RECENTER).performClick()
            awaitZoom(before)

            // One level closer (the dots of guarded refuges are drawn from zoom 11). The
            // style's filter: guarded refuges only in "fond", not the unguarded cabin.
            val huts = listOf("huts-dot-info")
            doubleTapMap()
            awaitZoom(before + 1)
            awaitLayers(huts)
            assertEquals(1, renderedCounts(huts)["huts-dot-info"])
            // Layer group "refuges" off then on: visibility only, no style reload.
            openSheetAndClick(target.getString(R.string.group_refuges))
            rule.waitUntil(ANIMATION_TIMEOUT_MS) { renderedCounts(huts).values.sum() == 0 }
            rule.onNodeWithText(target.getString(R.string.group_refuges)).performScrollTo().performClick()
            awaitLayers(huts)
            assertEquals("style reloaded by the layer switches", loads, MapProbe.styleLoads)
            Espresso.pressBack()

            // Practice SRN: one style load; the refuges of every source are drawn (OSM and
            // Camptocamp only in SRN).
            openSheetAndClick(target.getString(R.string.practice_srn))
            Espresso.pressBack()
            awaitRendered("skimap test srn-clair")
            assertEquals(loads + 1, MapProbe.styleLoads)
            val srnHuts = listOf("huts-dot-info", "huts-dot-osm", "huts-dot-camptocamp", "pistes-line")
            awaitLayers(srnHuts)
            android.util.Log.i("skimap-test", "test pack srn: ${renderedCounts(srnHuts)}")

            // Touch the guarded refuge: its card, built by the core from the map feature.
            var at = android.graphics.PointF()
            instrumentation.runOnMainSync {
                // "Refuge du test" of the test pack (crates/ffi/examples/test_pack.rs).
                at = checkNotNull(MapProbe.map).projection.toScreenLocation(LatLng(44.8586 + 0.016, 6.7746 - 0.015))
            }
            rule.onNodeWithTag(Tags.MAP).performTouchInput { click(Offset(at.x, at.y)) }
            rule.waitUntil(ANIMATION_TIMEOUT_MS) { rule.onAllNodesWithTag(Tags.CARD).fetchSemanticsNodes().isNotEmpty() }
            rule.onNodeWithText("Refuge du test").assertExists()
            Espresso.pressBack()
        }
    }

    private fun realSites(): List<String> {
        val src = "/data/local/tmp/skimap-packs"
        return shell("ls $src").lines().map(String::trim).filter { it.isNotEmpty() && "No such file" !in it }
    }

    private fun installReal(sites: List<String>) {
        packs.mkdirs()
        for (site in sites) {
            File(packs, site).deleteRecursively()
            shell("cp -r /data/local/tmp/skimap-packs/$site ${packs.absolutePath}/$site")
        }
    }

    @Test
    fun realPacksInEveryStyle() {
        val sites = realSites()
        assumeTrue("no real pack in /data/local/tmp/skimap-packs", sites.isNotEmpty())
        installReal(sites)
        for (site in sites) {
            launch(site).use {
                val fond = awaitRendered("skimap $site fond-clair")
                val fondLayers = listOf("ign-forest", "pistes-line", "pistes-casing", "ign-road-major")
                awaitLayers(fondLayers)
                val fondCounts = renderedCounts(fondLayers)
                shell("screencap -p /data/local/tmp/skimap-$site-fond.png")
                // Two levels closer: piste names along the pistes, as the old capture.
                val fit = zoom()
                doubleTapMap()
                awaitZoom(fit + 1)
                doubleTapMap()
                awaitZoom(fit + 2)
                // The zoom of the capture of the former app style, for comparison.
                awaitLayers(listOf("pistes-line", "ign-contour-index"))
                rule.waitForIdle()
                shell("screencap -p /data/local/tmp/skimap-zoom2-$site.png")
                val zoomOptional = renderedCounts(listOf("huts-dot-info", "pistes-closed", "osm-parking", "ign-place-commune"))
                // Piste names from zoom 13 (style), along the pistes: at the village. A name
                // is drawn only where one OSM way is long enough for it (Chapelle-des-Bois
                // yes; at Cervières the long names do not fit the short ways: reported).
                val (lat, lon) = CENTRES.getValue(site)
                moveTo(lat, lon, LABEL_ZOOM)
                awaitLayers(listOf("pistes-line", "ign-forest"))
                if (site == "chapelle-des-bois") awaitLayers(listOf("pistes-label"))
                rule.waitForIdle()
                val zoomCounts = renderedCounts(listOf("pistes-label", "pistes-line", "ign-forest"))
                shell("screencap -p /data/local/tmp/skimap-village-$site.png")

                // SRN: slope classes and avalanche exposure, refuges, zones.
                rule.onNodeWithTag(Tags.RECENTER).performClick()
                awaitZoom(fit)
                openSheetAndClick(target.getString(R.string.practice_srn))
                Espresso.pressBack()
                val srn = awaitRendered("skimap $site srn-clair")
                shell("screencap -p /data/local/tmp/skimap-$site-srn.png")
                doubleTapMap()
                awaitZoom(fit + 1)
                doubleTapMap()
                awaitZoom(fit + 2)
                doubleTapMap()
                awaitZoom(fit + 3)
                val srnLayers = listOf("slope-1", "ign-contour-index")
                awaitLayers(srnLayers)
                val srnCounts = renderedCounts(srnLayers + listOf("slope-2", "slope-3", "exposure-2", "zones-line", "huts-dot-info"))
                shell("screencap -p /data/local/tmp/skimap-zoom3-$site-srn.png")
                android.util.Log.i("skimap-test", "$site: $fond $fondCounts; zoom +2 $zoomCounts $zoomOptional; $srn $srnCounts")
                (fondCounts + zoomCounts - "pistes-label").forEach { (layer, n) -> assertTrue("$site $layer drew nothing", n > 0) }
                srnLayers.forEach { assertTrue("$site $it drew nothing", (srnCounts[it] ?: 0) > 0) }
            }
            // Dark theme of the system: the dark style, without a gesture.
            shell("cmd uimode night yes")
            launch(site).use {
                val dark = awaitRendered("skimap $site srn-sombre")
                // The camera comes back where the app was left; the village for the capture.
                val (lat, lon) = CENTRES.getValue(site)
                moveTo(lat, lon, LABEL_ZOOM)
                awaitLayers(listOf("pistes-line", "ign-contour-index"))
                shell("screencap -p /data/local/tmp/skimap-$site-srn-sombre.png")
                android.util.Log.i("skimap-test", "$site dark: $dark")
            }
            shell("cmd uimode night no")
            target
                .getSharedPreferences("skimap", Context.MODE_PRIVATE)
                .edit()
                .clear()
                .commit()
        }
    }

    @Test
    fun realPacksTogetherInFreeMode() {
        val sites = realSites()
        assumeTrue("needs cervieres and chapelle-des-bois", sites.containsAll(listOf("cervieres", "chapelle-des-bois")))
        installReal(sites)
        launch("libre").use {
            val r = awaitRendered("skimap libre fond-clair")
            // Both packs in one style: between the two areas at zoom 8, each draws its pistes.
            moveTo((44.86 + 46.6) / 2, (6.77 + 6.1) / 2, FREE_ZOOM)
            val layers = listOf("pistes-line", "pistes-line~p1")
            awaitLayers(layers)
            val counts = renderedCounts(layers)
            shell("screencap -p /data/local/tmp/skimap-free.png")
            // Up close on Chapelle-des-Bois: the second pack, drawn as the first.
            moveTo(46.6, 6.11, CLOSE_ZOOM)
            awaitLayers(listOf("pistes-line~p1", "ign-forest~p1"))
            shell("screencap -p /data/local/tmp/skimap-free-chapelle.png")
            android.util.Log.i("skimap-test", "free mode: $r $counts")
            rule.onNodeWithText(target.getString(R.string.free_mode_name), substring = true).assertExists()
        }
    }

    private companion object {
        const val ANIMATION_TIMEOUT_MS = 10_000L
        const val ZOOM_TOLERANCE = 0.05
        const val LABEL_TIMEOUT_MS = 30_000L
        const val FREE_ZOOM = 8.0
        const val LABEL_ZOOM = 14.0

        /** Commune centres of the real packs (manifests), latitude and longitude. */
        val CENTRES = mapOf("cervieres" to (44.8586 to 6.7746), "chapelle-des-bois" to (46.5952 to 6.1118))
        const val CLOSE_ZOOM = 13.0
        const val DRAG_STEPS = 40
    }
}
