package fr.skimap.app

import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsNodeInteractionCollection
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import fr.skimap.app.ui.MapActions
import fr.skimap.app.ui.MapSheetContent
import fr.skimap.app.ui.MapUiState
import fr.skimap.app.ui.NoPackScreen
import fr.skimap.app.ui.Tags
import fr.skimap.core.StatusSummary
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** Compose tests of the screens: actions, texts, 72 dp targets, recompositions. */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "w412dp-h915dp-xxhdpi")
class MapScreenTest {
    @get:Rule
    val rule = createComposeRule()

    /**
     * Every node that reacts to a tap is at least [min] dp square: 48 on the map screen
     * (Android standard), 72 in the sheet and the other screens (gloves, K12 #115).
     */
    private fun SemanticsNodeInteractionCollection.assertTargetsAtLeast(min: Int) {
        val nodes = fetchSemanticsNodes()
        assertTrue("no clickable node", nodes.isNotEmpty())
        val density = rule.density
        for (n in nodes) {
            // Layout size, not the visible bounds: rows scrolled out of the sheet count too.
            val w = with(density) { n.size.width.toDp() }
            val h = with(density) { n.size.height.toDp() }
            val label =
                n.config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString()
                    ?: n.config.getOrNull(SemanticsProperties.Text)?.joinToString()
                    ?: n.config.getOrNull(SemanticsProperties.TestTag)
            assertTrue("$label: $w × $h", w >= min.dp && h >= min.dp)
        }
    }

    @Test
    fun mapComesFirstWithSmallControls() {
        rule.setContent { Themed { TestMapScreen(readyState()) } }
        rule.onNodeWithText("Cervières · 0 ouverte, 8 fermées").assertIsDisplayed()
        // The validity is in the sheet, not on the map while the pack is valid.
        rule.onNodeWithText("Valable jusqu'au 17 novembre 2026").assertDoesNotExist()
        // Primary action and ⓘ only; no zoom button, no recentre while the view is automatic.
        assertEquals(2, rule.onAllNodes(hasClickAction()).fetchSemanticsNodes().size)
        rule.onNodeWithTag(Tags.PRIMARY).assertIsDisplayed()
        rule.onNodeWithTag(Tags.ATTRIBUTION).assertIsDisplayed()
        rule.onNodeWithTag(Tags.RECENTER).assertDoesNotExist()
        rule.onAllNodes(hasClickAction()).assertTargetsAtLeast(48)
        // Small controls: the map keeps the screen.
        val primary = rule.onNodeWithTag(Tags.PRIMARY).fetchSemanticsNode()
        assertTrue(with(rule.density) { primary.size.width.toDp() } <= 56.dp)
    }

    @Test
    fun creditsOpenFromTheInfoButton() {
        rule.setContent { Themed { TestMapScreen(readyState()) } }
        rule.onNodeWithContentDescription("Sources et licences des données").performClick()
        rule.onNodeWithText("Licences des données").assertIsDisplayed()
        rule.onNodeWithText("© IGN – Plan IGN v2, mise à jour du 2026-10-08, Licence Ouverte Etalab 2.0").assertIsDisplayed()
        rule.onNodeWithText("Fermer").performClick()
        rule.onNodeWithText("Licences des données").assertDoesNotExist()
    }

    @Test
    fun autoViewButtonOnceTheMapWasMoved() {
        var recenter = 0
        rule.setContent { Themed { TestMapScreen(readyState(), MapActions(recenter = { recenter++ }), mapMoved = true) } }
        assertEquals(3, rule.onAllNodes(hasClickAction()).fetchSemanticsNodes().size)
        rule.onAllNodes(hasClickAction()).assertTargetsAtLeast(48)
        rule.onNodeWithContentDescription("Vue auto : recentrer sur Cervières").performClick()
        assertEquals(1, recenter)
    }

    @Test
    fun largeFontKeepsEveryTargetReachable() {
        rule.setContent { Themed(fontScale = 2f) { TestMapScreen(readyState(), mapMoved = true) } }
        rule.onAllNodes(hasClickAction()).assertTargetsAtLeast(48)
        rule.onNodeWithTag(Tags.PRIMARY).assertIsDisplayed()
        rule.onNodeWithTag(Tags.RECENTER).assertIsDisplayed()
    }

    @Test
    fun primaryActionOpensTheSheet() {
        val chosen = mutableListOf<String>()
        rule.setContent {
            Themed { TestMapScreen(readyState(), MapActions(selectBasemap = { chosen += it }, chooseDomain = { chosen += "domains" })) }
        }
        rule.onNodeWithTag(Tags.PRIMARY).performClick()
        rule.onNodeWithTag(Tags.SHEET).assertIsDisplayed()
        rule.onNodeWithText("OpenStreetMap (Protomaps)").performScrollTo().performClick()
        // No list of sites in the menu: the areas of France are in the picker.
        rule.onNodeWithText("Chapelle-des-Bois").assertDoesNotExist()
        rule.onNodeWithText("Choisir un domaine en France").performScrollTo().performClick()
        assertEquals(listOf("osm", "domains"), chosen)
    }

    @Test
    fun sheetContentTargetsAndLegendText() {
        rule.setContent { Themed { MapSheetContent(readyState(), {}, {}, {}, TODAY, PARIS) } }
        // Colour is never alone: every difficulty has its name in words.
        listOf("Verte : très facile", "Bleue : facile", "Rouge : difficile", "Noire : très difficile").forEach {
            rule.onNodeWithText(it).performScrollTo().assertIsDisplayed()
        }
        rule.onNodeWithText("Croix : itinéraire fermé").performScrollTo().assertIsDisplayed()
        rule.onNodeWithText("Pack 20261115T014538Z · ouvert en 196 ms · skimap-ffi 0.1.0").performScrollTo()
        rule.onAllNodes(hasClickAction()).assertTargetsAtLeast(72)
    }

    @Test
    fun layerSwitchesInTheSheet() {
        val changes = mutableListOf<Pair<String, Boolean>>()
        val state = readyState().copy(hidden = setOf("no-network"))
        rule.setContent {
            Themed { MapSheetContent(state, {}, {}, {}, TODAY, PARIS, onSetLayerVisible = { id, v -> changes += id to v }) }
        }
        rule.onNodeWithText("Couches").performScrollTo().assertIsDisplayed()
        rule
            .onNodeWithText("Refuges, cabanes et abris")
            .performScrollTo()
            .assertIsOn()
            .performClick()
        rule
            .onNodeWithText("Zones sans réseau mobile (112 compris)")
            .performScrollTo()
            .assertIsOff()
            .performClick()
        assertEquals(listOf("refuges" to false, "no-network" to true), changes)
        // Pistes and their status are never offered as a switch.
        val switches = rule.onAllNodes(isToggleable(), useUnmergedTree = false).fetchSemanticsNodes()
        assertEquals(4, switches.size)
        rule.onAllNodes(isToggleable() and hasText("Piste", substring = true, ignoreCase = true)).assertCountEquals(0)
        rule.onNodeWithText("Toujours affichés", substring = true).performScrollTo().assertIsDisplayed()
        rule.onAllNodes(hasClickAction()).assertTargetsAtLeast(72)
    }

    @Test
    fun noPackScreenTargets() {
        var retries = 0
        rule.setContent {
            Themed { NoPackScreen(MapUiState.NoPack("/sdcard/packs", emptyList()), onRetry = { retries++ }) }
        }
        rule.onAllNodes(hasClickAction()).assertTargetsAtLeast(72)
        rule.onNodeWithText("Chercher à nouveau").performClick()
        assertEquals(1, retries)
    }

    @Test
    fun expiredPackIsSaidInWords() {
        val expired = readyState().let { it.copy(opened = it.opened.copy(info = Fixtures.info(validUntil = "2026-11-14"))) }
        rule.setContent { Themed { TestMapScreen(expired) } }
        rule.onNodeWithText("Périmé depuis le 14 novembre 2026 : mettez le pack à jour").assertIsDisplayed()
    }

    /**
     * The map is the expensive part of the screen: an error message, a status change or
     * the sheet must not recompose it.
     */
    @Test
    fun mapIsNotRecomposedByTheRestOfTheScreen() {
        var mapCompositions = 0
        var headerState by mutableStateOf(readyState())
        val map: @Composable () -> Unit = {
            SideEffect { mapCompositions++ }
            FakeMap()
        }
        rule.setContent { Themed { TestMapScreen(headerState, map = map) } }
        rule.waitForIdle()
        assertEquals(1, mapCompositions)

        headerState = readyState(error = "Fond de carte « x » absent du pack")
        rule.waitForIdle()
        rule.onNodeWithText("Fond de carte « x » absent du pack").assertIsDisplayed()
        val withStatus =
            headerState.let { s ->
                s.copy(opened = s.opened.copy(info = s.opened.info.copy(status = StatusSummary(2u, 6u, 0u, null))))
            }
        headerState = withStatus
        rule.waitForIdle()
        rule.onNodeWithText("Cervières · 2 ouvertes, 6 fermées").assertIsDisplayed()

        rule.onNodeWithTag(Tags.PRIMARY).performClick()
        rule.waitForIdle()
        assertEquals("map recomposed by header, status or sheet", 1, mapCompositions)
    }
}
