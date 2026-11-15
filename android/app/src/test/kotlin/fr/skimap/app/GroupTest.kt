package fr.skimap.app

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.group.GroupMapLayer
import fr.skimap.app.group.RustGroupBackend
import fr.skimap.core.Lang
import fr.skimap.group.GroupActions
import fr.skimap.group.GroupAlertKind
import fr.skimap.group.GroupInfo
import fr.skimap.group.GroupLive
import fr.skimap.group.GroupScreen
import fr.skimap.group.GroupTags
import fr.skimap.group.GroupUi
import fr.skimap.group.Member
import fr.skimap.group.OwnFix
import fr.skimap.group.OwnPhase
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The group mode (N2) in the app: the Rust core through UniFFI (two phones exchanging
 * frames), the group screen (Compose) and its captures (Roborazzi, `group_*.png`).
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class GroupTest {
    @get:Rule
    val rule = createComposeRule()

    private val backend = RustGroupBackend { Lang.FR }

    private fun capture(name: String) = rule.onRoot().captureRoboImage("src/test/screenshots/$name.png")

    private fun group(): GroupInfo =
        backend.create(ByteArray(32) { (it * 7 + 3).toByte() }, listOf("Pierre", "Léa", "Jo"), null, 1_768_000_000)

    private val live =
        GroupLive(
            members =
                listOf(
                    Member(0, "Pierre", true, 44.87, 6.73, 0, 0, stopped = false, offRoute = false, batteryLow = false, distanceM = 0.0),
                    Member(1, "Léa", false, 44.866, 6.73, 380, 1, stopped = true, offRoute = false, batteryLow = true, distanceM = 447.0),
                    Member(2, "Jo", false, null, null, null, null, stopped = false, offRoute = false, batteryLow = false, distanceM = null),
                ),
            radio = true,
            heard = 12,
        )

    @Test
    fun twoPhonesThroughTheCore() {
        val g = group()
        assertEquals(g, backend.read(g.qrText))
        val qr = backend.qr(g.qrText)
        assertEquals(qr.width * qr.width, qr.modules.size)
        val a = backend.open(g.qrText, 0, null)
        val b = backend.open(g.qrText, 1, null)
        val t0 = 1_768_036_800_000L

        fun fix(
            t: Long,
            lat: Double,
        ) = OwnFix(t, lat, 6.73, OwnPhase.MOVING, offRoute = false, progressM = null, batteryLow = false)
        val tb = b.tick(t0, fix(t0, 44.87), emptyList())
        assertEquals(21, tb.ownFrame!!.size)
        assertEquals(3, tb.prefixes.size)
        val ta = a.tick(t0, fix(t0, 44.87), listOf(tb.ownFrame!!))
        assertEquals(1, ta.newPositions)
        assertEquals(0, ta.members.first { it.number == 1 }.hops)
        // Six minutes later, 600 m apart, Léa not heard: separation alert (no common plan: distance).
        val now = t0 + 360_000
        val late = a.tick(now, fix(now, 44.87 + 600 / 111_195.0), emptyList())
        assertEquals(GroupAlertKind.STRAGGLER, late.alerts.single().kind)
        assertTrue(
            late.alerts.single().text,
            late.alerts
                .single()
                .text
                .startsWith("Léa : pas de nouvelles depuis 6 min, dernière position à 600 m"),
        )
        a.close()
        b.close()
    }

    @Test
    fun mapLabelsTellTheAge() {
        assertEquals("Léa · 6 min", GroupMapLayer.label(live.members[1]))
        assertEquals("Jo", GroupMapLayer.label(live.members[2]))
        assertEquals("Léa · 1 h 05", GroupMapLayer.label(live.members[1].copy(ageS = 3_900)))
        val f = GroupMapLayer.features(live.members)
        // Only the others heard: Léa.
        assertEquals(1, f.features()!!.size)
        assertEquals(false, f.features()!![0].getBooleanProperty("fresh"))
        assertEquals(true, f.features()!![0].getBooleanProperty("alert"))
    }

    @Test
    fun createFlow() {
        var created: List<String>? = null
        var scanning = false
        rule.setContent {
            Themed {
                Surface(Modifier.fillMaxSize()) {
                    GroupScreen(
                        GroupUi.NoGroup(),
                        null,
                        hasPlan = true,
                        GroupActions(create = { created = it }, wantCamera = {
                            scanning = true
                            it()
                        }),
                        scanner = { _, _ -> Text("caméra") },
                    )
                }
            }
        }
        rule.onNodeWithTag(GroupTags.CREATE).performClick()
        rule.onNodeWithTag(GroupTags.DONE).assertIsNotEnabled()
        rule.onNodeWithTag("${GroupTags.NAME}_0").performTextInput("Pierre")
        rule.onNodeWithTag("${GroupTags.NAME}_1").performTextInput("Léa")
        rule.onNodeWithTag(GroupTags.ADD).performClick()
        rule.onNodeWithTag("${GroupTags.NAME}_2").performTextInput("Jo")
        rule.onNodeWithText("Le groupe partage le plan de journée validé sur ce téléphone.").assertIsDisplayed()
        capture("group_create_light")
        rule.onNodeWithTag(GroupTags.DONE).performScrollTo().performClick()
        assertEquals(listOf("Pierre", "Léa", "Jo"), created)
        assertTrue(!scanning)
    }

    @Test
    fun joinAndChoose() {
        var chosen = -1
        rule.setContent {
            Themed {
                Surface(Modifier.fillMaxSize()) {
                    GroupScreen(GroupUi.Choosing(group()), null, hasPlan = false, GroupActions(choose = { chosen = it }))
                }
            }
        }
        rule.onNodeWithText("Qui êtes-vous ?").assertIsDisplayed()
        rule.onNodeWithTag("${GroupTags.MEMBER}_1").performClick()
        assertEquals(1, chosen)
    }

    @Test
    fun scannerOpensAndAnInvalidQrIsTold() {
        var scanned: String? = null
        rule.setContent {
            Themed {
                Surface(Modifier.fillMaxSize()) {
                    GroupScreen(
                        GroupUi.NoGroup(invalidQr = true),
                        null,
                        hasPlan = false,
                        GroupActions(scanned = { scanned = it }),
                        scanner = { onText, _ -> Text("caméra", Modifier.fillMaxSize()).also { onTextLater = onText } },
                    )
                }
            }
        }
        rule.onNodeWithText("Ce QR n'est pas un groupe skimap.").assertIsDisplayed()
        capture("group_none_light")
        rule.onNodeWithTag(GroupTags.JOIN).performClick()
        rule.onNodeWithTag(GroupTags.SCANNER).assertIsDisplayed()
        rule.runOnIdle { onTextLater!!("SKIMAP:G1:ABC") }
        assertEquals("SKIMAP:G1:ABC", scanned)
    }

    private var onTextLater: ((String) -> Unit)? = null

    @Test
    fun inGroupWithMembersDuringTheOuting() {
        val g = group()
        rule.setContent {
            Themed {
                Surface(Modifier.fillMaxSize()) {
                    GroupScreen(GroupUi.InGroup(g, 0, backend.qr(g.qrText)), live, hasPlan = false, GroupActions())
                }
            }
        }
        rule.onNodeWithContentDescription("QR du groupe ${g.shortId}, à scanner par les autres membres").assertIsDisplayed()
        capture("group_qr_light")
        rule
            .onNodeWithText(
                "à 450 m · position d'il y a 6 min · relayée 1 fois · arrêt prolongé · batterie faible",
            ).performScrollTo()
            .assertIsDisplayed()
        rule.onNodeWithText("pas encore de position").assertIsDisplayed()
        rule.onNodeWithText("Pierre (vous)").assertIsDisplayed()
    }

    @Test
    fun inGroupDarkOutsideAnOuting() {
        val g = group()
        rule.setContent {
            Themed(dark = true) {
                Surface(Modifier.fillMaxSize()) {
                    GroupScreen(GroupUi.InGroup(g, 1, backend.qr(g.qrText)), null, hasPlan = false, GroupActions())
                }
            }
        }
        rule.onNodeWithText("Léa (vous)").performScrollTo().assertIsDisplayed()
        rule
            .onNodeWithText(
                "Les positions sont partagées pendant la sortie (« Valider la journée »).",
            ).performScrollTo()
            .assertIsDisplayed()
        capture("group_qr_dark")
    }
}
