package fr.skimap.app.sync

import android.util.Log
import fr.skimap.core.ServerSite
import fr.skimap.core.parseServerCatalog
import fr.skimap.sync.Fetched
import fr.skimap.sync.HttpSource
import java.io.File
import java.io.IOException

/**
 * The sites the server publishes (`/v1/catalog.json`), kept on the phone: read from the
 * network when the area picker opens (one request, 304 when unchanged), from the copy
 * otherwise, so that the picker works offline and shows what can be downloaded later.
 */
class ServerCatalog(
    private val http: () -> HttpSource,
    /** Directory of the copy (`catalog.json`, `catalog.etag`). */
    private val dir: File,
) {
    private val body get() = File(dir, "catalog.json")
    private val etag get() = File(dir, "catalog.etag")

    /** The copy on the phone; empty before the first successful request. */
    fun cached(): List<ServerSite> =
        body
            .takeIf { it.isFile }
            ?.let {
                try {
                    parseServerCatalog(it.readText())
                } catch (_: fr.skimap.core.SyncException) {
                    null
                }
            }.orEmpty()

    /** Revalidates the copy with the server; `null` when the server cannot be reached. */
    fun refresh(): List<ServerSite>? {
        val known = etag.takeIf { it.isFile && body.isFile }?.readText()
        return try {
            when (val r = http().fetch(CATALOG_URL, known)) {
                Fetched.NotModified -> {
                    cached()
                }

                is Fetched.Body -> {
                    val sites = parseServerCatalog(r.bytes.decodeToString())
                    dir.mkdirs()
                    val tmp = File(dir, "catalog.json.tmp")
                    tmp.writeBytes(r.bytes)
                    tmp.renameTo(body)
                    if (r.etag != null) etag.writeText(r.etag!!) else etag.delete()
                    sites
                }
            }
        } catch (e: IOException) {
            Log.i(TAG, "server catalog not refreshed, the copy is used: $e")
            null
        }
    }

    companion object {
        const val CATALOG_URL = "/v1/catalog.json"
        private const val TAG = "skimap"
    }
}
