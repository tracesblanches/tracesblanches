package fr.skimap.app

import android.Manifest
import android.app.Application
import android.location.Location
import android.location.LocationManager
import android.os.SystemClock
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.junit4.v2.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.fog.Calibration
import fr.skimap.app.fog.CompassReading
import fr.skimap.app.fog.Fix
import fr.skimap.app.fog.FogActivity
import fr.skimap.app.fog.FogScreen
import fr.skimap.app.fog.FogTags
import fr.skimap.app.fog.FogUi
import fr.skimap.app.fog.QuickFix
import fr.skimap.app.fog.accuracyText
import fr.skimap.core.FogTarget
import fr.skimap.core.Lang
import fr.skimap.core.fogBearing
import fr.skimap.core.fogPoint
import fr.skimap.core.fogTargets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.util.Locale

/**
 * The bearing screen of #181: the targets without a name told apart, and the quick start
 * of the position (the last known one and the fused provider until the GPS has a fix,
 * then the GPS), with its accuracy shown. Captures `src/test/screenshots/fog_*.png`.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class FogQuickTest {
    @get:Rule
    val rule = createEmptyComposeRule()

    private fun fix(
        gps: Boolean,
        t: Long,
    ) = Fix(6.0, 45.0, if (gps) 5f else 800f, gps, t, 0.0)

    @Test
    fun quickFixGivesWayToTheGps() {
        val now = 1_000_000_000L
        // Nothing yet: anything.
        assertTrue(QuickFix.replaces(null, fix(gps = false, now), now))
        // The GPS replaces a quick position at once.
        assertTrue(QuickFix.replaces(fix(gps = false, now - 1000), fix(gps = true, now), now))
        // A quick position never replaces a GPS fix of the last 30 s…
        assertFalse(QuickFix.replaces(fix(gps = true, now - 29_000), fix(gps = false, now), now))
        // …but does a GPS fix lost for longer (under a cliff, in a forest).
        assertTrue(QuickFix.replaces(fix(gps = true, now - 31_000), fix(gps = false, now), now))
        // An older position never replaces a newer one.
        assertFalse(QuickFix.replaces(fix(gps = false, now), fix(gps = true, now - 1), now))
        // The freshest last known position, not older than 30 min.
        val old = fix(gps = false, now - 31 * 60_000L)
        val recent = fix(gps = false, now - 2 * 60_000L)
        assertEquals(recent, QuickFix.freshest(listOf(old, recent), now))
        assertEquals(null, QuickFix.freshest(listOf(old), now))
        assertEquals("5 m", accuracyText(4.6f, Locale.FRANCE))
        assertEquals("1 m", accuracyText(0.2f, Locale.FRANCE))
        assertEquals("1,2 km", accuracyText(1200f, Locale.FRANCE))
    }

    private fun location(
        provider: String,
        lon: Double,
        lat: Double,
        accuracy: Float,
        timeMs: Long,
    ) = Location(provider).apply {
        longitude = lon
        latitude = lat
        this.accuracy = accuracy
        time = timeMs
        elapsedRealtimeNanos = SystemClock.elapsedRealtimeNanos()
    }

    /**
     * The real screen on the real core with the test pack: the bearing shows from the last
     * known position before any GPS fix, with « Position rapide ± 800 m … il y a 2 min »;
     * the fused provider updates it; the first GPS fix replaces it (« Position GPS ± 5 m »)
     * and a later fused fix does not. The point of the map is named by the nearest place
     * of the pack.
     */
    @Test
    fun bearingStartsFromTheQuickPositionThenTheGps() {
        val app = ApplicationProvider.getApplicationContext<Application>()
        val src = File(checkNotNull(System.getProperty("skimap.testPack")) { "skimap.testPack not set" })
        val packs = MainActivity.packsDir(app)
        src.copyRecursively(File(packs, "test"), overwrite = true)
        shadowOf(app).grantPermissions(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
        val lm = app.getSystemService(LocationManager::class.java)
        val slm = shadowOf(lm)
        slm.setProviderEnabled(LocationManager.FUSED_PROVIDER, true)
        slm.setProviderEnabled(LocationManager.NETWORK_PROVIDER, true)
        val now = System.currentTimeMillis()
        val targets = fogTargets(File(packs, "test").path, null, null, Lang.FR, 0u)
        val refuge = targets.first { it.kind == "refuge" }
        val (lon, lat) = refuge.lon to refuge.lat + 600.0 / 111_195.0
        slm.setLastKnownLocation(
            LocationManager.NETWORK_PROVIDER,
            location(LocationManager.NETWORK_PROVIDER, refuge.lon, refuge.lat - 1400.0 / 111_195.0, 800f, now - 2 * 60_000L),
        )
        val expected = fogPoint(File(packs, "test").path, lon, lat, Lang.FR).name
        assertTrue(expected, expected.startsWith("Point à ") && expected != "Point")

        ActivityScenario.launch<FogActivity>(FogActivity.intent(app, "test", lat, lon)).use {
            rule.waitUntil(5_000) { rule.onAllNodesWithText(expected).fetchSemanticsNodes().isNotEmpty() }
            // Before any GPS fix: the bearing from the last known position, its accuracy and age.
            rule.onNodeWithTag(FogTags.ARROW).assertIsDisplayed()
            rule.onNodeWithTag(FogTags.FIX).assertTextEquals("Position rapide ± 800 m, en attente du GPS, il y a 2 min")
            rule.onNodeWithText("2 kilomètres").assertIsDisplayed()
            rule.onRoot().captureRoboImage("src/test/screenshots/fog_quick_fix.png")

            // The fused provider: a newer quick position.
            slm.simulateLocation(location(LocationManager.FUSED_PROVIDER, lon, lat - 1000.0 / 111_195.0, 30f, System.currentTimeMillis()))
            rule.waitForIdle()
            rule.onNodeWithTag(FogTags.FIX).assertTextEquals("Position rapide ± 30 m, en attente du GPS")
            rule.onNodeWithText("1 kilomètre").assertIsDisplayed()

            // The GPS has its fix: it replaces the quick position.
            slm.simulateLocation(location(LocationManager.GPS_PROVIDER, lon, lat - 500.0 / 111_195.0, 5f, System.currentTimeMillis()))
            rule.waitForIdle()
            rule.onNodeWithTag(FogTags.FIX).assertTextEquals("Position GPS ± 5 m")
            rule.onNodeWithText("500 mètres").assertIsDisplayed()

            // A later quick position does not replace a GPS fix of the last 30 s.
            slm.simulateLocation(location(LocationManager.FUSED_PROVIDER, lon, lat - 3000.0 / 111_195.0, 30f, System.currentTimeMillis()))
            rule.waitForIdle()
            rule.onNodeWithTag(FogTags.FIX).assertTextEquals("Position GPS ± 5 m")
            rule.onNodeWithText("500 mètres").assertIsDisplayed()
            rule.onRoot().captureRoboImage("src/test/screenshots/fog_gps_fix.png")
        }
    }
}

/**
 * The list « Aller vers » of #181 with the labels the core gives the unnamed car parks and
 * huts of a real pack (`fog_targets_count` on the Cervières pack): every row told apart,
 * the kind said once, and the distance and direction from the position.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class FogTargetsTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun unnamedTargetsAreToldApart() {
        val here = Fix(6.7270, 44.8560, 6f, true, 0L, 0.0)
        val targets =
            listOf(
                FogTarget("parking", "Parking près du Laus", 6.7256, 44.8577, null),
                FogTarget("parking", "Parking à 300 mètres au nord du Laus", 6.7270, 44.8585, null),
                FogTarget("parking", "Parking à 840 mètres au sud du refuge Napoléon", 6.7232, 44.8305, null),
                FogTarget("parking", "Parking des Vignes", 6.7264, 44.8707, null),
                FogTarget("refuge", "Refuge à 570 mètres au nord-ouest de l'office du tourisme", 6.7330, 44.7720, null),
                FogTarget("refuge", "Refuge Napoléon", 6.7260, 44.8380, null),
                FogTarget("start", "Le Laus", 6.7272, 44.8551, null),
            )
        val t = targets[1]
        val ui =
            FogUi(
                targets = targets,
                target = t,
                bearing = fogBearing(here.lon, here.lat, t.lon, t.lat, 20.0, Lang.FR),
                compass = CompassReading(20.0, Calibration.OK),
                fix = here,
                nowMs = 0L,
            )
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { FogScreen(ui, hasCompass = true, onPick = {}, onVoice = {}) } } }
        rule.onNodeWithTag(FogTags.FIX).assertTextEquals("Position GPS ± 6 m")
        // Every row by its own name; the kind not repeated when the name says it, the
        // distance and direction from here under it.
        assertEquals(targets.size, targets.map { it.name }.toSet().size)
        val list = rule.onNode(hasScrollAction())
        targets.forEach {
            list.performScrollToNode(hasText(it.name))
            assertEquals(0, rule.onAllNodesWithText("Parking ·", substring = true).fetchSemanticsNodes().size)
        }
        assertNotEquals(0, rule.onAllNodesWithText("Départ · ", substring = true).fetchSemanticsNodes().size)
        rule.onNodeWithText("Refuge Napoléon").assertIsDisplayed()
        rule.onRoot().captureRoboImage("src/test/screenshots/fog_targets.png")
    }
}
