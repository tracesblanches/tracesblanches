package fr.skimap.sport

import android.net.Uri
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import java.io.File
import java.io.IOException

/** Every call of [StravaClient] against the fake skimap-server and the fake Strava API. */
@RunWith(AndroidJUnit4::class)
class StravaClientTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val fake = FakeStrava()
    private val client = StravaClient(fake.base, fake.api)

    @After
    fun close() = fake.close()

    @Test
    fun configAndAuthorizeUrl() {
        val c = client.config()
        assertTrue(c.configured)
        assertEquals(FakeStrava.CLIENT_ID, c.clientId)
        val u = Uri.parse(client.authorizeUrl(c, "abc"))
        assertEquals("/oauth/mobile/authorize", u.path)
        assertEquals(FakeStrava.CLIENT_ID, u.getQueryParameter("client_id"))
        assertEquals("${fake.base}/v1/strava/callback", u.getQueryParameter("redirect_uri"))
        assertEquals("code", u.getQueryParameter("response_type"))
        assertEquals("auto", u.getQueryParameter("approval_prompt"))
        assertEquals("activity:write", u.getQueryParameter("scope"))
        assertEquals("abc", u.getQueryParameter("state"))
    }

    @Test
    fun notConfiguredServer() {
        // What https://skimap.warnier.net answers today (no Strava application yet).
        fake.configured = false
        val c = client.config()
        assertFalse(c.configured)
        assertNull(c.clientId)
        val e = assertThrows(StravaException::class.java) { client.exchange(FakeStrava.CODE) }
        assertEquals(503, e.code)
    }

    @Test
    fun exchangeRefreshRevoke() {
        val t = client.exchange(FakeStrava.CODE)
        assertEquals("access-1", t.access)
        assertEquals("refresh-1", t.refresh)
        assertEquals("Ada L.", t.athlete)
        val body = fake.requests.last { it.path == "/v1/strava/token" }.json()
        assertEquals(FakeStrava.CODE, body.getString("code"))
        assertFalse("no secret in the app", body.has("client_secret"))

        val e = assertThrows(StravaException::class.java) { client.exchange("bad") }
        assertEquals(400, e.code)
        assertFalse(e.retry)

        val t2 = client.refresh(t)
        assertEquals("access-2", t2.access)
        assertEquals("refresh-2", t2.refresh)
        assertEquals("the athlete's name is kept", "Ada L.", t2.athlete)

        client.revoke(t2)
        assertEquals(listOf("access-2"), fake.revoked)
    }

    @Test
    fun uploadThenSportTypeAndDescription() {
        val o = SportFixtures.outing(tmp.root, SportType.BACKCOUNTRY_SKI)
        val up = client.upload("access-1", o, File(o.gpx))
        assertEquals(FakeStrava.UPLOAD_ID, up.id)
        assertNull(up.activityId)
        val r = fake.requests.last { it.path == "/api/v3/uploads" }
        assertTrue(r.headers["content-type"]!!.startsWith("multipart/form-data; boundary="))
        assertEquals("gpx", fake.field(r, "data_type"))
        assertEquals("Boucle du Chaumoz", fake.field(r, "name"))
        assertEquals(o.description, fake.field(r, "description"))
        assertEquals("skimap-${o.id}.gpx", fake.field(r, "external_id"))
        assertEquals(File(o.gpx).readText(), fake.field(r, "file"))
        assertTrue(r.text.contains("filename=\"trace.gpx\""))

        assertNull(client.uploadStatus("access-1", up.id).activityId)
        val done = client.uploadStatus("access-1", up.id)
        assertEquals(FakeStrava.ACTIVITY_ID, done.activityId)

        client.updateActivity("access-1", done.activityId!!, o)
        val put = fake.requests.last { it.method == "PUT" }
        assertEquals("/api/v3/activities/${FakeStrava.ACTIVITY_ID}", put.path)
        assertEquals("BackcountrySki", put.json().getString("sport_type"))
        assertEquals(o.description, put.json().getString("description"))
    }

    @Test
    fun badTokenAndDuplicate() {
        val o = SportFixtures.outing(tmp.root)
        val e = assertThrows(StravaException::class.java) { client.upload("stale", o, File(o.gpx)) }
        assertTrue(e.unauthorized)
        fake.uploadError = "trace.gpx duplicate of <a href='/activities/123456'>Boucle</a>"
        val up = client.upload("access-1", o, File(o.gpx))
        assertEquals("a trace sent twice is the activity Strava has", 123456L, up.activityId)
        assertNull(up.error)
        fake.uploadError = "Improperly formatted data."
        assertEquals("Improperly formatted data.", client.upload("access-1", o, File(o.gpx)).error)
    }

    @Test
    fun unreachable() {
        fake.close()
        assertThrows(IOException::class.java) { client.config() }
    }
}
