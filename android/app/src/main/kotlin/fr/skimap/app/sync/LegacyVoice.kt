package fr.skimap.app.sync

import android.content.Context
import java.io.File

/**
 * What earlier versions kept for the pre-generated voice, removed at the start of the app:
 * the voice packs downloaded per site and language (`voice/` next to `packs/`, VO6) and
 * the volume restore of the announcements (#179). The announcements are now said by the
 * phone's own speech synthesis, and the app never changes the volume.
 */
object LegacyVoice {
    /** Input of a voice pack download queued by an earlier version ([PackSyncWorker] drops it). */
    const val WORK_KEY = "voice"

    /** Directory of the voice packs of earlier versions. */
    fun dir(context: Context): File = File(context.getExternalFilesDir(null), "voice")

    /** Preferences of the volume restore of earlier versions. */
    const val VOLUME_PREFS = "skimap_stream_boost"

    /**
     * Removes them; true when something was there. The volume saved by an earlier version
     * is dropped, never applied: the volume stays the user's.
     */
    fun cleanUp(context: Context): Boolean {
        val d = dir(context)
        val had = d.exists()
        if (had) d.deleteRecursively()
        val prefs = File(context.applicationInfo.dataDir, "shared_prefs/$VOLUME_PREFS.xml").exists()
        if (prefs) context.deleteSharedPreferences(VOLUME_PREFS)
        return had || prefs
    }
}
