package fr.skimap.app

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.day.PlanActions
import fr.skimap.app.day.PlanScreen
import fr.skimap.sport.ShareFormat
import fr.skimap.sport.SportTags
import fr.skimap.tracking.Recap
import fr.skimap.tracking.TrackingState
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** The recap of the outing with its share (#63), captured in `src/test/screenshots/day_plan_ended_send.png`. */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class SendRecapTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun recapOffersToShare() {
        val shared = ArrayList<ShareFormat>()
        val ended = TrackingState.Ended("/plan", Recap(true, 6.2, 70.0, "Sortie terminée. 6,2 kilomètres en 1 h 10."), "/plan/trace.gpx")
        rule.setContent {
            Themed {
                Surface(
                    Modifier.fillMaxSize(),
                ) { PlanScreen(DayFixtures.plan, ended, PlanActions(shareOuting = { shared += it })) }
            }
        }
        rule.onNodeWithText("Sortie terminée. 6,2 kilomètres en 1 h 10.").assertIsDisplayed()
        rule.onNodeWithTag(SportTags.SHARE_FIT).performScrollTo()
        rule.onRoot().captureRoboImage("src/test/screenshots/day_plan_ended_send.png")
        rule.onNodeWithTag(SportTags.SHARE_FIT).performClick()
        rule.onNodeWithTag(SportTags.SHARE_GPX).performScrollTo().performClick()
        assertEquals(listOf(ShareFormat.FIT, ShareFormat.GPX), shared)
    }

    @Test
    fun noTraceNoShare() {
        val ended = TrackingState.Ended("/plan", null, null)
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { PlanScreen(DayFixtures.plan, ended, PlanActions()) } } }
        rule.onNodeWithTag(SportTags.SHARE_FIT).assertDoesNotExist()
    }
}
