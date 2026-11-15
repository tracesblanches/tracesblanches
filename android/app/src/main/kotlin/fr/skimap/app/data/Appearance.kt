package fr.skimap.app.data

import android.app.UiModeManager
import android.content.Context
import android.os.Build

/** Light or dark map and screens: the phone's theme, or the user's choice. */
enum class Appearance {
    /** The theme of the phone. */
    AUTO,

    /** Light: the most readable in full sun on snow. */
    LIGHT,

    /** Dark: dawn, night. */
    DARK,
    ;

    companion object {
        fun of(name: String?): Appearance = entries.firstOrNull { it.name == name } ?: AUTO
    }
}

/**
 * Applies the user's [Appearance] through the system (`UiModeManager`, Android 12 and
 * later), which keeps it for the app: the configuration of the activity carries it from
 * the start, so the map style loads once in the right theme and nothing is read from disk
 * on the main thread. The choice shown in the menu is kept in [SettingsStore]. Not offered
 * before Android 12.
 */
object AppearanceSetting {
    val supported: Boolean get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S

    fun apply(
        context: Context,
        appearance: Appearance,
    ) {
        if (!supported) return
        val manager = context.getSystemService(UiModeManager::class.java) ?: return
        manager.setApplicationNightMode(
            when (appearance) {
                Appearance.AUTO -> UiModeManager.MODE_NIGHT_AUTO
                Appearance.LIGHT -> UiModeManager.MODE_NIGHT_NO
                Appearance.DARK -> UiModeManager.MODE_NIGHT_YES
            },
        )
    }
}
