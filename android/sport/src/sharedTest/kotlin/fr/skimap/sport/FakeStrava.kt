// Test code (the detekt configuration exempts src/test and src/androidTest from MagicNumber).
@file:Suppress("MagicNumber")

package fr.skimap.sport

import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.Collections
import kotlin.concurrent.thread

/**
 * A fake skimap-server (its Strava routes, crates/server/docs/api.md) and a fake Strava API
 * (`/api/v3`: uploads, upload status, activity update) on one local port, for the JVM and
 * the instrumented tests. It checks what the real ones check (the bearer token, the code,
 * the refresh token) and records every request.
 */
class FakeStrava(
    /** `false`: the server has no Strava application (`configured: false`, 503 on the token routes). */
    @Volatile var configured: Boolean = true,
) : AutoCloseable {
    data class Request(
        val method: String,
        val path: String,
        val headers: Map<String, String>,
        val body: ByteArray,
    ) {
        val text: String get() = body.decodeToString()

        fun json(): JSONObject = JSONObject(text)
    }

    val requests: MutableList<Request> = Collections.synchronizedList(ArrayList())

    /** Tokens in force (refreshed: the old access token no longer works). */
    @Volatile var access = "access-1"

    @Volatile var refresh = "refresh-1"

    @Volatile var expiresAt = System.currentTimeMillis() / 1000 + 6 * 3600

    /** Status calls answered « still processing » before the activity id. */
    @Volatile var processing = 1

    /** Uploads answered with this error (a bad file). */
    @Volatile var uploadError: String? = null

    val revoked: MutableList<String> = Collections.synchronizedList(ArrayList())

    private val socket = ServerSocket(0, BACKLOG, InetAddress.getByName("127.0.0.1"))
    private var statusCalls = 0
    private var nextUpload = UPLOAD_ID

    val base: String get() = "http://127.0.0.1:${socket.localPort}"
    val api: String get() = "$base/api/v3"

    private val loop =
        thread(name = "fake-strava", isDaemon = true) {
            while (!socket.isClosed) {
                val s = runCatching { socket.accept() }.getOrNull() ?: break
                runCatching { serve(s) }
                runCatching { s.close() }
            }
        }

    override fun close() {
        socket.close()
        loop.join(JOIN_MS)
    }

    private fun serve(s: Socket) {
        val input = BufferedInputStream(s.getInputStream())
        val line = readLine(input) ?: return
        val (method, target) = line.split(' ').let { it[0] to it[1] }
        val headers =
            generateSequence { readLine(input)?.takeIf { it.isNotEmpty() } }
                .associate { h -> h.substringBefore(':').trim().lowercase() to h.substringAfter(':').trim() }
        val length = headers["content-length"]?.toInt() ?: 0
        val body = ByteArray(length)
        var read = 0
        var n = 0
        while (read < length && n >= 0) {
            n = input.read(body, read, length - read)
            if (n > 0) read += n
        }
        val r = Request(method, target.substringBefore('?'), headers, body)
        requests += r
        val (code, reply) = synchronized(this) { answer(r) }
        val bytes = reply.toByteArray()
        val out = s.getOutputStream()
        out.write(
            ("HTTP/1.1 $code X\r\nContent-Type: application/json\r\nContent-Length: ${bytes.size}\r\nConnection: close\r\n\r\n")
                .toByteArray(),
        )
        out.write(bytes)
        out.flush()
    }

    private fun answer(r: Request): Pair<Int, String> =
        when {
            r.path.startsWith("/v1/strava/") -> server(r)
            r.headers["authorization"] != "Bearer $access" -> UNAUTHORIZED to """{"message":"Authorization Error"}"""
            else -> api(r)
        }

    /** skimap-server's Strava routes. */
    @Suppress("ReturnCount")
    private fun server(r: Request): Pair<Int, String> {
        val p = r.path
        if (p == "/v1/strava/config" && r.method == "GET") {
            return OK to
                JSONObject()
                    .put("configured", configured)
                    .put("client_id", if (configured) CLIENT_ID else JSONObject.NULL)
                    .put("authorize_url", "$base/oauth/mobile/authorize")
                    .put("redirect_uri", "$base/v1/strava/callback")
                    .put("scope", "activity:write")
                    .toString()
        }
        if (!configured) return UNAVAILABLE to """{"error":"strava_not_configured"}"""
        return when (p) {
            "/v1/strava/token" -> {
                if (r.json().optString("code") == CODE) OK to tokens(athlete = true) else BAD to """{"message":"Bad Request"}"""
            }

            "/v1/strava/refresh" -> {
                if (r.json().optString("refresh_token") != refresh) return BAD to """{"message":"Bad Request"}"""
                access = "access-" + (access.substringAfter('-').toInt() + 1)
                refresh = "refresh-" + (refresh.substringAfter('-').toInt() + 1)
                expiresAt = System.currentTimeMillis() / 1000 + 6 * 3600
                OK to tokens(athlete = false)
            }

            "/v1/strava/revoke" -> {
                revoked += r.json().optString("token")
                OK to "{}"
            }

            else -> {
                NOT_FOUND to "{}"
            }
        }
    }

    /** The Strava API: uploads, upload status, activity update. */
    private fun api(r: Request): Pair<Int, String> {
        val p = r.path
        return when {
            p == "/api/v3/uploads" && r.method == "POST" -> {
                statusCalls = 0
                val id = nextUpload++
                CREATED to upload(id, uploadError, null)
            }

            p.startsWith("/api/v3/uploads/") && r.method == "GET" -> {
                val id = p.substringAfterLast('/').toLong()
                statusCalls++
                OK to upload(id, null, if (statusCalls > processing) ACTIVITY_ID + (id - UPLOAD_ID) else null)
            }

            p.startsWith("/api/v3/activities/") && r.method == "PUT" -> {
                OK to
                    JSONObject()
                        .put(
                            "id",
                            p.substringAfterLast('/').toLong(),
                        ).put("sport_type", r.json().optString("sport_type"))
                        .toString()
            }

            else -> {
                NOT_FOUND to "{}"
            }
        }
    }

    private fun tokens(athlete: Boolean): String {
        val o =
            JSONObject()
                .put("token_type", "Bearer")
                .put("access_token", access)
                .put("refresh_token", refresh)
                .put("expires_at", expiresAt)
        if (athlete) o.put("athlete", JSONObject().put("id", ATHLETE_ID).put("firstname", "Ada").put("lastname", "L."))
        return o.toString()
    }

    private fun upload(
        id: Long,
        error: String?,
        activity: Long?,
    ): String =
        JSONObject()
            .put("id", id)
            .put("id_str", id.toString())
            .put("status", if (activity != null) "Your activity is ready." else "Your activity is still being processed.")
            .put("error", error ?: JSONObject.NULL)
            .put("activity_id", activity ?: JSONObject.NULL)
            .toString()

    private fun readLine(input: InputStream): String? {
        val b = ByteArrayOutputStream()
        while (true) {
            val c = input.read()
            if (c < 0) return if (b.size() == 0) null else b.toString()
            if (c == '\n'.code) return b.toString().trimEnd('\r')
            b.write(c)
        }
    }

    /** Field [name] of the multipart body of [r]. */
    fun field(
        r: Request,
        name: String,
    ): String? {
        val marker = "name=\"$name\""
        val t = r.text
        val i = t.indexOf(marker)
        if (i < 0) return null
        val start = t.indexOf("\r\n\r\n", i) + 4
        val end = t.indexOf("\r\n--", start)
        return t.substring(start, end)
    }

    companion object {
        const val CLIENT_ID = "4242"
        const val CODE = "good-code"
        const val UPLOAD_ID = 9001L
        const val ACTIVITY_ID = 7_000_001L
        const val ATHLETE_ID = 77
        private const val OK = 200
        private const val CREATED = 201
        private const val BAD = 400
        private const val UNAUTHORIZED = 401
        private const val NOT_FOUND = 404
        private const val UNAVAILABLE = 503
        private const val BACKLOG = 16
        private const val JOIN_MS = 2_000L
    }
}
