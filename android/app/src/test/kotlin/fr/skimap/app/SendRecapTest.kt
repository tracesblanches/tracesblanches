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
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.day.PlanActions
import fr.skimap.app.day.PlanScreen
import fr.skimap.sport.Delivery
import fr.skimap.sport.Outing
import fr.skimap.sport.Queued
import fr.skimap.sport.SportTags
import fr.skimap.sport.SportType
import fr.skimap.sport.Step
import fr.skimap.tracking.Recap
import fr.skimap.tracking.TrackingState
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** The recap of the outing with « Envoyer » (K11), captured in `src/test/screenshots/day_plan_ended_send.png`. */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class SendRecapTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun recapOffersToSendAndShowsWhereItIs() {
        var sends = 0
        val ended = TrackingState.Ended("/plan", Recap(true, 6.2, 70.0, "Sortie terminée. 6,2 kilomètres en 1 h 10."), "/plan/trace.gpx")
        val sent =
            Queued(
                Outing("x", "/q.gpx", "Boucle du Chaumoz", "", SportType.NORDIC_SKI, 0, 1, 6.2),
                Delivery(Step.DONE, "hc-1"),
                Delivery(Step.PENDING),
                0,
            )
        rule.setContent {
            Themed {
                Surface(
                    Modifier.fillMaxSize(),
                ) { PlanScreen(DayFixtures.plan, ended, PlanActions(sendOuting = { sends++ }), sent = sent) }
            }
        }
        rule.onNodeWithText("Sortie terminée. 6,2 kilomètres en 1 h 10.").assertIsDisplayed()
        rule.onNodeWithText("Boucle du Chaumoz · Health Connect envoyée · Strava en attente").assertIsDisplayed()
        rule.onRoot().captureRoboImage("src/test/screenshots/day_plan_ended_send.png")
        rule.onNodeWithTag(SportTags.SEND).performClick()
        assertEquals(1, sends)
    }
}
