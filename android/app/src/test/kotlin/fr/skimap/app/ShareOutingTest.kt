package fr.skimap.app

import android.content.Context
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import fr.skimap.app.recap.OutingFiles
import fr.skimap.app.recap.RecapActions
import fr.skimap.app.recap.RecapScreen
import fr.skimap.app.recap.RecapUi
import fr.skimap.core.FogTarget
import fr.skimap.core.Lang
import fr.skimap.core.guidedBearing
import fr.skimap.core.openGuided
import fr.skimap.core.outingHistory
import fr.skimap.core.outingRecap
import fr.skimap.sport.HeartSample
import fr.skimap.sport.OutingShare
import fr.skimap.sport.ShareFormat
import fr.skimap.sport.SportTags
import fr.skimap.sport.SportType
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.time.Instant
import javax.xml.XMLConstants
import javax.xml.parsers.DocumentBuilderFactory
import javax.xml.transform.stream.StreamSource
import javax.xml.validation.SchemaFactory

/**
 * The share of a finished outing (#63) through the real Rust core: the files exported
 * from a guided outing recorded by the core, the GPX valid against the GPX 1.1 schema
 * (and Garmin's TrackPointExtension for the heart rate), the FIT file with its header,
 * and « Partager » on the recap (`src/test/screenshots/tools_recap_share.png`).
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "fr-w412dp-h915dp-hdpi")
class ShareOutingTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val tmp = TemporaryFolder()

    private val context: Context = ApplicationProvider.getApplicationContext()

    private fun pack(fixes: List<Triple<Long, Double, Double>>): ByteArray {
        val b = ByteBuffer.allocate(fixes.size * 24).order(ByteOrder.LITTLE_ENDIAN)
        fixes.forEach { (t, lon, lat) -> b.putLong(t).putDouble(lon).putDouble(lat) }
        return b.array()
    }

    /** A guided outing recorded by the core: 240 fixes, one every 5 s, 6 m apart. */
    private fun recorded(dir: File): List<Triple<Long, Double, Double>> {
        val target = FogTarget("refuge", "Refuge & lac", 6.0, 45.0 + 1300.0 / 111_195.0, null)
        guidedBearing(dir.path, target, null, 0.0, Lang.FR, null, null)
        val s = openGuided(dir.path)
        val fixes = (1..240).map { i -> Triple(T0 + i * 5000L, 6.0, 45.0 + i * 6.0 / 111_195.0) }
        fixes.chunked(12).forEach { s.pushFixes(pack(it)) }
        s.close()
        return fixes
    }

    @Test
    fun exportedFilesAreValidAndMatchTheTrace() {
        val dir = File(tmp.newFolder("plans"), "2026-02-15-1")
        val fixes = recorded(dir)
        val heart = (0 until 130).map { HeartSample(T0 + it * 10_000L, 125 + it % 25) }
        val o = OutingFiles.export(context, dir.path, heart, "Bilan")!!
        assertEquals("2026-02-15-1", o.id)
        assertEquals("Cap vers Refuge & lac", o.title)
        assertEquals(SportType.NORDIC_SKI, o.sport)
        assertEquals(fixes.size, o.points.size)
        assertEquals(fixes.first().first, o.startMs)
        assertEquals(fixes.last().first, o.endMs)
        assertEquals(239 * 6.0, o.distanceM, 1.0)
        assertTrue(o.points.all { it.heartBpm != null })
        assertEquals(OutingShare.dir(context), o.gpx.parentFile)
        assertEquals(OutingShare.dir(context), o.fit.parentFile)

        // GPX 1.1, valid against the schema of GPX and of the heart rate extension.
        val xsd = javaClass.getResource("/gpx/gpx.xsd")!!
        val tpx = javaClass.getResource("/gpx/TrackPointExtensionv1.xsd")!!
        val schema =
            SchemaFactory
                .newInstance(XMLConstants.W3C_XML_SCHEMA_NS_URI)
                .newSchema(arrayOf(StreamSource(xsd.openStream(), xsd.toString()), StreamSource(tpx.openStream(), tpx.toString())))
        schema.newValidator().validate(StreamSource(o.gpx))
        // The same points and times, read back by the platform's parser.
        val doc =
            DocumentBuilderFactory
                .newInstance()
                .apply { isNamespaceAware = true }
                .newDocumentBuilder()
                .parse(o.gpx)
        val pts = doc.getElementsByTagNameNS(GPX, "trkpt")
        assertEquals(o.points.size, pts.length)
        for (i in 0 until pts.length) {
            val e = pts.item(i) as org.w3c.dom.Element
            assertEquals(o.points[i].lat, e.getAttribute("lat").toDouble(), 1e-7)
            assertEquals(o.points[i].lon, e.getAttribute("lon").toDouble(), 1e-7)
            val t = e.getElementsByTagNameNS(GPX, "time").item(0).textContent
            assertEquals(o.points[i].timeMs, Instant.parse(t).toEpochMilli())
            val hr = e.getElementsByTagNameNS(TPX, "hr").item(0).textContent
            assertEquals(o.points[i].heartBpm, hr.toInt())
        }
        assertEquals("cross_country_skiing", doc.getElementsByTagNameNS(GPX, "type").item(0).textContent)

        // FIT: header, « .FIT », data size, and the file size it announces.
        val fit = o.fit.readBytes()
        assertEquals(14, fit[0].toInt())
        assertEquals(".FIT", String(fit, 8, 4))
        val dataSize = ByteBuffer.wrap(fit, 4, 4).order(ByteOrder.LITTLE_ENDIAN).int
        assertEquals(14 + dataSize + 2, fit.size)

        // Exported again (another share): only the files of this share are left.
        OutingFiles.export(context, dir.path, emptyList())!!
        assertEquals(setOf(o.gpx.name, o.fit.name), OutingShare.dir(context).list()!!.toSet())
    }

    @Test
    fun recapOffersTheShare() {
        val root = tmp.newFolder("plans")
        recorded(File(root, "2026-02-15-1"))
        val h = outingHistory(root.path, Lang.FR)
        val r = outingRecap(h[0].dir, root.path, tmp.newFolder("work").path, Lang.FR)
        val shared = ArrayList<ShareFormat>()
        rule.setContent {
            Themed {
                Surface(Modifier.fillMaxSize()) {
                    RecapScreen(RecapUi(h, r), RecapActions(share = { x, f -> if (x == r) shared += f }))
                }
            }
        }
        rule.onNode(hasScrollAction()).performScrollToNode(hasTestTag(SportTags.SHARE_GPX))
        rule.onRoot().captureRoboImage("src/test/screenshots/tools_recap_share.png")
        rule.onNodeWithTag(SportTags.SHARE_FIT).performClick()
        rule.onNodeWithTag(SportTags.SHARE_GPX).performClick()
        assertEquals(listOf(ShareFormat.FIT, ShareFormat.GPX), shared)
    }

    private companion object {
        /** 2026-02-15 09:00 UTC. */
        const val T0 = 1_771_146_000_000L
        const val GPX = "http://www.topografix.com/GPX/1/1"
        const val TPX = "http://www.garmin.com/xmlschemas/TrackPointExtension/v1"
    }
}
