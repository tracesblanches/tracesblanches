package fr.skimap.app

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import fr.skimap.app.day.DayActions
import fr.skimap.app.day.DayInputs
import fr.skimap.app.day.DayScreen
import fr.skimap.app.day.DayTags
import fr.skimap.app.day.PlanActions
import fr.skimap.app.day.PlanScreen
import fr.skimap.app.day.PlanTags
import fr.skimap.app.day.groupSpec
import fr.skimap.app.guidance.GuidanceOverlayContent
import fr.skimap.app.guidance.GuidanceTags
import fr.skimap.core.GroupSpec
import fr.skimap.tracking.Phase
import fr.skimap.tracking.TrackingState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** The morning, the plan and the guidance overlay: actions, texts, sizes. */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-xxhdpi")
class DayScreenTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun morningChoicesAndValidateInOneGesture() {
        val inputs = ArrayList<DayInputs>()
        var chosen = -1
        var validated = 0
        val state = DayFixtures.ready()
        rule.setContent {
            Themed {
                DayScreen(
                    state,
                    DayActions(
                        setInputs = { f -> inputs += f(state.inputs) },
                        choose = { chosen = it },
                        validate = { validated++ },
                    ),
                )
            }
        }
        // The choices of the morning, each one a gesture.
        rule.onNodeWithText("3 h").performClick()
        rule.onNodeWithText("Skating").performClick()
        rule.onNodeWithText("Vertes et bleues").performClick()
        assertEquals(listOf(180, 120, 120), inputs.map { it.budgetMin })
        assertTrue(inputs[1].skating && inputs[2].easyOnly)
        // The loops with the group's range and the warning when it passes the limit.
        rule.onNodeWithTag(DayTags.LIST).performScrollToNode(hasTestTag(DayTags.LOOP + 0))
        rule
            .onNodeWithText(
                "2 h 35 à 6 h 10 · médiane 3 h 40, prévu sur 4 h 50 (côté lent), retour prévu vers 14 h 50",
                useUnmergedTree = true,
            ).assertIsDisplayed()
        rule.onNodeWithTag(DayTags.LOOP + 0).assertIsSelected()
        rule.onNodeWithTag(DayTags.LIST).performScrollToNode(hasTestTag(DayTags.LOOP + 1))
        rule
            .onNodeWithText(
                "Retour prévu après la limite de 16 h 00 : boucle longue pour ce groupe",
                useUnmergedTree = true,
            ).assertIsDisplayed()
        rule.onNodeWithText("Si tout va bien", useUnmergedTree = true).assertIsDisplayed()
        rule.onNodeWithTag(DayTags.LOOP + 1).performClick()
        assertEquals(1, chosen)
        // « Valider la journée »: one button, glove size, always visible.
        rule
            .onNodeWithTag(DayTags.VALIDATE)
            .assertIsDisplayed()
            .assertIsEnabled()
            .assertHeightIsAtLeast(72.dp)
            .performClick()
        assertEquals(1, validated)
    }

    @Test
    fun theGroupTypedInTheDialog() {
        // The dialog itself is driven on the emulator (DayFlowTest): its text fields never
        // get idle under Robolectric.
        val g = groupSpec(" Avec Léa ", "2", "4, 11 ;16", "beginner")
        assertEquals("Avec Léa", g.name)
        assertEquals(2u.toUByte(), g.adults)
        assertEquals(listOf<Byte>(4, 11, 16), g.children.toList())
        assertEquals(listOf<Byte>(17), groupSpec("x", "0", "40", "expert").children.toList())
        assertEquals(1u.toUByte(), groupSpec("x", "", "", "expert").adults)
    }

    @Test
    fun noValidationWhileComputing() {
        rule.setContent { Themed { DayScreen(DayFixtures.ready(computing = true), DayActions()) } }
        rule.onNodeWithTag(DayTags.VALIDATE).assertIsNotEnabled()
    }

    @Test
    fun planActions112AndSmsAreOneGestureEach() {
        val did = ArrayList<String>()
        val running =
            TrackingState.Running(
                DayFixtures.plan.planDir,
                DayFixtures.guidance(),
                null,
                "Dans deux cent cinquante mètres, tournez à gauche.",
            )
        rule.setContent {
            Themed {
                PlanScreen(
                    DayFixtures.plan,
                    running,
                    PlanActions(
                        send = { did += "sms" },
                        emergency = { did += "112" },
                        bringBack = { did += "back" },
                        stop = { did += "stop" },
                    ),
                )
            }
        }
        rule.onNodeWithContentDescription("Appeler le 112 : ouvre le composeur").assertHeightIsAtLeast(72.dp).performClick()
        rule.onNodeWithTag(PlanTags.SEND).assertHeightIsAtLeast(72.dp).performClick()
        rule.onNodeWithTag(PlanTags.BACK).assertHeightIsAtLeast(72.dp).performClick()
        // Stopping asks first: no stop by a stray touch.
        rule.onNodeWithTag(PlanTags.STOP).performClick()
        assertEquals(listOf("112", "sms", "back"), did)
        rule.onNode(hasText("Arrêter la sortie") and hasAnyAncestor(isDialog())).performClick()
        assertEquals(listOf("112", "sms", "back", "stop"), did)
        rule.onNodeWithText("Retour prévu 13 h 55", substring = true).assertIsDisplayed()
        rule.onNodeWithText("Au plus tard 16 h 00").assertIsDisplayed()
    }

    @Test
    fun overlayIsAThinBarDuringTheOutingAndASmallEntryOtherwise() {
        var state: TrackingState by androidx.compose.runtime.mutableStateOf<TrackingState>(TrackingState.Idle)
        var opened = 0
        val answers = ArrayList<Boolean>()
        rule.setContent {
            Themed {
                Box(Modifier.fillMaxSize().padding(8.dp)) {
                    FakeMap()
                    GuidanceOverlayContent(state, onOpen = { opened++ }, onAnswer = { answers += it }, onRefuseSwitch = {})
                }
            }
        }
        rule
            .onNodeWithTag(GuidanceTags.DAY)
            .assertIsDisplayed()
            .assertHeightIsAtLeast(48.dp)
            .performClick()
        assertEquals(1, opened)
        state = TrackingState.Running("p", DayFixtures.guidance(), null, null)
        rule.waitForIdle()
        val bar = rule.onNodeWithTag(GuidanceTags.BAR).assertIsDisplayed().fetchSemanticsNode()
        val h = with(rule.density) { bar.size.height.toDp() }
        // Thin: two lines, it does not hide the map.
        assertTrue("bar $h", h < 80.dp)
        rule.onNodeWithText("250 m à gauche · Chaumoz").assertIsDisplayed()
        rule.onNodeWithText("1 h 05 · arrivée 12 h 40").assertIsDisplayed()
        // A proposal: accept or refuse, one gesture.
        state = TrackingState.Running("p", DayFixtures.guidance(proposal = DayFixtures.proposal), null, null)
        rule.waitForIdle()
        rule.onNodeWithTag(GuidanceTags.ACCEPT).assertHeightIsAtLeast(56.dp).performClick()
        rule.onNodeWithTag(GuidanceTags.DECLINE).performClick()
        assertEquals(listOf(true, false), answers)
        // Paused: said so on the second line.
        state = TrackingState.Running("p", DayFixtures.guidance(phase = Phase.PAUSED), null, null)
        rule.waitForIdle()
        rule.onNodeWithText("Pause · 1 h 05 · arrivée 12 h 40").assertIsDisplayed()
    }
}
