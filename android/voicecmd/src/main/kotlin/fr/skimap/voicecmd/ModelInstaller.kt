package fr.skimap.voicecmd

import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import java.io.File
import java.io.IOException
import java.io.RandomAccessFile
import java.net.HttpURLConnection
import java.net.URI
import java.security.MessageDigest

/**
 * Downloads the speech model into a [ModelStore]: file by file, resumed where it stopped
 * (HTTP Range on the `.part` file), each file checked against its sha256 before it takes
 * its name; the store is marked installed once all are. Cancellable between two blocks
 * (the coroutine). Run by the app under its download rules (Wi-Fi for 642 MiB).
 */
class ModelInstaller(
    private val store: ModelStore,
    private val open: (url: String) -> HttpURLConnection = { URI(it).toURL().openConnection() as HttpURLConnection },
) {
    /** Bytes done out of the total, after each block. */
    fun interface Progress {
        fun onProgress(
            done: Long,
            total: Long,
        )
    }

    suspend fun install(progress: Progress = Progress { _, _ -> }) {
        if (store.installed) return
        store.dir.mkdirs()
        val total = store.spec.totalBytes
        var before = 0L
        for (f in store.spec.files) {
            val dest = store.file(f.name)
            if (dest.length() != f.size || sha256(dest) != f.sha256) {
                fetch(f, dest) { progress.onProgress(before + it, total) }
            }
            before += f.size
            progress.onProgress(before, total)
        }
        if (store.spec.files.any { it.name == ModelSpec.TOKENIZER }) {
            BpeVocab.write(store.file(ModelSpec.TOKENIZER), store.file(ModelSpec.TOKENS), store.file(ModelSpec.BPE_VOCAB))
        }
        store.markInstalled()
    }

    private suspend fun fetch(
        f: ModelFile,
        dest: File,
        progress: (Long) -> Unit,
    ) {
        val part = File(dest.path + ".part")
        if (part.length() > f.size) part.delete()
        if (part.length() < f.size) download(f, part, progress)
        val got = sha256(part)
        if (part.length() != f.size || got != f.sha256) {
            part.delete()
            throw IOException("${f.name}: ${part.length()} bytes, sha256 $got, expected ${f.size} bytes, ${f.sha256}")
        }
        dest.delete()
        if (!part.renameTo(dest)) throw IOException("${f.name}: rename failed")
    }

    /** Appends to [part] from where it stopped (Range), or from the start if the server cannot. */
    private suspend fun download(
        f: ModelFile,
        part: File,
        progress: (Long) -> Unit,
    ) {
        var have = part.length()
        val c = open(f.url)
        try {
            c.connectTimeout = TIMEOUT_MS
            c.readTimeout = TIMEOUT_MS
            if (have > 0) c.setRequestProperty("Range", "bytes=$have-")
            when (val code = c.responseCode) {
                // No resume from this server: start again.
                HttpURLConnection.HTTP_OK -> have = 0

                HttpURLConnection.HTTP_PARTIAL -> Unit

                else -> throw IOException("${f.url}: HTTP $code")
            }
            RandomAccessFile(part, "rw").use { out ->
                out.setLength(have)
                out.seek(have)
                c.inputStream.use { copy(it, out, have, f, progress) }
            }
        } finally {
            c.disconnect()
        }
    }

    private suspend fun copy(
        input: java.io.InputStream,
        out: RandomAccessFile,
        from: Long,
        f: ModelFile,
        progress: (Long) -> Unit,
    ) {
        var have = from
        val buf = ByteArray(BLOCK)
        while (true) {
            currentCoroutineContext().ensureActive()
            val n = input.read(buf)
            if (n < 0) break
            out.write(buf, 0, n)
            have += n
            if (have > f.size) throw IOException("${f.name}: more than ${f.size} bytes")
            progress(have)
        }
    }

    companion object {
        private const val BLOCK = 1 shl 16
        private const val TIMEOUT_MS = 30_000

        fun sha256(f: File): String? {
            if (!f.isFile) return null
            val md = MessageDigest.getInstance("SHA-256")
            f.inputStream().use { i ->
                val buf = ByteArray(BLOCK)
                while (true) {
                    val n = i.read(buf)
                    if (n < 0) break
                    md.update(buf, 0, n)
                }
            }
            return md.digest().joinToString("") { "%02x".format(it) }
        }
    }
}
