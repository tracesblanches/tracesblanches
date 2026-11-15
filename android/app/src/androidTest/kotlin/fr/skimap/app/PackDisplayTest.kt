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
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.maplibre.android.geometry.LatLng
import org.maplibre.android.maps.MapLibreMap
import java.io.File

/**
 * On a device or emulator: opens a real pack with the real Rust core and MapLibre, waits
 * for the map to be fully rendered and checks that the basemap and every pack layer drew
 * features. The synthetic test pack (crates/ffi/examples/test_pack.rs, test assets)
 * always runs; the real packs run when copied to `/data/local/tmp/skimap-packs/<site>/`
 * (README).
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

    /** Waits for the camera to reach [expected] zoom (end of the animation). */
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

    /** Double tap in the middle of the map: MapLibre zooms in by one level. */
    private fun doubleTapMap() {
        rule.onNodeWithTag(Tags.MAP).performTouchInput { doubleClick(center) }
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

    @Test
    fun testPackShowsBasemapAndLayers() {
        File(packs, "test").deleteRecursively()
        copyAssets("testpack", File(packs, "test"))
        launch("test").use {
            val r = awaitRendered("skimap test test")
            val layers =
                listOf(
                    "snow",
                    "grid",
                    "skimap-status-halo",
                    "skimap-pistes-line",
                    "skimap-pistes-walk",
                    "skimap-refuges-info-icon",
                    "skimap-refuges-osm-icon",
                    "skimap-refuges-camptocamp-icon",
                )
            // Symbols (refuge icons) can be placed a frame or two after the first full
            // rendering: wait for them, bounded.
            rule.waitUntil(LABEL_TIMEOUT_MS) { renderedCounts(layers).values.all { it > 0 } }
            val counts = renderedCounts(layers)
            android.util.Log.i("skimap-test", "test pack: $r, rendered $counts")
            counts.forEach { (layer, n) -> assertTrue("$layer drew nothing: $counts", n > 0) }
            // Filters: the water point and the bus shelter are not refuges.
            assertEquals(1, counts["skimap-refuges-info-icon"])
            assertEquals(1, counts["skimap-refuges-osm-icon"])

            rule.onNodeWithText("Pack de test · 1 ouverte, 1 fermée").assertExists()
            assertTouchTargets()
            rule.enableAccessibilityChecks()
            rule.onRoot().tryPerformAccessibilityChecks()

            // No map control on screen; gestures move the map, then "Vue auto" appears and
            // brings the automatic view back, without reloading the style.
            val loads = MapProbe.styleLoads
            val before = zoom()
            rule.onNodeWithTag(Tags.RECENTER).assertDoesNotExist()
            doubleTapMap()
            awaitZoom(before + 1)
            rule.waitUntil(ANIMATION_TIMEOUT_MS) { rule.onAllNodesWithTag(Tags.RECENTER).fetchSemanticsNodes().isNotEmpty() }
            assertTouchTargets()
            rule.onNodeWithTag(Tags.RECENTER).performClick()
            awaitZoom(before)
            rule.onNodeWithTag(Tags.RECENTER).assertDoesNotExist()
            // A drag injected by the system (UiAutomator), as a finger: the drag injected
            // through the Compose test rule does not reach the MapLibre view.
            val w = target.resources.displayMetrics.widthPixels
            val h = target.resources.displayMetrics.heightPixels
            UiDevice.getInstance(instrumentation).swipe(w * 3 / 4, h / 2, w / 4, h / 2, DRAG_STEPS)
            rule.waitUntil(ANIMATION_TIMEOUT_MS) { rule.onAllNodesWithTag(Tags.RECENTER).fetchSemanticsNodes().isNotEmpty() }
            assertEquals("style reloaded by camera actions", loads, MapProbe.styleLoads)

            // Panel "Couches": refuges off then on, by layer visibility (no style reload).
            val refuges = listOf("skimap-refuges-info-icon", "skimap-refuges-osm-icon", "skimap-refuges-camptocamp-icon")
            rule.onNodeWithTag(Tags.RECENTER).performClick()
            rule.onNodeWithTag(Tags.PRIMARY).performClick()
            rule.onNodeWithText("Refuges, cabanes et abris").performScrollTo().performClick()
            rule.waitUntil(ANIMATION_TIMEOUT_MS) { renderedCounts(refuges).values.sum() == 0 }
            rule.onNodeWithText("Refuges, cabanes et abris").performScrollTo().performClick()
            rule.waitUntil(LABEL_TIMEOUT_MS) { renderedCounts(refuges).values.all { it > 0 } }
            assertEquals("style reloaded by the layer switches", loads, MapProbe.styleLoads)
            Espresso.pressBack()

            // Touch a refuge: its card (bottom sheet), built by the core from the map feature.
            var at = android.graphics.PointF()
            instrumentation.runOnMainSync {
                // "Cabane du test" of the test pack (crates/ffi/examples/test_pack.rs).
                at = checkNotNull(MapProbe.map).projection.toScreenLocation(LatLng(44.8586 + 0.016, 6.7746 - 0.015))
            }
            rule.onNodeWithTag(Tags.MAP).performTouchInput { click(Offset(at.x, at.y)) }
            rule.waitUntil(ANIMATION_TIMEOUT_MS) { rule.onAllNodesWithTag(Tags.CARD).fetchSemanticsNodes().isNotEmpty() }
            rule.onNodeWithText("Cabane du test").assertExists()
            rule.onNodeWithText("Gardiennage").assertExists()
        }
    }

    @Test
    fun realPacksShowBothBasemaps() {
        val src = "/data/local/tmp/skimap-packs"
        val sites = shell("ls $src").lines().map(String::trim).filter { it.isNotEmpty() && "No such file" !in it }
        assumeTrue("no real pack in $src", sites.isNotEmpty())
        packs.mkdirs()
        for (site in sites) {
            File(packs, site).deleteRecursively()
            shell("cp -r $src/$site ${packs.absolutePath}/$site")
            launch(site).use {
                val ign = awaitRendered("skimap $site plan-ign")
                val loads = MapProbe.styleLoads
                val ignLayers = listOf("skimap-pistes-line", "skimap-refuges-info-icon", "skimap-status-halo")
                rule.waitUntil(LABEL_TIMEOUT_MS) { renderedCounts(ignLayers).values.all { it > 0 } }
                val ignCounts = renderedCounts(ignLayers)
                // Touch the first refuge drawn: its card, captured for the report.
                var at: android.graphics.PointF? = null
                instrumentation.runOnMainSync {
                    val map = checkNotNull(MapProbe.map)
                    val w =
                        target.resources.displayMetrics.widthPixels
                            .toFloat()
                    val h =
                        target.resources.displayMetrics.heightPixels
                            .toFloat()
                    val f = map.queryRenderedFeatures(RectF(0f, h * 0.3f, w * 0.75f, h * 0.8f), "skimap-refuges-info-icon").firstOrNull()
                    val p = f?.geometry() as? org.maplibre.geojson.Point
                    at = p?.let { map.projection.toScreenLocation(LatLng(it.latitude(), it.longitude())) }
                }
                at?.let { pt ->
                    rule.onNodeWithTag(Tags.MAP).performTouchInput { click(Offset(pt.x, pt.y)) }
                    rule.waitUntil(ANIMATION_TIMEOUT_MS) { rule.onAllNodesWithTag(Tags.CARD).fetchSemanticsNodes().isNotEmpty() }
                    rule.waitForIdle()
                    shell("screencap -p /data/local/tmp/skimap-card-$site.png")
                    Espresso.pressBack()
                }
                // Piste names (with the shape of their difficulty) from zoom 12.
                val fit = zoom()
                doubleTapMap()
                awaitZoom(fit + 1)
                doubleTapMap()
                awaitZoom(fit + 2)
                rule.waitUntil(LABEL_TIMEOUT_MS) { renderedCounts(listOf("skimap-pistes-label")).values.first() > 0 }
                val labelCounts = renderedCounts(listOf("skimap-pistes-label", "skimap-pistes-difficulty"))
                shell("screencap -p /data/local/tmp/skimap-zoom2-$site.png")
                rule.onNodeWithTag(Tags.RECENTER).performClick()
                awaitZoom(fit)
                rule.onNodeWithTag(Tags.PRIMARY).performClick()
                rule.onNodeWithText("OpenStreetMap (Protomaps)").performScrollTo().performClick()
                val osm = awaitRendered("skimap $site osm")
                val osmLayers = listOf("skimap-pistes-line", "skimap-refuges-info-icon")
                rule.waitUntil(LABEL_TIMEOUT_MS) { renderedCounts(osmLayers).values.all { it > 0 } }
                val osmCounts = renderedCounts(osmLayers)
                android.util.Log.i("skimap-test", "$site: $ign $ignCounts; zoom ${fit + 2}: $labelCounts; $osm $osmCounts")
                assertEquals("one style load for the basemap switch", loads + 1, MapProbe.styleLoads)
                (ignCounts + labelCounts + osmCounts).forEach { (layer, n) -> assertTrue("$site $layer drew nothing", n > 0) }
            }
        }
    }

    @Test
    fun realPacksTogetherInFreeMode() {
        val src = "/data/local/tmp/skimap-packs"
        val sites = shell("ls $src").lines().map(String::trim).filter { it.isNotEmpty() && "No such file" !in it }
        assumeTrue("needs two real packs in $src", sites.size >= 2)
        packs.mkdirs()
        for (site in sites) {
            File(packs, site).deleteRecursively()
            shell("cp -r $src/$site ${packs.absolutePath}/$site")
        }
        launch("libre").use {
            val r = awaitRendered("skimap libre plan-ign")
            val layers = listOf("skimap-pistes-line", "skimap-pistes-line~p1")
            // Both packs in one style: zoom out over both areas, each draws its pistes.
            instrumentation.runOnMainSync {
                val map = checkNotNull(MapProbe.map)
                map.moveCamera(
                    org.maplibre.android.camera.CameraUpdateFactory
                        .zoomTo(FREE_ZOOM),
                )
            }
            rule.waitUntil(LABEL_TIMEOUT_MS) { renderedCounts(layers).values.all { it > 0 } }
            val counts = renderedCounts(layers)
            android.util.Log.i("skimap-test", "free mode: $r $counts")
            shell("screencap -p /data/local/tmp/skimap-free.png")
            rule.onNodeWithText("Tous les domaines installés", substring = true).assertExists()
        }
    }

    private companion object {
        const val ANIMATION_TIMEOUT_MS = 10_000L
        const val ZOOM_TOLERANCE = 0.05
        const val LABEL_TIMEOUT_MS = 30_000L
        const val FREE_ZOOM = 7.5
        const val DRAG_STEPS = 40
    }
}
