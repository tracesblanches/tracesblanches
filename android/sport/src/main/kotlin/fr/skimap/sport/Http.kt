package fr.skimap.sport

import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.util.UUID

/** An HTTP answer: status and body (text). */
data class HttpReply(
    val code: Int,
    val body: String,
)

/** A part of a `multipart/form-data` body: a field or a file. */
sealed interface Part {
    val name: String

    data class Field(
        override val name: String,
        val value: String,
    ) : Part

    data class FilePart(
        override val name: String,
        val file: File,
        val contentType: String,
    ) : Part
}

/**
 * The few HTTP calls of the module (JSON, form, multipart), with the platform's
 * [HttpURLConnection]: no client library. Blocking: called from WorkManager or
 * `Dispatchers.IO`. A network failure is an [IOException].
 */
class Http(
    private val userAgent: String = "skimap-android",
    private val timeoutMs: Int = TIMEOUT_MS,
) {
    fun get(
        url: String,
        bearer: String? = null,
    ): HttpReply = call("GET", url, bearer, null, null)

    fun json(
        method: String,
        url: String,
        body: String,
        bearer: String? = null,
    ): HttpReply = call(method, url, bearer, "application/json", body.toByteArray())

    fun multipart(
        url: String,
        parts: List<Part>,
        bearer: String?,
    ): HttpReply {
        val boundary = "skimap-" + UUID.randomUUID()
        val out = ByteArrayOutputStream()
        for (p in parts) {
            out.write("--$boundary\r\n".toByteArray())
            when (p) {
                is Part.Field -> {
                    out.write("Content-Disposition: form-data; name=\"${p.name}\"\r\n\r\n".toByteArray())
                    out.write(p.value.toByteArray())
                }

                is Part.FilePart -> {
                    out.write(
                        (
                            "Content-Disposition: form-data; name=\"${p.name}\"; filename=\"${p.file.name}\"\r\n" +
                                "Content-Type: ${p.contentType}\r\n\r\n"
                        ).toByteArray(),
                    )
                    out.write(p.file.readBytes())
                }
            }
            out.write("\r\n".toByteArray())
        }
        out.write("--$boundary--\r\n".toByteArray())
        return call("POST", url, bearer, "multipart/form-data; boundary=$boundary", out.toByteArray())
    }

    private fun call(
        method: String,
        url: String,
        bearer: String?,
        contentType: String?,
        body: ByteArray?,
    ): HttpReply {
        val c = URL(url).openConnection() as HttpURLConnection
        try {
            c.requestMethod = method
            c.connectTimeout = timeoutMs
            c.readTimeout = timeoutMs
            c.instanceFollowRedirects = false
            c.setRequestProperty("User-Agent", userAgent)
            c.setRequestProperty("Accept", "application/json")
            bearer?.let { c.setRequestProperty("Authorization", "Bearer $it") }
            if (body != null) {
                c.doOutput = true
                c.setRequestProperty("Content-Type", contentType)
                c.setFixedLengthStreamingMode(body.size)
                c.outputStream.use { it.write(body) }
            }
            val code = c.responseCode
            val stream = if (code >= HttpURLConnection.HTTP_BAD_REQUEST) c.errorStream else c.inputStream
            val text = stream?.use { it.readBytes().decodeToString() }.orEmpty()
            return HttpReply(code, text)
        } finally {
            c.disconnect()
        }
    }

    private companion object {
        const val TIMEOUT_MS = 30_000
    }
}
