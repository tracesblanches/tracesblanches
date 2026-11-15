package fr.skimap.sync

import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URI

/** Answer of a revalidated document (`catalog.json`, `index.json`). */
sealed interface Fetched {
    /** 304: the copy the phone has is current. */
    data object NotModified : Fetched

    /** 200: a new body and its `ETag`. */
    class Body(
        val bytes: ByteArray,
        val etag: String?,
    ) : Fetched
}

/** An HTTP status the app does not expect (`error` and `message` of the server body). */
class HttpStatusException(
    val status: Int,
    val url: String,
) : IOException("HTTP $status for $url")

/** Bytes moved by one download call. */
data class Downloaded(
    /** Bytes received over the network by this call. */
    val received: Long,
    /** Bytes the file already had when the call started (kept: resumed download). */
    val resumedFrom: Long,
)

/**
 * HTTP of the pack sync (K2): small revalidated documents and resumable downloads of
 * immutable bundles, as `crates/server/docs/api.md` describes them. Blocking calls: run
 * them on `Dispatchers.IO`. [base] is the server (`https://skimap.warnier.net`); URLs of
 * the server documents are relative to it.
 */
class HttpSource(
    private val base: String,
    private val userAgent: String,
    private val connectTimeoutMs: Int = CONNECT_TIMEOUT_MS,
    private val readTimeoutMs: Int = READ_TIMEOUT_MS,
) {
    /** Requests made by this source (measurements). */
    @Volatile
    var requests: Int = 0
        private set

    /** Body bytes received by this source (measurements). */
    @Volatile
    var bodyBytes: Long = 0
        private set

    fun url(path: String): String = if (path.startsWith("http://") || path.startsWith("https://")) path else base.trimEnd('/') + path

    private fun open(path: String): HttpURLConnection {
        val c = URI(url(path)).toURL().openConnection() as HttpURLConnection
        c.connectTimeout = connectTimeoutMs
        c.readTimeout = readTimeoutMs
        c.instanceFollowRedirects = true
        // Cloudflare's browser check lets clients with a User-Agent through (api.md).
        c.setRequestProperty("User-Agent", userAgent)
        requests++
        return c
    }

    /** GET with `If-None-Match` = [etag]: 304 gives [Fetched.NotModified]. */
    fun fetch(
        path: String,
        etag: String?,
    ): Fetched {
        val c = open(path)
        try {
            if (etag != null) c.setRequestProperty("If-None-Match", etag)
            return when (val status = c.responseCode) {
                HttpURLConnection.HTTP_NOT_MODIFIED -> {
                    Fetched.NotModified
                }

                HttpURLConnection.HTTP_OK -> {
                    val body = c.inputStream.use { it.readBytes() }
                    bodyBytes += body.size
                    Fetched.Body(body, c.getHeaderField("ETag"))
                }

                else -> {
                    throw HttpStatusException(status, url(path))
                }
            }
        } finally {
            c.disconnect()
        }
    }

    /**
     * Downloads [path] into [dest], resuming a partial file: `Range: bytes=<size>-` with
     * `If-Range: "<sha256>"` (the `ETag` of a bundle is its SHA-256). A 200 to a range
     * request (the file changed, or a cache answered with the whole file) starts again from
     * zero; a 416 (range past the end: a stale or wrong partial file) too. [bytes] is the
     * expected size; [onProgress] gets (bytes in the file, [bytes]).
     */
    fun download(
        path: String,
        dest: File,
        sha256: String,
        bytes: Long,
        onProgress: (Long, Long) -> Unit = { _, _ -> },
    ): Downloaded {
        dest.parentFile?.mkdirs()
        if (dest.isFile && dest.length() > bytes) dest.delete()
        val have = if (dest.isFile) dest.length() else 0L
        if (have == bytes) return Downloaded(0, have)
        // 416: the partial file does not fit the bundle; from zero, once.
        val got =
            transfer(path, dest, sha256, have, bytes, onProgress)
                ?: dest.delete().let { transfer(path, dest, sha256, 0, bytes, onProgress) }
                ?: throw HttpStatusException(RANGE_NOT_SATISFIABLE, url(path))
        // A connection closed early is not always an error for the HTTP stack: the
        // length decides. The bytes received are kept for the next attempt.
        if (got.resumedFrom + got.received < bytes) {
            throw IOException("${url(path)}: ${got.resumedFrom + got.received} of $bytes bytes, connection closed")
        }
        return got
    }

    /** One GET from byte [have] (0: the whole file); `null` on 416. */
    private fun transfer(
        path: String,
        dest: File,
        sha256: String,
        have: Long,
        bytes: Long,
        onProgress: (Long, Long) -> Unit,
    ): Downloaded? {
        val c = open(path)
        try {
            // Offsets must be those of the file itself, never of a compressed transfer.
            c.setRequestProperty("Accept-Encoding", "identity")
            if (have > 0) {
                c.setRequestProperty("Range", "bytes=$have-")
                c.setRequestProperty("If-Range", "\"$sha256\"")
            }
            val start =
                when (val status = c.responseCode) {
                    HttpURLConnection.HTTP_PARTIAL -> checkedStart(c.getHeaderField("Content-Range"), have)
                    HttpURLConnection.HTTP_OK -> 0L
                    RANGE_NOT_SATISFIABLE -> return null
                    else -> throw HttpStatusException(status, url(path))
                }
            val received =
                c.inputStream.use { input ->
                    FileOutputStream(dest, start > 0).use { out -> copy(input, out, start, bytes, onProgress) }
                }
            return Downloaded(received, start)
        } finally {
            c.disconnect()
        }
    }

    private fun checkedStart(
        contentRange: String?,
        have: Long,
    ): Long {
        val start = contentRangeStart(contentRange)
        if (start != have) throw IOException("Content-Range starts at $start, expected $have")
        return start
    }

    private fun copy(
        input: InputStream,
        out: FileOutputStream,
        start: Long,
        total: Long,
        onProgress: (Long, Long) -> Unit,
    ): Long {
        val buf = ByteArray(BUFFER)
        var n = 0L
        var reported = 0L
        while (true) {
            val k = input.read(buf)
            if (k < 0) break
            out.write(buf, 0, k)
            n += k
            bodyBytes += k
            // Progress every 256 kB: no busy UI updates.
            if (n - reported >= PROGRESS_STEP) {
                reported = n
                onProgress(start + n, total)
            }
        }
        out.fd.sync()
        onProgress(start + n, total)
        return n
    }

    companion object {
        const val CONNECT_TIMEOUT_MS = 15_000
        const val READ_TIMEOUT_MS = 30_000
        private const val RANGE_NOT_SATISFIABLE = 416
        private const val BUFFER = 64 * 1024
        private const val PROGRESS_STEP = 256 * 1024L

        /** First byte of `Content-Range: bytes <first>-<last>/<size>`. */
        fun contentRangeStart(header: String?): Long =
            header
                ?.removePrefix("bytes ")
                ?.substringBefore('-')
                ?.trim()
                ?.toLongOrNull() ?: -1
    }
}
