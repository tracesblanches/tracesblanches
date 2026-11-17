package fr.skimap.app

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
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.ui.CARD_NOTICE_TAG
import fr.skimap.app.ui.CardContent
import fr.skimap.core.Card
import fr.skimap.core.Lang
import fr.skimap.core.featureCard
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** Cards of touched features: built by the Rust core from the map properties, shown here. */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class CardTest {
    @get:Rule
    val rule = createComposeRule()

    /** Properties as MapLibre gives them for an OSM refuge (nested tags). */
    private fun card(lang: Lang = Lang.FR): Card =
        checkNotNull(
            featureCard(
                "refuges-osm",
                """{"kind":"alpine_hut","osm_type":"way","osm_id":171614493,
                   "tags":{"name":"Refuge de la Blanche","phone":"+33 4 00 00 72 52","website":"https://example.org/blanche",
                           "beds":"30","ele":"2499","fireplace":"yes"}}""",
                lang,
            ),
        )

    @Test
    fun dialsOnlyOnTouchAndOpensLinks() {
        val dialled = mutableListOf<String>()
        val opened = mutableListOf<String>()
        val c = card()
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { CardContent(c, { dialled += it }, { opened += it }) } } }
        rule.onNodeWithText("Refuge de la Blanche").assertIsDisplayed()
        rule.onNodeWithText("30").assertIsDisplayed()
        assertTrue(dialled.isEmpty())
        rule.onNodeWithText("Appeler : Téléphone, +33 4 00 00 72 52").performClick()
        assertEquals(listOf("+33400007252"), dialled)
        rule.onNodeWithText("Fiche OpenStreetMap").performScrollTo().performClick()
        assertEquals(listOf("https://www.openstreetmap.org/way/171614493"), opened)
        rule.onAllNodes(hasClickAction()).fetchSemanticsNodes().forEach {
            assertTrue(with(rule.density) { it.size.height.toDp() }.value >= 72f)
        }
    }

    @Test
    fun capture() {
        val c = card()
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { CardContent(c, {}, {}) } } }
        rule.onRoot().captureRoboImage("src/test/screenshots/card_refuge.png")
    }

    /** The same card on a phone in English: labels of the core in English (VO6). */
    @Test
    @Config(qualifiers = "en-w412dp-h915dp-hdpi")
    fun captureEnglish() {
        val c = card(Lang.EN)
        assertEquals("Hut", c.kind)
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { CardContent(c, {}, {}) } } }
        rule.onNodeWithText("OpenStreetMap page").performScrollTo().assertIsDisplayed()
        rule.onRoot().captureRoboImage("src/test/screenshots/card_refuge_en.png")
    }

    /** A piste of the official plan absent from OSM (pack layer `pistes-estimees`). */
    private fun estimated(): Card =
        checkNotNull(
            featureCard(
                "pistes-estimees",
                """{"name":"Le Gy","difficulty":"novice","certainty":"incertain","method":"estimated_paths",
                   "lengths_km_plan":[3.5],"departures":["Chapelle-des-Bois"],
                   "plan_url":"https://www.espacenordiquejurassien.com/plan.pdf"}""",
                Lang.FR,
            ),
        )

    @Test
    fun estimatedPisteSaysUncertainAndOpensThePlan() {
        val opened = mutableListOf<String>()
        val c = estimated()
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { CardContent(c, {}, { opened += it }) } } }
        rule.onNodeWithText("Tracé incertain, d'après le plan de la station").assertIsDisplayed()
        rule.onNodeWithText("Voir le plan officiel").performScrollTo().performClick()
        assertEquals(listOf("https://www.espacenordiquejurassien.com/plan.pdf"), opened)
    }

    @Test
    fun offlineLinkSaysSo() {
        val c = estimated()
        val text = "Pas de réseau : ce lien ne s'ouvre pas hors ligne. Réessaie une fois connecté."
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { CardContent(c, {}, {}, notice = text) } } }
        rule.onNodeWithTag(CARD_NOTICE_TAG).performScrollTo().assertIsDisplayed()
        rule.onNodeWithText(text).assertIsDisplayed()
    }

    @Test
    fun captureEstimated() {
        val c = estimated()
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { CardContent(c, {}, {}) } } }
        rule.onRoot().captureRoboImage("src/test/screenshots/card_piste_estimee.png")
    }
}
