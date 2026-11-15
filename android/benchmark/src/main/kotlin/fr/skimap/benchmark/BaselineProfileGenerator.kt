package fr.skimap.benchmark

import androidx.benchmark.macro.junit4.BaselineProfileRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.uiautomator.By
import androidx.test.uiautomator.Until
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.regex.Pattern

/**
 * Baseline Profile of the critical path: cold start, pack opening, first map, a map
 * gesture and the "Vue auto" button, the primary action (sheet) and a practice switch
 * (another validated style). French or English texts (the language of the device).
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
            device.wait(Until.hasObject(By.desc(AUTO_VIEW)), WAIT_MS)
            device.findObject(By.desc(AUTO_VIEW))?.click()
            device.findObject(By.desc(SHEET))?.click()
            device.wait(Until.hasObject(By.text(SRN)), WAIT_MS)
            device.findObject(By.text(SRN))?.click()
            device.wait(Until.hasObject(By.desc(SRN_SHOWN)), WAIT_MS)
            device.pressBack()
            device.waitForIdle()
        }

    private companion object {
        val AUTO_VIEW: Pattern = Pattern.compile("(Vue auto|Auto view).*")
        val SHEET: Pattern = Pattern.compile("Carte et légende|Map and legend")
        val SRN: Pattern = Pattern.compile("Ski de randonnée nordique, hors piste|Nordic backcountry skiing, off piste")
        val SRN_SHOWN: Pattern = Pattern.compile(".*(hors piste, affichée|off piste, shown)")
        const val WAIT_MS = 30_000L
        const val CENTER_X = 400
        const val CENTER_Y = 1300
        const val SWIPE = 300
        const val SWIPE_STEPS = 20
    }
}
