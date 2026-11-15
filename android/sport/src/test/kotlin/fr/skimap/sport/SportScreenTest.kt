package fr.skimap.sport

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** « Envoyer mes sorties » in its states, captured (src/test/screenshots/sport_*.png), and its gestures. */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class SportScreenTest {
    @get:Rule
    val rule = createComposeRule()

    private fun outing(
        name: String,
        health: Delivery,
        strava: Delivery,
    ) = Queued(Outing(name, "/x.gpx", name, "", SportType.NORDIC_SKI, 0, 1, 6.2), health, strava, 0)

    private fun show(
        s: SportUi,
        dark: Boolean = false,
        actions: SportActions = SportActions(),
    ) = rule.setContent {
        MaterialTheme(colorScheme = if (dark) darkColorScheme() else lightColorScheme()) {
            Surface(Modifier.fillMaxSize()) { SportScreen(s, actions) }
        }
    }

    private fun capture(name: String) = rule.onRoot().captureRoboImage("src/test/screenshots/$name.png")

    @Test
    fun stravaNotConfiguredYet() {
        // What the app shows today: skimap.warnier.net answers `configured: false`.
        show(SportUi(health = HealthUi.NOT_ALLOWED, strava = StravaUi.NotConfigured))
        rule.onNodeWithText("La connexion à Strava sera possible quand le service skimap sera configuré pour Strava.").assertIsDisplayed()
        rule.onNodeWithTag(SportTags.HEALTH_ALLOW).assertIsDisplayed()
        capture("sport_not_configured")
    }

    @Test
    fun connectedWithQueue() {
        var auto: Boolean? = null
        var disconnect = 0
        show(
            SportUi(
                health = HealthUi.READY,
                strava = StravaUi.Connected("Ada L."),
                autoSend = false,
                queue =
                    listOf(
                        outing("Boucle du Chaumoz", Delivery(Step.DONE, "hc"), Delivery(Step.PENDING)),
                        outing("Les Fonts", Delivery(Step.DONE, "hc"), Delivery(Step.DONE, "7000001")),
                    ),
            ),
            actions = SportActions(setAutoSend = { auto = it }, disconnectStrava = { disconnect++ }),
        )
        rule.onNodeWithText("Connecté : Ada L.").assertIsDisplayed()
        rule.onNodeWithText("Boucle du Chaumoz · Health Connect envoyée · Strava en attente").assertExists()
        capture("sport_connected")
        rule.onNodeWithTag(SportTags.AUTO).performClick()
        assertEquals(true, auto)
        rule.onNodeWithTag(SportTags.STRAVA_DISCONNECT).performClick()
        assertEquals(1, disconnect)
    }

    @Test
    fun disconnectedDark() {
        var connect = 0
        show(
            SportUi(
                health = HealthUi.NEEDS_UPDATE,
                strava = StravaUi.Disconnected,
                autoSend = true,
                queue = listOf(outing("Boucle du Chaumoz", Delivery(Step.OFF, Sport.NOT_ALLOWED), Delivery(Step.OFF, Sport.NOT_CONNECTED))),
            ),
            dark = true,
            actions = SportActions(connectStrava = { connect++ }),
        )
        rule.onNodeWithText("Boucle du Chaumoz · Health Connect non autorisé · Strava non connecté").assertExists()
        capture("sport_disconnected_dark")
        rule.onNodeWithTag(SportTags.STRAVA_CONNECT).performClick()
        assertEquals(1, connect)
    }

    @Test
    fun rationale() {
        rule.setContent { MaterialTheme { Surface(Modifier.fillMaxSize()) { HealthRationale(onClose = {}) } } }
        rule.onNodeWithText("skimap et Health Connect").assertIsDisplayed()
        capture("sport_health_rationale")
    }

    @Test
    fun sendButtonOfTheRecap() {
        var sent = 0
        rule.setContent {
            MaterialTheme {
                Surface {
                    SendOuting(
                        outing("Boucle du Chaumoz", Delivery(Step.DONE, "hc"), Delivery(Step.UPLOADING, "9001")),
                        onSend = { sent++ },
                    )
                }
            }
        }
        rule.onNodeWithTag(SportTags.SEND).performClick()
        assertEquals(1, sent)
        rule.onNodeWithText("Boucle du Chaumoz · Health Connect envoyée · Strava en traitement").assertExists()
        capture("sport_send_button")
    }
}
