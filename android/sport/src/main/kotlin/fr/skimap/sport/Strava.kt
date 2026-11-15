package fr.skimap.sport

import androidx.core.net.toUri
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection

/** What skimap-server says of its Strava application (`GET /v1/strava/config`). */
data class StravaConfig(
    val configured: Boolean,
    val clientId: String?,
    val authorizeUrl: String,
    val redirectUri: String,
    val scope: String,
)

/** The athlete's tokens (Strava's answer, relayed by the server). */
data class StravaTokens(
    val access: String,
    val refresh: String,
    /** Epoch seconds. */
    val expiresAt: Long,
    val athlete: String?,
) {
    /** Expires within [marginS] seconds of [nowS]. */
    fun expiring(
        nowS: Long,
        marginS: Long = EXPIRY_MARGIN_S,
    ): Boolean = expiresAt - marginS <= nowS

    private companion object {
        const val EXPIRY_MARGIN_S = 120L
    }
}

/** An upload being processed by Strava (`GET /uploads/{id}`). */
data class UploadStatus(
    val id: Long,
    val activityId: Long?,
    val error: String?,
)

/**
 * Strava refused or could not be reached. [retry]: the same call may succeed later (no
 * network, rate limit, server error); otherwise it will not (bad token, bad file).
 */
class StravaException(
    message: String,
    val code: Int,
    val retry: Boolean,
) : IOException(message) {
    /** The athlete's authorisation is gone (revoked on strava.com, or never given). */
    val unauthorized: Boolean get() = code == HttpURLConnection.HTTP_UNAUTHORIZED
}

/**
 * Strava for the app. The OAuth exchange goes through skimap-server ([server], S1): Strava
 * has no PKCE and wants its client secret, which stays on the server
 * (crates/server/docs/strava.md). The upload goes straight to the Strava API ([api]) with
 * the athlete's token. Blocking calls.
 */
@Suppress("TooManyFunctions") // one per call of the API, and their parsing
class StravaClient(
    private val server: String,
    private val api: String = DEFAULT_API,
    private val http: Http = Http(),
) {
    fun config(): StravaConfig {
        val r = http.get("$server/v1/strava/config")
        check(r, "config")
        val o = JSONObject(r.body)
        return StravaConfig(
            configured = o.optBoolean("configured"),
            clientId = o.optString("client_id").takeIf { !o.isNull("client_id") && it.isNotEmpty() },
            authorizeUrl = o.optString("authorize_url"),
            redirectUri = o.optString("redirect_uri"),
            scope = o.optString("scope"),
        )
    }

    /** The page where the athlete allows skimap to upload ([state]: checked at the callback). */
    fun authorizeUrl(
        c: StravaConfig,
        state: String,
    ): String =
        c.authorizeUrl
            .toUri()
            .buildUpon()
            .appendQueryParameter("client_id", c.clientId)
            .appendQueryParameter("redirect_uri", c.redirectUri)
            .appendQueryParameter("response_type", "code")
            .appendQueryParameter("approval_prompt", "auto")
            .appendQueryParameter("scope", c.scope)
            .appendQueryParameter("state", state)
            .build()
            .toString()

    /** The code of the callback for the tokens (`POST /v1/strava/token`). */
    fun exchange(code: String): StravaTokens = tokens("token", JSONObject().put("code", code), null)

    /** New tokens (`POST /v1/strava/refresh`); the athlete's name is kept from [old]. */
    fun refresh(old: StravaTokens): StravaTokens = tokens("refresh", JSONObject().put("refresh_token", old.refresh), old.athlete)

    /** Disconnects skimap from the athlete's account (`POST /v1/strava/revoke`). */
    fun revoke(t: StravaTokens) {
        val r = http.json("POST", "$server/v1/strava/revoke", JSONObject().put("token", t.access).toString())
        check(r, "revoke")
    }

    /** Uploads the GPX (`POST /uploads`): the upload's id, processed by Strava afterwards. */
    fun upload(
        access: String,
        outing: Outing,
        file: File,
    ): UploadStatus {
        val parts =
            listOf(
                Part.FilePart("file", file, "application/gpx+xml"),
                Part.Field("data_type", "gpx"),
                Part.Field("name", outing.name),
                Part.Field("description", outing.description),
                Part.Field("external_id", "skimap-${outing.id}.gpx"),
            )
        val r = http.multipart("$api/uploads", parts, access)
        check(r, "upload")
        return status(JSONObject(r.body))
    }

    fun uploadStatus(
        access: String,
        id: Long,
    ): UploadStatus {
        val r = http.get("$api/uploads/$id", access)
        check(r, "upload status")
        return status(JSONObject(r.body))
    }

    /** Sets the sport type and the description of the activity (`PUT /activities/{id}`). */
    fun updateActivity(
        access: String,
        activityId: Long,
        outing: Outing,
    ) {
        val body =
            JSONObject()
                .put("sport_type", outing.sport.strava)
                .put("name", outing.name)
                .put("description", outing.description)
        val r = http.json("PUT", "$api/activities/$activityId", body.toString(), access)
        check(r, "activity")
    }

    private fun tokens(
        path: String,
        body: JSONObject,
        athlete: String?,
    ): StravaTokens {
        val r = http.json("POST", "$server/v1/strava/$path", body.toString())
        check(r, path)
        val o = JSONObject(r.body)
        val a = o.optJSONObject("athlete")
        val name =
            a?.let { listOf(it.optString("firstname"), it.optString("lastname")).filter(String::isNotBlank).joinToString(" ") }
        return StravaTokens(
            access = o.getString("access_token"),
            refresh = o.getString("refresh_token"),
            expiresAt = o.getLong("expires_at"),
            athlete = name?.takeIf { it.isNotBlank() } ?: athlete,
        )
    }

    private fun status(o: JSONObject): UploadStatus {
        val error = o.optString("error").takeIf { !o.isNull("error") && it.isNotBlank() }
        val activity = if (o.isNull("activity_id")) null else o.optLong("activity_id")
        // A trace sent twice: Strava names the activity it already has.
        val duplicate =
            error?.let {
                DUPLICATE
                    .find(it)
                    ?.groupValues
                    ?.get(1)
                    ?.toLong()
            }
        return UploadStatus(o.optLong("id"), activity ?: duplicate, if (duplicate != null) null else error)
    }

    private fun check(
        r: HttpReply,
        what: String,
    ) {
        if (r.code in OK) return
        val retry = r.code == TOO_MANY || r.code >= HttpURLConnection.HTTP_INTERNAL_ERROR
        throw StravaException("Strava $what: HTTP ${r.code} ${r.body.take(MAX_ERROR)}", r.code, retry)
    }

    companion object {
        const val DEFAULT_API = "https://www.strava.com/api/v3"

        /** The redirection of the server's callback to the app (crates/server/docs/api.md). */
        const val CALLBACK = "skimap://strava/callback"

        private val OK = 200..299
        private const val TOO_MANY = 429
        private const val MAX_ERROR = 200
        private val DUPLICATE = Regex("""duplicate of .*?activities/(\d+)""")
    }
}
