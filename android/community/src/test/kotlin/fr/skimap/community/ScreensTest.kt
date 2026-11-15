package fr.skimap.community

import android.content.Context
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.core.app.ApplicationProvider
import com.github.takahirom.roborazzi.captureRoboImage
import kotlinx.coroutines.Dispatchers
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/** A core that answers from memory. */
class FakeCore : CommunityCore {
    override fun importStrava(
        zip: File,
        outRoot: File,
    ) = ImportSummary(3, 0, 1, 2, 3, 3, emptyList())

    override fun outings(roots: List<File>) =
        listOf(
            Outing("strava-103", "strava", "Risoux", 1_768_035_600_000, 21.4, 132.0, 310.0),
            Outing("2026-01-05-1", "skimap", "Boucle des Lacs", 1_767_600_000_000, 12.9, 95.0, 412.0),
        )

    override fun challenges(
        roots: List<File>,
        routesJson: String,
        domains: List<AreaContext>,
        nowMs: Long,
    ) = listOf(
        Challenge(
            "gtj-srn",
            ChallengeKind.ROUTE,
            "GTJ à ski de randonnée nordique",
            43.9,
            99.8,
            0.44,
            4,
            null,
            "JURA TOURISME (Licence Ouverte 2.0)",
        ),
        Challenge(
            "domain-chapelle-des-bois",
            ChallengeKind.DOMAIN,
            "Chapelle-des-Bois",
            60.7,
            311.5,
            0.19,
            6,
            null,
            "© OpenStreetMap (ODbL)",
        ),
        Challenge("km-250", ChallengeKind.DISTANCE, "250", 212.0, 250.0, 0.85, 14, null, ""),
    )

    override fun osmNote(
        correction: Correction,
        pisteName: String?,
        osmWayId: Long?,
        comment: String,
        lon: Double,
        lat: Double,
        french: Boolean,
    ) = OsmNote(lat, lon, "Le nom de cette piste est faux.\n\n$comment\n\n(Signalé sur le terrain avec l'application skimap) #skimap", "")

    override fun area(siteId: String?) = AreaContext("cervieres", "Cervières", doubleArrayOf(6.66, 44.77, 6.86, 44.92), null, null)

    override fun outingRoots() = emptyList<File>()
}

@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class ScreensTest {
    @get:Rule
    val rule = createComposeRule()

    private val now = 1_768_040_000_000L

    private fun capture(name: String) = rule.onRoot().captureRoboImage("src/test/screenshots/$name.png")

    @Composable
    private fun Themed(
        dark: Boolean = false,
        content: @Composable () -> Unit,
    ) = MaterialTheme(colorScheme = if (dark) darkColorScheme() else lightColorScheme()) { Surface { content() } }

    private fun around() =
        listOf(
            ServerReport("a", ReportKind.ICE, 6.77, 44.85, now - 3 * 3_600_000, now + 21 * 3_600_000, 1, 0, false),
            ServerReport("b", ReportKind.TRACK, 6.78, 44.86, now - 20 * 60_000, now + 47 * 3_600_000, 0, 0, false),
            ServerReport("c", ReportKind.OBSTACLE, 6.79, 44.84, now - 2 * 86_400_000, now + 5 * 86_400_000, 2, 1, true),
        )

    @Test
    fun reportScreenOneGestureThenPhotoAndVotes() {
        val tapped = mutableListOf<ReportKind>()
        val votes = mutableListOf<Pair<String, Boolean>>()
        val state =
            ReportState(
                position = Fix(6.77, 44.85, now - 30_000),
                locating = false,
                last = QueuedReport("l", ReportKind.ICE, 6.77, 44.85, now),
                pending = 2,
                around = around(),
                aroundLoaded = true,
                mine = setOf("b"),
            )
        rule.setContent { Themed { ReportScreen(state, now, { tapped += it }, {}, {}, { id, c -> votes += id to c }) } }
        capture("report_fr")
        rule.onNodeWithTag(CommunityTags.KIND + "wind_blown").performClick()
        assertEquals(listOf(ReportKind.WIND_BLOWN), tapped)
        rule.onNodeWithText("2 signalements attendent le réseau.").assertIsDisplayed()
        rule.onNodeWithTag(CommunityTags.CONFIRM + "a").performScrollTo().performClick()
        rule.onNodeWithTag(CommunityTags.DENY + "c").performScrollTo().performClick()
        assertEquals(listOf("a" to true, "c" to false), votes)
        rule.onNodeWithText("Glace · il y a 3 h", substring = true).performScrollTo().assertIsDisplayed()
    }

    @Test
    @Config(qualifiers = "en-w412dp-h915dp-night-hdpi")
    fun reportScreenOfflineDarkEnglish() {
        val state = ReportState(position = null, locating = false, pending = 1, aroundLoaded = false)
        rule.setContent { Themed(dark = true) { ReportScreen(state, now, {}, {}, {}, { _, _ -> }) } }
        capture("report_offline_en_dark")
        rule.onNodeWithText("No network: the reports around are not shown.").assertIsDisplayed()
    }

    @Test
    fun outingsScreen() {
        val core = FakeCore()
        val state =
            OutingsState(
                outings = core.outings(emptyList()),
                challenges = core.challenges(emptyList(), "", emptyList(), now),
                loading = false,
                imported = core.importStrava(File("x"), File("y")),
            )
        var imports = 0
        rule.setContent { Themed { OutingsScreen(state, onImport = { imports++ }) } }
        capture("outings_fr")
        rule.onNodeWithTag(CommunityTags.IMPORT).performScrollTo().performClick()
        assertEquals(1, imports)
    }

    @Test
    fun osmNoteScreenWithTheViewModel() {
        val context: Context = ApplicationProvider.getApplicationContext()
        val sent = mutableListOf<OsmNote>()
        val vm = OsmViewModel(FakeCore(), OsmNotes(OsmNotes.SANDBOX), "master.apis.dev.openstreetmap.org", true, Dispatchers.Unconfined)
        vm.setPosition(Fix(6.77, 44.85, now))
        vm.setComment("Panneau : Les Fonts")
        rule.setContent {
            val s by vm.state.collectAsState()
            Themed { OsmNoteScreen(s, vm::setCorrection, vm::setComment, { s.note?.let { sent += it } }) }
        }
        capture("osm_note_fr")
        rule
            .onNodeWithText(
                "Publique sur master.apis.dev.openstreetmap.org, anonyme, à votre position.",
            ).performScrollTo()
            .assertIsDisplayed()
        rule.onNodeWithTag(CommunityTags.SEND_OSM).performScrollTo().performClick()
        assertEquals(1, sent.size)
        assertEquals(true, sent[0].text.contains("Panneau : Les Fonts"))
    }

    @Test
    fun theGestureQueuesAtOnceWithTheTimeOfTheTap() {
        val context: Context = ApplicationProvider.getApplicationContext()
        val outbox = ReportOutbox(File(context.cacheDir, "gesture").also { it.deleteRecursively() })
        var enqueued = 0
        val reports =
            ReportsBackend(
                outbox = outbox,
                api = CommunityApi("http://127.0.0.1:9"),
                device = { "d".repeat(32) },
                mine = MyReports { context.getSharedPreferences("m2", Context.MODE_PRIVATE) },
                enqueue = { enqueued++ },
            )
        val deps = CommunityDeps(FakeCore(), reports, { "{}" }, now = { now })
        val vm = CommunityViewModel(deps, null, Dispatchers.Unconfined)
        vm.report(ReportKind.ICE)
        assertEquals(0, outbox.size())
        vm.setPosition(Fix(6.77, 44.85, now - 5_000))
        vm.report(ReportKind.ICE)
        rule.waitUntil { outbox.size() == 1 }
        val q = outbox.pending().single()
        assertEquals(now, q.seenAtMs)
        assertEquals(1, enqueued)
        vm.attachPhoto(byteArrayOf(1))
        rule.waitUntil { outbox.pending().single().photo != null }
        vm.cancelLast()
        rule.waitUntil { outbox.size() == 0 }
    }
}
