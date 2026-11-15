package fr.skimap.app.data

import android.content.SharedPreferences
import androidx.core.content.edit
import fr.skimap.core.Bounds

/** A camera position kept between launches. */
data class CameraSpot(
    val lat: Double,
    val lon: Double,
    val zoom: Double,
) {
    fun isIn(b: Bounds): Boolean = lon in b.west..b.east && lat in b.south..b.north
}

/** User choices kept between launches. Called off the main thread (disk). */
interface SettingsStore {
    /** Last pack shown (directory), or `libre`, so that the app reopens the same way. */
    fun get(): String?

    fun set(dir: String)

    /** Practice chosen ([Practice.style]); `null`: never chosen. */
    fun practice(): String?

    fun setPractice(practice: String)

    /** Layer groups the user turned off for a style family; `null`: never chosen. */
    fun hiddenLayers(style: String): Set<String>?

    fun setHiddenLayers(
        style: String,
        hidden: Set<String>,
    )

    /** Whether packs update by themselves (WorkManager, with constraints). */
    fun autoUpdate(): Boolean

    fun setAutoUpdate(on: Boolean)

    /** Camera when the app was last left. */
    fun camera(): CameraSpot?

    fun setCamera(spot: CameraSpot)
}

/** [SettingsStore] in the app preferences, opened on first use (off the main thread). */
class PreferencesSettingsStore(
    open: () -> SharedPreferences,
) : SettingsStore {
    private val prefs by lazy(open)

    override fun get(): String? = prefs.getString(KEY_LAST_PACK, null)

    override fun set(dir: String) = prefs.edit { putString(KEY_LAST_PACK, dir) }

    override fun practice(): String? = prefs.getString(KEY_PRACTICE, null)

    override fun setPractice(practice: String) = prefs.edit { putString(KEY_PRACTICE, practice) }

    override fun hiddenLayers(style: String): Set<String>? = prefs.getStringSet(KEY_HIDDEN_GROUPS + style, null)?.toSet()

    override fun setHiddenLayers(
        style: String,
        hidden: Set<String>,
    ) = prefs.edit { putStringSet(KEY_HIDDEN_GROUPS + style, hidden) }

    override fun autoUpdate(): Boolean = prefs.getBoolean(KEY_AUTO_UPDATE, true)

    override fun setAutoUpdate(on: Boolean) = prefs.edit { putBoolean(KEY_AUTO_UPDATE, on) }

    override fun camera(): CameraSpot? =
        prefs.getString(KEY_CAMERA, null)?.split(',')?.mapNotNull { it.toDoubleOrNull() }?.let {
            if (it.size == CAMERA_FIELDS) CameraSpot(it[0], it[1], it[2]) else null
        }

    override fun setCamera(spot: CameraSpot) = prefs.edit { putString(KEY_CAMERA, "${spot.lat},${spot.lon},${spot.zoom}") }

    private companion object {
        const val KEY_LAST_PACK = "last_pack_dir"
        const val KEY_PRACTICE = "practice"
        const val KEY_AUTO_UPDATE = "auto_update"
        const val KEY_CAMERA = "camera"
        const val CAMERA_FIELDS = 3

        /** Per style family (`fond`, `srn`, `guidage`): the groups of `skimap:group`. */
        const val KEY_HIDDEN_GROUPS = "hidden_groups_"
    }
}
