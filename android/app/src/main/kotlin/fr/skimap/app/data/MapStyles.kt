package fr.skimap.app.data

import android.content.res.AssetManager
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.util.concurrent.ConcurrentHashMap

/** What the user does on skis: decides the map style (K15, #123). */
enum class Practice(
    /** Style family of `styles/` (`fond`, `srn`). */
    val style: String,
) {
    /** Cross-country skiing on groomed pistes: style `fond`. */
    FOND("fond"),

    /** Nordic backcountry skiing (ski de randonnée nordique): style `srn`. */
    SRN("srn"),
    ;

    companion object {
        fun of(id: String?): Practice = entries.firstOrNull { it.style == id } ?: FOND
    }
}

/**
 * Which of the six validated styles of `styles/` the map shows: the practice, or the
 * guidance style while a day plan is followed, in the light or dark theme of the system.
 * No manipulation: the style follows the mode.
 */
data class StyleChoice(
    val practice: Practice = Practice.FOND,
    /** A day plan is being followed (agent B's guidance): style `guidage`. */
    val guiding: Boolean = false,
    /** Dark theme of the system: `sombre` variant. */
    val dark: Boolean = false,
) {
    /** Style family: `fond`, `srn` or `guidage`. */
    val family: String get() = if (guiding) GUIDANCE else practice.style

    /** Asset of the style: `styles/<family>-<clair|sombre>.json`. */
    val asset: String get() = "styles/$family-${if (dark) "sombre" else "clair"}.json"

    companion object {
        const val GUIDANCE = "guidage"
    }
}

/** The style JSON of a [StyleChoice] (relative URLs, instantiated by the core). */
fun interface StyleSource {
    suspend fun read(choice: StyleChoice): String
}

/** [StyleSource] on the assets of the app (copied from `styles/` at build time). */
class AssetStyleSource(
    private val assets: () -> AssetManager,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : StyleSource {
    private val cache = ConcurrentHashMap<String, String>()

    override suspend fun read(choice: StyleChoice): String =
        cache[choice.asset] ?: withContext(io) {
            assets().open(choice.asset).use { it.readBytes().decodeToString() }
        }.also { cache[choice.asset] = it }
}
