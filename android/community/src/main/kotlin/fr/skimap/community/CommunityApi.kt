package fr.skimap.community

import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URI
import java.net.URLEncoder
import java.time.Instant
import java.util.Locale

/** What the server said to a new report. */
sealed interface SendOutcome {
    /** Stored: its identifier. */
    data class Created(
        val id: String,
    ) : SendOutcome

    /** Refused for good (expired before it arrived: 410, invalid: 400): drop it. */
    data class Dropped(
        val status: Int,
    ) : SendOutcome

    /** Try again later (no network, 429, 5xx). */
    data object Retry : SendOutcome
}

/**
 * Client of `/v1/collab/` (crates/server/docs/collab.md), blocking: call it off the main
 * thread. [base] is the server (`https://skimap.warnier.net`, or a local one in tests).
 */
class CommunityApi(
    private val base: String,
    private val userAgent: String = "skimap-android",
) {
    private fun open(
        path: String,
        method: String,
        device: String? = null,
    ): HttpURLConnection {
        val c = URI(base.trimEnd('/') + path).toURL().openConnection() as HttpURLConnection
        c.requestMethod = method
        c.connectTimeout = TIMEOUT_MS
        c.readTimeout = TIMEOUT_MS
        c.setRequestProperty("User-Agent", userAgent)
        if (device != null) c.setRequestProperty("X-Skimap-Device", device)
        return c
    }

    private fun HttpURLConnection.send(
        body: ByteArray,
        type: String,
    ) {
        doOutput = true
        setRequestProperty("Content-Type", type)
        setFixedLengthStreamingMode(body.size)
        outputStream.use { it.write(body) }
    }

    private fun HttpURLConnection.text(): String =
        (
            if (responseCode <
                HTTP_ERRORS
            ) {
                inputStream
            } else {
                errorStream
            }
        )?.use { it.readBytes().decodeToString() }.orEmpty()

    /** Sends a queued report with the time it was seen. */
    fun create(
        r: QueuedReport,
        device: String,
    ): SendOutcome {
        val body =
            JSONObject()
                .put("kind", r.kind.id)
                .put("lon", r.lon)
                .put("lat", r.lat)
                .put("seen_at", Instant.ofEpochMilli(r.seenAtMs).toString())
                .put("device", device)
        return try {
            val c = open("/v1/collab/reports", "POST", device)
            try {
                c.send(body.toString().toByteArray(), "application/json")
                val code = c.responseCode
                when {
                    code == HTTP_CREATED -> SendOutcome.Created(JSONObject(c.text()).getString("id"))
                    code == HTTP_TOO_MANY || code >= HTTP_SERVER -> SendOutcome.Retry
                    else -> SendOutcome.Dropped(code)
                }
            } finally {
                c.disconnect()
            }
        } catch (_: IOException) {
            SendOutcome.Retry
        }
    }

    /** Sends the photo of a report; the HTTP status (0: no network). */
    fun photo(
        id: String,
        device: String,
        jpeg: ByteArray,
    ): Int =
        try {
            val c = open("/v1/collab/reports/$id/photo", "POST", device)
            try {
                c.send(jpeg, "image/jpeg")
                c.responseCode
            } finally {
                c.disconnect()
            }
        } catch (_: IOException) {
            0
        }

    /** Reports shown in a box `[minLon, minLat, maxLon, maxLat]`. */
    fun list(bbox: DoubleArray): List<ServerReport> {
        val b = bbox.joinToString(",") { String.format(Locale.ROOT, "%.5f", it) }
        val c = open("/v1/collab/reports?bbox=" + URLEncoder.encode(b, "UTF-8"), "GET")
        try {
            if (c.responseCode != HTTP_OK) throw IOException("HTTP ${c.responseCode}")
            val a = JSONObject(c.text()).getJSONArray("reports")
            return (0 until a.length()).mapNotNull { parse(a.getJSONObject(it)) }
        } finally {
            c.disconnect()
        }
    }

    /** « Toujours là » or « Plus là »; the report after the vote, or null if refused. */
    fun vote(
        id: String,
        device: String,
        confirm: Boolean,
    ): ServerReport? {
        val c = open("/v1/collab/reports/$id/${if (confirm) "confirm" else "deny"}", "POST", device)
        try {
            c.send(ByteArray(0), "application/json")
            return if (c.responseCode == HTTP_OK) parse(JSONObject(c.text())) else null
        } finally {
            c.disconnect()
        }
    }

    /** Deletes one of this phone's reports. */
    fun delete(
        id: String,
        device: String,
    ): Boolean {
        val c = open("/v1/collab/reports/$id", "DELETE", device)
        try {
            return c.responseCode == HTTP_NO_CONTENT
        } finally {
            c.disconnect()
        }
    }

    /** Flags the photo of a report (deleted at once). */
    fun flagPhoto(
        id: String,
        device: String,
    ): Boolean {
        val c = open("/v1/collab/reports/$id/photo/flag", "POST", device)
        try {
            c.send(ByteArray(0), "application/json")
            return c.responseCode == HTTP_NO_CONTENT
        } finally {
            c.disconnect()
        }
    }

    /** URL of the photo of a report. */
    fun photoUrl(id: String): String = base.trimEnd('/') + "/v1/collab/photos/$id.jpg"

    companion object {
        private const val TIMEOUT_MS = 15_000
        private const val HTTP_OK = 200
        private const val HTTP_CREATED = 201
        private const val HTTP_NO_CONTENT = 204
        private const val HTTP_ERRORS = 400
        private const val HTTP_TOO_MANY = 429
        private const val HTTP_SERVER = 500

        fun parse(o: JSONObject): ServerReport? {
            val kind = ReportKind.of(o.optString("kind")) ?: return null
            return ServerReport(
                id = o.getString("id"),
                kind = kind,
                lon = o.getDouble("lon"),
                lat = o.getDouble("lat"),
                seenAtMs = Instant.parse(o.getString("seen_at")).toEpochMilli(),
                expiresAtMs = Instant.parse(o.getString("expires_at")).toEpochMilli(),
                confirmations = o.optInt("confirmations"),
                denials = o.optInt("denials"),
                photo = o.optBoolean("photo"),
            )
        }

        /** Parses a listing (tests). */
        fun parseList(json: String): List<ServerReport> {
            val a: JSONArray = JSONObject(json).getJSONArray("reports")
            return (0 until a.length()).mapNotNull { parse(a.getJSONObject(it)) }
        }
    }
}

/**
 * Sends the queue (V11): each report with the time it was seen, then its photo; a report
 * refused for good (already expired: 410) leaves the queue, one that cannot be sent now
 * stays. Returns whether something is left to retry.
 */
class ReportUploader(
    private val outbox: ReportOutbox,
    private val api: CommunityApi,
    private val device: () -> String,
    private val mine: MyReports? = null,
) {
    data class Result(
        val sent: Int,
        val dropped: Int,
        val retry: Boolean,
    )

    private enum class One { SENT, DROPPED, RETRY }

    /** One report: created (or already), then its photo. */
    private fun send(
        r: QueuedReport,
        dev: String,
    ): One {
        val id = outbox.serverId(r.localId)
        val outcome = if (id != null) SendOutcome.Created(id) else api.create(r, dev)
        return when (outcome) {
            is SendOutcome.Created -> {
                outbox.markCreated(r.localId, outcome.id)
                mine?.add(outcome.id)
                sendPhoto(r, outcome.id, dev)
            }

            is SendOutcome.Dropped -> {
                outbox.remove(r.localId)
                One.DROPPED
            }

            SendOutcome.Retry -> {
                One.RETRY
            }
        }
    }

    /** The photo, if any; the report leaves the queue once both are sent. */
    private fun sendPhoto(
        r: QueuedReport,
        id: String,
        dev: String,
    ): One {
        val code = r.photo?.let { api.photo(id, dev, it.readBytes()) } ?: HTTP_DONE
        val later = code == 0 || code == HTTP_TOO_MANY || code >= HTTP_SERVER
        if (!later) outbox.remove(r.localId)
        return if (later) One.RETRY else One.SENT
    }

    fun drain(): Result {
        val dev = device()
        val done = outbox.pending().map { send(it, dev) }
        return Result(done.count { it == One.SENT }, done.count { it == One.DROPPED }, One.RETRY in done)
    }

    private companion object {
        const val HTTP_DONE = 204
        const val HTTP_TOO_MANY = 429
        const val HTTP_SERVER = 500
    }
}
