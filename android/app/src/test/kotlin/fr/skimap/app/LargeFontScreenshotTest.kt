package fr.skimap.app

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.data.Appearance
import fr.skimap.app.ui.CardContent
import fr.skimap.app.ui.MapActions
import fr.skimap.app.ui.MapSheetContent
import fr.skimap.app.ui.WelcomeScreen
import fr.skimap.app.ui.WelcomeTags
import fr.skimap.core.Lang
import fr.skimap.core.pisteCard
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The menus and cards at the large font sizes of Android (×1.3 and ×2), French (the
 * longest texts): nothing may overlap or be cut. References in `src/test/screenshots`.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-xxhdpi")
class LargeFontScreenshotTest {
    @get:Rule
    val rule = createComposeRule()

    private fun capture(name: String) = rule.onRoot().captureRoboImage("src/test/screenshots/$name.png")

    private fun map(scale: Float) {
        rule.setContent { Themed(fontScale = scale) { TestMapScreen(readyState(free = true), mapMoved = true) } }
        capture("font_${tag(scale)}_map")
    }

    private fun sheetAppearance(scale: Float) {
        rule.setContent {
            Themed(fontScale = scale) {
                Surface(Modifier.fillMaxSize()) {
                    MapSheetContent(readyState(), MapActions(), {}, TODAY, PARIS, appearance = Appearance.AUTO)
                }
            }
        }
        rule.onNodeWithText("Apparence").performScrollTo()
        capture("font_${tag(scale)}_sheet_appearance")
    }

    private fun pisteCardSheet(scale: Float) {
        val card =
            pisteCard(
                """{"name":"Souliers","difficulty":"intermediate","classic":true,"skating":true,"length_m":322.9,
                   "oneway":false,"state":"closed","state_at":"2026-04-06","state_source":"nordicfrance",
                   "relations":[{"name":"Souliers"},{"name":"Izoard"}]}""",
                "Cervières",
                listOf(
                    """{"name":"Nordic Pass saison Alpes du Sud",
                       "prices":[{"label":"Adulte, plein tarif","price_eur":205.0,"date":"2026-2027"}]}""",
                ),
                Lang.FR,
            )!!
        rule.setContent { Themed(fontScale = scale) { Surface(Modifier.fillMaxSize()) { CardContent(card, {}, {}) } } }
        capture("font_${tag(scale)}_card_piste")
    }

    private fun welcome(scale: Float) {
        rule.setContent { Themed(fontScale = scale) { WelcomeScreen(hasPack = true, onSkip = {}, onStart = {}) } }
        rule.onNodeWithTag(WelcomeTags.NEXT).performScrollTo().performClick()
        capture("font_${tag(scale)}_welcome_last")
    }

    private fun tag(scale: Float) = if (scale < 2f) "130" else "200"

    @Test fun map130() = map(1.3f)

    @Test fun map200() = map(2f)

    @Test fun sheetAppearance130() = sheetAppearance(1.3f)

    @Test fun sheetAppearance200() = sheetAppearance(2f)

    @Test fun pisteCard130() = pisteCardSheet(1.3f)

    @Test fun pisteCard200() = pisteCardSheet(2f)

    @Test fun welcome130() = welcome(1.3f)

    @Test fun welcome200() = welcome(2f)
}
