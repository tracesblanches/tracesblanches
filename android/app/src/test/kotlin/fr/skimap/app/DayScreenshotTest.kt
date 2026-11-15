package fr.skimap.app

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.day.DayActions
import fr.skimap.app.day.DayScreen
import fr.skimap.app.day.PlanActions
import fr.skimap.app.day.PlanScreen
import fr.skimap.app.guidance.GuidanceOverlayContent
import fr.skimap.tracking.Phase
import fr.skimap.tracking.TrackingState
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Captures of the morning, the plan and the guidance over the map (Roborazzi, references
 * in `src/test/screenshots/day_*.png`); the map is the snow-white stand-in.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class DayScreenshotTest {
    @get:Rule
    val rule = createComposeRule()

    private fun capture(name: String) = rule.onRoot().captureRoboImage("src/test/screenshots/$name.png")

    @Test
    fun morningLight() {
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { DayScreen(DayFixtures.ready(), DayActions()) } } }
        capture("day_morning_light")
    }

    @Test
    fun morningDark() {
        rule.setContent {
            Themed(
                dark = true,
            ) { Surface(Modifier.fillMaxSize()) { DayScreen(DayFixtures.ready(choice = 1), DayActions()) } }
        }
        capture("day_morning_dark")
    }

    @Test
    fun morningLargeFont() {
        rule.setContent { Themed(fontScale = 2f) { Surface(Modifier.fillMaxSize()) { DayScreen(DayFixtures.ready(), DayActions()) } } }
        capture("day_morning_large_font")
    }

    @Test
    fun planDuringTheOuting() {
        val s =
            TrackingState.Running(
                DayFixtures.plan.planDir,
                DayFixtures.guidance(),
                null,
                "Dans deux cent cinquante mètres, tournez à gauche.",
            )
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { PlanScreen(DayFixtures.plan, s, PlanActions()) } } }
        capture("day_plan_running")
    }

    private fun overMap(
        state: TrackingState,
        dark: Boolean = false,
        fontScale: Float = 1f,
    ) {
        rule.setContent {
            Themed(dark = dark, fontScale = fontScale) {
                Box(Modifier.fillMaxSize()) {
                    FakeMap()
                    Box(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing).padding(8.dp)) {
                        GuidanceOverlayContent(state, onOpen = {}, onAnswer = {}, onRefuseSwitch = {})
                    }
                }
            }
        }
    }

    @Test
    fun guidanceBarOverTheMap() {
        overMap(TrackingState.Running("p", DayFixtures.guidance(), null, null))
        capture("day_guidance_bar")
    }

    @Test
    fun guidanceBarDarkLargeFont() {
        overMap(TrackingState.Running("p", DayFixtures.guidance(phase = Phase.PAUSED), null, null), dark = true, fontScale = 2f)
        capture("day_guidance_bar_dark_large_font")
    }

    @Test
    fun guidanceProposal() {
        overMap(TrackingState.Running("p", DayFixtures.guidance(proposal = DayFixtures.proposal), null, null))
        capture("day_guidance_proposal")
    }

    @Test
    fun dayEntryOverTheMap() {
        overMap(TrackingState.Idle)
        capture("day_entry")
    }
}
