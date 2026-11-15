package fr.skimap.community

import android.content.SharedPreferences
import androidx.core.content.edit
import org.json.JSONObject
import java.io.File
import java.security.SecureRandom
import java.util.UUID

/**
 * The anonymous device identifier of the reports (V8): 128 random bits, renewed every
 * [ROTATION_MS]; no account, nothing else about the phone. The server keeps only a salted
 * hash of it.
 */
class DeviceId(
    private val prefs: () -> SharedPreferences,
    private val now: () -> Long = System::currentTimeMillis,
) {
    fun current(): String {
        val p = prefs()
        val id = p.getString(KEY_ID, null)
        val since = p.getLong(KEY_SINCE, 0L)
        if (id != null && now() - since < ROTATION_MS) return id
        val bytes = ByteArray(ID_BYTES).also { SecureRandom().nextBytes(it) }
        val fresh = bytes.joinToString("") { "%02x".format(it) }
        p.edit {
            putString(KEY_ID, fresh)
            putLong(KEY_SINCE, now())
        }
        return fresh
    }

    companion object {
        const val ROTATION_MS = 30L * 24 * 3600 * 1000
        private const val ID_BYTES = 16
        private const val KEY_ID = "device_id"
        private const val KEY_SINCE = "device_since"
    }
}

/**
 * The offline queue of the reports (V11): one JSON file per report (and its reduced photo)
 * in [dir], written at the gesture, removed once the server took it or refused it for
 * good. Survives restarts; the time of the observation is the time of the gesture.
 */
class ReportOutbox(
    private val dir: File,
) {
    private fun json(id: String) = File(dir, "$id.json")

    private fun jpg(id: String) = File(dir, "$id.jpg")

    /** Queues a report seen now at a position. */
    @Synchronized
    fun add(
        kind: ReportKind,
        lon: Double,
        lat: Double,
        seenAtMs: Long,
    ): QueuedReport {
        dir.mkdirs()
        val id = UUID.randomUUID().toString()
        val o =
            JSONObject()
                .put("kind", kind.id)
                .put("lon", lon)
                .put("lat", lat)
                .put("seen_at_ms", seenAtMs)
        val tmp = File(dir, "$id.tmp")
        tmp.writeText(o.toString())
        tmp.renameTo(json(id))
        return QueuedReport(id, kind, lon, lat, seenAtMs)
    }

    /** Attaches a reduced JPEG to a queued report. False if it was already sent. */
    @Synchronized
    fun attachPhoto(
        localId: String,
        jpeg: ByteArray,
    ): Boolean {
        if (!json(localId).isFile) return false
        val tmp = File(dir, "$localId.jpg.tmp")
        tmp.writeBytes(jpeg)
        return tmp.renameTo(jpg(localId))
    }

    /** Reports waiting, oldest first. */
    @Synchronized
    fun pending(): List<QueuedReport> =
        dir
            .listFiles { f -> f.name.endsWith(".json") }
            .orEmpty()
            .mapNotNull { f ->
                runCatching {
                    val o = JSONObject(f.readText())
                    val id = f.name.removeSuffix(".json")
                    QueuedReport(
                        localId = id,
                        kind = ReportKind.of(o.getString("kind")) ?: return@mapNotNull null,
                        lon = o.getDouble("lon"),
                        lat = o.getDouble("lat"),
                        seenAtMs = o.getLong("seen_at_ms"),
                        photo = jpg(id).takeIf { it.isFile },
                    )
                }.getOrNull()
            }.sortedBy { it.seenAtMs }

    /** Removes a report from the queue (sent, or refused for good, or cancelled). */
    @Synchronized
    fun remove(localId: String) {
        json(localId).delete()
        jpg(localId).delete()
    }

    /** The report was created on the server but its photo is still to send. */
    @Synchronized
    fun markCreated(
        localId: String,
        serverId: String,
    ) {
        val f = json(localId)
        if (!f.isFile) return
        val o = JSONObject(f.readText()).put("server_id", serverId)
        f.writeText(o.toString())
    }

    /** Server identifier once created (photo pending), else null. */
    @Synchronized
    fun serverId(localId: String): String? =
        runCatching { JSONObject(json(localId).readText()).optString("server_id").ifEmpty { null } }.getOrNull()

    fun size(): Int = pending().size
}

/** Identifiers of the reports this phone made (no votes on them, « Retirer »). */
class MyReports(
    private val prefs: () -> SharedPreferences,
) {
    fun all(): Set<String> = prefs().getStringSet(KEY, emptySet()).orEmpty()

    fun add(id: String) {
        prefs().edit { putStringSet(KEY, all() + id) }
    }

    fun remove(id: String) {
        prefs().edit { putStringSet(KEY, all() - id) }
    }

    private companion object {
        const val KEY = "my_reports"
    }
}
