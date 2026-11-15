package fr.skimap.app.data

import android.content.SharedPreferences
import androidx.core.content.edit

/** User choices kept between launches. Called off the main thread (disk). */
interface SettingsStore {
    /** Last pack shown, so that the app reopens on the same site. */
    fun get(): String?

    fun set(dir: String)

    /** Layer switches the user turned off for a map style (basemap); `null`: never chosen. */
    fun hiddenLayers(style: String): Set<String>?

    fun setHiddenLayers(
        style: String,
        hidden: Set<String>,
    )
}

/** [SettingsStore] in the app preferences, opened on first use (off the main thread). */
class PreferencesSettingsStore(
    open: () -> SharedPreferences,
) : SettingsStore {
    private val prefs by lazy(open)

    override fun get(): String? = prefs.getString(KEY_LAST_PACK, null)

    override fun set(dir: String) = prefs.edit { putString(KEY_LAST_PACK, dir) }

    override fun hiddenLayers(style: String): Set<String>? = prefs.getStringSet(KEY_HIDDEN + style, null)?.toSet()

    override fun setHiddenLayers(
        style: String,
        hidden: Set<String>,
    ) = prefs.edit { putStringSet(KEY_HIDDEN + style, hidden) }

    private companion object {
        const val KEY_LAST_PACK = "last_pack_dir"
        const val KEY_HIDDEN = "hidden_layers_"
    }
}
