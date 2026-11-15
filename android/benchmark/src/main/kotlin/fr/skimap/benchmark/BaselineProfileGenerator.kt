package fr.skimap.benchmark

import androidx.benchmark.macro.junit4.BaselineProfileRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.uiautomator.By
import androidx.test.uiautomator.Until
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Baseline Profile of the critical path: cold start, pack opening, first map, a map
 * gesture and the "Vue auto" button, the primary action (sheet) and a basemap switch.
 * `./gradlew :app:generateBaselineProfile` writes it to `app/src/release/generated/`.
 */
@RunWith(AndroidJUnit4::class)
class BaselineProfileGenerator {
    @get:Rule
    val rule = BaselineProfileRule()

    @Test
    fun generate() =
        rule.collect(packageName = PACKAGE) {
            installPack()
            startOnPackAndWaitForMap()
            // Gestures on the map: a swipe, then the "Vue auto" button that appears.
            device.swipe(CENTER_X, CENTER_Y, CENTER_X - SWIPE, CENTER_Y - SWIPE, SWIPE_STEPS)
            device.wait(Until.hasObject(By.descStartsWith("Vue auto")), WAIT_MS)
            device.findObject(By.descStartsWith("Vue auto"))?.click()
            device.findObject(By.desc("Carte et légende"))?.click()
            device.wait(Until.hasObject(By.text("OpenStreetMap (Protomaps)")), WAIT_MS)
            device.findObject(By.text("OpenStreetMap (Protomaps)"))?.click()
            device.wait(Until.hasObject(By.descContains("(Protomaps), affichée")), WAIT_MS)
            device.pressBack()
            device.waitForIdle()
        }

    private companion object {
        const val WAIT_MS = 30_000L
        const val CENTER_X = 400
        const val CENTER_Y = 1300
        const val SWIPE = 300
        const val SWIPE_STEPS = 20
    }
}
