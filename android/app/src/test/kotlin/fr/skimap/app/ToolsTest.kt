package fr.skimap.app

import android.hardware.SensorManager
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Surface
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.fog.BackArrow
import fr.skimap.app.fog.Calibration
import fr.skimap.app.fog.Compass
import fr.skimap.app.fog.CompassReading
import fr.skimap.app.fog.FogScreen
import fr.skimap.app.fog.FogTags
import fr.skimap.app.fog.FogUi
import fr.skimap.app.fog.distanceText
import fr.skimap.app.offpiste.OffpisteActions
import fr.skimap.app.offpiste.OffpisteScreen
import fr.skimap.app.offpiste.OffpisteTags
import fr.skimap.app.offpiste.OffpisteUi
import fr.skimap.app.offpiste.RouteOverlay
import fr.skimap.app.prep.AlertState
import fr.skimap.app.prep.HttpAnswer
import fr.skimap.app.prep.PrepActions
import fr.skimap.app.prep.PrepScreen
import fr.skimap.app.prep.PrepTags
import fr.skimap.app.prep.PrepUi
import fr.skimap.app.prep.ReturnAlert
import fr.skimap.app.prep.TripsApi
import fr.skimap.app.recap.RecapActions
import fr.skimap.app.recap.RecapScreen
import fr.skimap.app.recap.RecapTags
import fr.skimap.app.recap.RecapUi
import fr.skimap.app.rules.PlaceContent
import fr.skimap.app.rules.PlaceTags
import fr.skimap.core.FogTarget
import fr.skimap.core.InfoAge
import fr.skimap.core.Lang
import fr.skimap.core.OffpisteRouteView
import fr.skimap.core.PlaceRules
import fr.skimap.core.PrepState
import fr.skimap.core.PreparedDay
import fr.skimap.core.PreparedLoop
import fr.skimap.core.RuleLine
import fr.skimap.core.TopicLine
import fr.skimap.core.TurnDirection
import fr.skimap.core.batteryForecast
import fr.skimap.core.fogBearing
import fr.skimap.core.guidedBearing
import fr.skimap.core.isGuided
import fr.skimap.core.openGuided
import fr.skimap.core.outingDeleteAll
import fr.skimap.core.outingHistory
import fr.skimap.core.outingRecap
import fr.skimap.tracking.BackGuide
import fr.skimap.tracking.GpsMode
import fr.skimap.tracking.Guidance
import fr.skimap.tracking.Phase
import fr.skimap.tracking.Turn
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * The tools of the outing (K7, V1, A10, E2, V6, V7, S2) on the JVM: the logic through the
 * real Rust core (JNA), the screens captured (Roborazzi, `src/test/screenshots/tools_*.png`).
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class ToolsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val tmp = TemporaryFolder()

    private fun capture(name: String) = rule.onRoot().captureRoboImage("src/test/screenshots/$name.png")

    private val refuge = FogTarget("refuge", "Refuge de la Blanche", 6.0, 45.0 + 1300.0 / 111_195.0, null)

    private fun pack(fixes: List<Triple<Long, Double, Double>>): ByteArray {
        val b = ByteBuffer.allocate(fixes.size * 24).order(ByteOrder.LITTLE_ENDIAN)
        fixes.forEach { (t, lon, lat) -> b.putLong(t).putDouble(lon).putDouble(lat) }
        return b.array()
    }

    /** A walk north to [refuge], a fix every 5 s, through the real guided session. */
    private fun walk(dir: File) {
        guidedBearing(dir.path, refuge, null, 0.0, Lang.FR, "cervieres", null)
        assertTrue(isGuided(dir.path))
        val s = openGuided(dir.path)
        val fixes = (1..240).map { i -> Triple(T0 + i * 5000L, 6.0, 45.0 + i * 6.0 / 111_195.0) }
        val said = mutableListOf<String>()
        fixes.chunked(12).forEach { batch -> said += s.pushFixes(pack(batch)).spoken.map { it.text } }
        assertTrue(said.first(), said.first().startsWith("Refuge de la Blanche : 1,3 kilomètre, cap 0 degrés, nord"))
        assertTrue(said.toString(), said.last().startsWith("Arrivée : Refuge de la Blanche"))
        assertEquals("fog", s.guidance().route)
        s.writeGpx()
        s.close()
    }

    @Test
    fun bearingAndCompass() {
        val v = fogBearing(6.0, 45.0, 5.99, 44.99, 180.0, Lang.FR)
        assertEquals("sud-ouest", v.cardinal)
        assertEquals(TurnDirection.SLIGHT_RIGHT, v.turn)
        assertEquals("légèrement à droite", v.turnText)
        assertEquals(92.5, Compass.trueHeading(Math.toRadians(90.0), 2.5), 1e-9)
        assertEquals(350.0, Compass.trueHeading(Math.toRadians(-8.0), -2.0), 1e-9)
        assertEquals(Calibration.OK, Compass.calibration(SensorManager.SENSOR_STATUS_ACCURACY_HIGH))
        assertEquals(Calibration.LOW, Compass.calibration(SensorManager.SENSOR_STATUS_UNRELIABLE))
    }

    @Test
    fun guidedOutingRecapHistoryAndDelete() {
        val root = tmp.newFolder("plans")
        walk(File(root, "2026-02-15-1"))
        val h = outingHistory(root.path, Lang.FR)
        assertEquals(1, h.size)
        assertEquals("fog", h[0].kind)
        val r = outingRecap(h[0].dir, root.path, tmp.newFolder("work").path, Lang.FR)
        assertTrue(r.text, r.text.contains("Distance : 1,4 km"))
        assertTrue(r.spoken, r.spoken.startsWith("Bilan : "))
        assertTrue(File(h[0].dir, "trace.gpx").isFile)
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { RecapScreen(RecapUi(h, r), RecapActions()) } } }
        rule.onNodeWithTag(RecapTags.LISTEN).assertIsDisplayed()
        capture("tools_recap")
        assertEquals(1u, outingDeleteAll(root.path, null))
        assertTrue(outingHistory(root.path, Lang.FR).isEmpty())
    }

    /** The recap of the same outing on a phone in English (VO6): every text of the core in English. */
    @Test
    @Config(qualifiers = "en-w412dp-h915dp-hdpi")
    fun guidedOutingRecapInEnglish() {
        val root = tmp.newFolder("plans")
        val dir = File(root, "2026-02-15-1")
        guidedBearing(dir.path, refuge, null, 0.0, Lang.EN, "cervieres", null)
        val s = openGuided(dir.path)
        val fixes = (1..240).map { i -> Triple(T0 + i * 5000L, 6.0, 45.0 + i * 6.0 / 111_195.0) }
        fixes.chunked(12).forEach { batch -> s.pushFixes(pack(batch)) }
        s.close()
        val h = outingHistory(root.path, Lang.EN)
        val r = outingRecap(h[0].dir, root.path, tmp.newFolder("work").path, Lang.EN)
        assertTrue(r.text, r.text.contains("Distance: 1.4 km"))
        assertTrue(r.spoken, r.spoken.startsWith("Recap: "))
        assertTrue(h[0].`when`, h[0].`when`.contains(" at "))
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { RecapScreen(RecapUi(h, r), RecapActions()) } } }
        rule.onNodeWithTag(RecapTags.LISTEN).assertIsDisplayed()
        capture("tools_recap_en")
    }

    @Test
    fun deleteAllAsksFirst() {
        var deleted = 0
        val s =
            fr.skimap.core.OutingSummary(
                "/x",
                "day",
                "cervieres",
                "Boucle du Laus",
                "2026-02-15",
                "15/02 à 9 h 30",
                1L,
                6.2,
                5400.0,
                6600.0,
                6000.0,
            )
        rule.setContent {
            Themed {
                Surface(
                    Modifier.fillMaxSize(),
                ) { RecapScreen(RecapUi(listOf(s)), RecapActions(deleteAll = { deleted++ })) }
            }
        }
        capture("tools_outings")
        rule.onNodeWithTag(RecapTags.DELETE_ALL).performClick()
        assertEquals(0, deleted)
        rule.onNodeWithTag(RecapTags.CONFIRM).performClick()
        assertEquals(1, deleted)
    }

    @Test
    fun fogScreen() {
        val b = fogBearing(6.0, 45.0, refuge.lon, refuge.lat, 330.0, Lang.FR)
        val ui =
            FogUi(
                targets = listOf(refuge, FogTarget("parking", "Parking du Laus", 6.72, 44.85, null)),
                target = refuge,
                bearing = b,
                compass = CompassReading(330.0, Calibration.LOW),
            )
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { FogScreen(ui, hasCompass = true, onPick = {}, onVoice = {}) } } }
        rule.onNodeWithTag(FogTags.ARROW).assertIsDisplayed()
        rule.onNodeWithText("Boussole non calibrée", substring = true).assertIsDisplayed()
        rule.onNodeWithText("légèrement à droite").assertIsDisplayed()
        capture("tools_fog")
    }

    @Test
    fun offpisteScreenAndOverlay() {
        val line = (0..20).flatMap { listOf(6.72 + it * 0.002, 44.85 + it * 0.0015 + (if (it % 4 == 0) 0.0005 else 0.0)) }
        val r =
            OffpisteRouteView(
                label = "Le Laus → col d'Izoard",
                line = line,
                lengthKm = 5.75,
                ascentM = 620.0,
                descentM = 0.0,
                maxSlopeDeg = 24.0,
                timeText = "environ 5 h 25 (3 h 00 à 9 h 35) au rythme du groupe",
                timeS = 19_500.0,
                level = "moyen",
                levelReverse = "moyen",
                reasons = listOf("traverse 3,7 km de terrain « exposé » aux avalanches (modèle), en 13 passages"),
                advice = listOf("Peaux courtes (ou demi-peaux) pour 7 montées de 12° ou plus"),
                text = "…",
                inPlaces = emptyList(),
                snapM = listOf(1.0, 2.0),
                neverGuarantee = "Indication calculée sur le relief et les cartes, jamais une garantie.",
                missing = emptyList(),
                computeMs = 55u,
            )
        val ui = OffpisteUi(points = listOf(refuge), from = refuge, to = refuge, day = java.time.LocalDate.of(2026, 2, 15), route = r)
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { OffpisteScreen(ui, OffpisteActions()) } } }
        rule.onNodeWithTag(OffpisteTags.RESULT).assertIsDisplayed()
        capture("tools_offpiste")
        rule.onNode(hasScrollAction()).performScrollToNode(hasTestTag(OffpisteTags.GUIDE))
        rule.onNodeWithTag(OffpisteTags.GUIDE).assertIsDisplayed()
        val json = JSONObject(RouteOverlay.geoJson(line))
        assertEquals("LineString", json.getString("type"))
        assertEquals(21, json.getJSONArray("coordinates").length())
    }

    @Test
    fun placeRulesSheet() {
        val r =
            PlaceRules(
                "le 15/02 à 10 h 00",
                true,
                listOf(
                    TopicLine("bivouac", "Bivouac", "interdit", true),
                    TopicLine("access", "Accès", "autorisé", false),
                    TopicLine("leave_routes", "Sortie des itinéraires balisés", "interdit", true),
                    TopicLine("fire", "Feu", "interdit (réchauds compris)", true),
                ),
                listOf("APPB du Haut-Doubs", "France"),
                listOf(
                    RuleLine(
                        "Bivouac",
                        "interdit",
                        true,
                        null,
                        "Bivouac interdit dans le périmètre de l'arrêté.",
                        "toute l'année",
                        null,
                        null,
                        null,
                        "Arrêté préfectoral APPB Haut-Doubs",
                        "https://www.doubs.gouv.fr/",
                        "projet d'arrêté (texte signé non lu)",
                    ),
                ),
                "Règles locales non vérifiées : Tourbière du Cerneux (arrêté de protection de biotope) : texte non lu ; " +
                    "arrêtés municipaux et préfectoraux : non recensés.",
                "Règles relevées dans les textes cités ; le texte officiel et la signalisation font foi.",
            )
        var car = 0
        rule.setContent { Themed { Surface { PlaceContent(r, onFog = {}, onOffpiste = {}, onCar = { car++ }) } } }
        rule.onNodeWithText("Bivouac : interdit").assertIsDisplayed()
        rule.onNodeWithText("Règles locales non vérifiées", substring = true).assertExists()
        capture("tools_place_rules")
        // « La voiture est ici » (K4): one touch, optional.
        rule.onNodeWithTag(PlaceTags.CAR).assertIsDisplayed().performClick()
        assertEquals(1, car)
    }

    /** The rules sheet on a phone in English (VO6): the texts as the core writes them in English. */
    @Test
    @Config(qualifiers = "en-w412dp-h915dp-hdpi")
    fun placeRulesSheetInEnglish() {
        val r =
            PlaceRules(
                "on 15/02 at 10:00",
                true,
                listOf(
                    TopicLine("bivouac", "Bivouac", "forbidden", true),
                    TopicLine("access", "Access", "allowed", false),
                    TopicLine("leave_routes", "Leaving the marked routes", "forbidden", true),
                    TopicLine("fire", "Fire", "forbidden (stoves included)", true),
                ),
                listOf("APPB du Haut-Doubs", "France"),
                listOf(
                    RuleLine(
                        "Bivouac",
                        "forbidden",
                        true,
                        null,
                        "Bivouac interdit dans le périmètre de l'arrêté.",
                        "toute l'année",
                        null,
                        null,
                        null,
                        "Arrêté préfectoral APPB Haut-Doubs",
                        "https://www.doubs.gouv.fr/",
                        "draft order (signed text not read)",
                    ),
                ),
                "Local rules not verified: Tourbière du Cerneux (biotope protection order): text not read; " +
                    "municipal and prefectoral orders: not listed.",
                "Rules read in the texts cited, at this date and time; the official text and the signs on the ground prevail.",
            )
        rule.setContent { Themed { Surface { PlaceContent(r, onFog = {}, onOffpiste = {}, onCar = {}) } } }
        rule.onNodeWithText("Bivouac: forbidden").assertIsDisplayed()
        rule.onNodeWithText("Local rules not verified", substring = true).assertExists()
        capture("tools_place_rules_en")
    }

    /** « Ramène-moi » going back to the car: 1 km south of the skier, the next point of the way 60 m west. */
    private fun backGuidance(offWay: Boolean): Pair<BackGuide, Guidance> {
        val back =
            BackGuide(
                target = "car",
                targetName = "Parking du Bourget",
                way = "trace_pistes",
                hereLon = 6.7269,
                hereLat = 44.8552,
                guideLon = 6.7269 - 60.0 / 78_870.0,
                guideLat = 44.8552,
                guideBearingDeg = 270.0,
                guideDistanceM = 60.0,
                offWay = offWay,
            )
        val g =
            Guidance(
                Phase.MOVING,
                "back",
                "Boucle du Laus",
                2_340.0,
                2_340.0,
                0.0,
                Turn.LEFT,
                300.0,
                null,
                2_700.0,
                "45 min",
                "15 h 20",
                0,
                null,
                null,
                55.0,
                GpsMode.INTERACTION,
                null,
                null,
                back = back,
            )
        return back to g
    }

    @Test
    fun backArrowPointsToTheNextPointOfTheWayWithTheCompass() {
        val (back, g) = backGuidance(offWay = true)
        // The phone points north: the next point (west) is to the left.
        val b = fogBearing(back.hereLon, back.hereLat, back.guideLon, back.guideLat, 0.0, Lang.FR)
        assertEquals(-90.0, b.relativeDeg!!, 1.0)
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { Box(Modifier.padding(16.dp)) { BackArrow(back, g, b) } } } }
        rule.onNodeWithTag(FogTags.BACK_ARROW).assertIsDisplayed()
        rule.onNodeWithText("Vers la voiture").assertIsDisplayed()
        rule.onNodeWithText("Reste 2,3 km · 45 min").assertIsDisplayed()
        rule.onNodeWithText("Prochain point du retour : 60 mètres").assertIsDisplayed()
        rule.onNodeWithText("à gauche").assertIsDisplayed()
        capture("tools_back_arrow")
    }

    @Test
    fun backArrowWithoutCompassSaysTheBearing() {
        val (back, g) = backGuidance(offWay = true)
        val b = fogBearing(back.hereLon, back.hereLat, back.guideLon, back.guideLat, null, Lang.FR)
        val dest = back.copy(target = "destination", targetName = "Gîte du Laus")
        rule.setContent { Themed { Surface(Modifier.fillMaxSize()) { Box(Modifier.padding(16.dp)) { BackArrow(dest, g, b) } } } }
        rule.onNodeWithText("Vers Gîte du Laus").assertIsDisplayed()
        rule.onNodeWithText("cap 270°, ouest").assertIsDisplayed()
        rule.onNodeWithText("Sans boussole", substring = true).assertIsDisplayed()
        assertEquals("850 m", distanceText(853.0, java.util.Locale.FRANCE))
        assertEquals("2,3 km", distanceText(2_340.0, java.util.Locale.FRANCE))
    }

    @Test
    fun prepScreenShowsDatesNeverStale() {
        val day =
            PreparedDay(
                "cervieres",
                "2026-11-16",
                "lundi 16/11",
                "Le Laus",
                listOf("Neige transformée"),
                "Meilleur départ vers 11 h 30",
                null,
                listOf(PreparedLoop("Route du Col d'Izoard – Le Gibera", 6.24, "environ 2 h 10 (1 h 10 à 3 h 55) au rythme du groupe")),
                "/x",
                "14 h 40",
                emptyList(),
                listOf("clip"),
                "Famille",
                0L,
                "mis à jour le 15/11 à 14 h 05",
                "Jamais « sans danger ».",
            )
        val infos = listOf(InfoAge("Bulletin d'avalanche (BERA)", "cervieres", "mis à jour le 15/11 à 10 h 05", 1L))
        val battery = batteryForecast(T0, 18.0, -12.0, 0.0, T0 + 6 * 3_600_000L, 20.0, 0.5, false, 1.0, 9.0, Lang.FR)
        assertFalse(battery.enough)
        rule.setContent {
            Themed {
                Surface(
                    Modifier.fillMaxSize(),
                ) { PrepScreen(PrepUi(PrepState(listOf(day), infos), battery = battery), PrepActions()) }
            }
        }
        rule.onNodeWithText("mis à jour le 15/11 à 14 h 05").assertIsDisplayed()
        rule.onNodeWithText("Batterie insuffisante", substring = true).assertIsDisplayed()
        capture("tools_prep")
    }

    @Test
    fun settingsShortSoundsFoldedThenSet() {
        var ui by mutableStateOf(PrepUi(PrepState(emptyList(), emptyList())))
        var listened = 0
        rule.setContent {
            Themed {
                Surface(Modifier.fillMaxSize()) {
                    PrepScreen(ui, PrepActions(earcons = { ui = ui.copy(earcons = it) }, listenEarcons = { listened++ }))
                }
            }
        }
        // Folded: only its title.
        rule.onNodeWithTag(PrepTags.EARCON_SWITCH).assertDoesNotExist()
        rule.onNodeWithText("Sons courts").performClick()
        rule.onNodeWithTag(PrepTags.EARCON_VOLUME).assertIsDisplayed()
        rule.onNodeWithText("Volume : 50 %").assertIsDisplayed()
        capture("tools_settings_earcons")
        rule.onNodeWithTag(PrepTags.EARCON_LISTEN).performClick()
        assertEquals(1, listened)
        rule.onNodeWithTag(PrepTags.EARCON_SWITCH).performClick()
        assertFalse(ui.earcons.enabled)
        rule.onNodeWithTag(PrepTags.EARCON_VOLUME).assertDoesNotExist()
        rule.onNodeWithTag(PrepTags.EARCON_SWITCH).performClick()
        assertTrue(ui.earcons.enabled)
        assertEquals(0.5f, ui.earcons.volume, 0f)
    }

    @Test
    fun serverAlertDepositCheckinAnd503() {
        val dir = tmp.newFolder("plan")
        val body = """{"contacts":["a@b.fr"]}"""
        val trip = File(dir, ReturnAlert.TRIP_FILE)

        fun pending() = trip.writeText(JSONObject().put("state", "PENDING").put("body", body).toString())
        // 503: the service is not configured on the server.
        pending()
        assertFalse(ReturnAlert.run({ _, _, _ -> HttpAnswer(503, """{"error":"unavailable"}""") }, dir.path, ReturnAlert.DEPOSIT))
        assertEquals(AlertState.UNAVAILABLE, ReturnAlert.state(dir.path).first)
        // Network error: retried.
        pending()
        assertTrue(ReturnAlert.run({ _, _, _ -> throw IOException("no network") }, dir.path, ReturnAlert.DEPOSIT))
        assertEquals(AlertState.PENDING, ReturnAlert.state(dir.path).first)
        // Deposit, then check-in with the token.
        val calls = mutableListOf<Pair<String, String?>>()
        val api =
            TripsApi { path, _, token ->
                calls += path to token
                if (path == "/v1/trips") HttpAnswer(201, """{"id":"t1","token":"abc","trip":{}}""") else HttpAnswer(200, "{}")
            }
        assertFalse(ReturnAlert.run(api, dir.path, ReturnAlert.DEPOSIT))
        assertEquals(AlertState.ACTIVE, ReturnAlert.state(dir.path).first)
        assertFalse(ReturnAlert.run(api, dir.path, ReturnAlert.CHECKIN))
        assertEquals(AlertState.RETURNED, ReturnAlert.state(dir.path).first)
        assertEquals(listOf("/v1/trips" to null, "/v1/trips/t1/checkin" to "abc"), calls)
    }

    @Test
    fun fogHintOpensTheBearing() {
        var opened = 0
        rule.setContent {
            Themed {
                Surface {
                    fr.skimap.app.fog
                        .FogHint(fr.skimap.app.fog.FogReason.FOG, "Le Laus") { opened++ }
                }
            }
        }
        rule.onNodeWithText("Brouillard prévu maintenant", substring = true).performClick()
        assertEquals(1, opened)
        capture("tools_fog_hint")
    }

    private companion object {
        const val T0 = 1_771_146_000_000L
    }
}
