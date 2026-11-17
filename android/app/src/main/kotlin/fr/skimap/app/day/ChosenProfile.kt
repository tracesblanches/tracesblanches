package fr.skimap.app.day

import fr.skimap.app.appLang
import fr.skimap.core.GroupSpec
import fr.skimap.core.GroupView
import fr.skimap.core.groupNew
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * The profile chosen during the outing (X14): the core proposes another group profile when
 * the pace of the day fits it better, the skier accepts with one gesture, and the location
 * service writes the core's `GroupProfile` beside the plan ([DayStore.PROFILE_CHOSEN_FILE]).
 * At the end of the outing that profile is the group of the day: the one that learns.
 */
object ChosenProfile {
    /**
     * The group of the profile chosen during the outing in [planDir]: the saved group of
     * that name, else of the same make-up, else a new group saved from it in [store].
     * `null` when no profile was chosen.
     */
    fun group(
        store: DayStore,
        planDir: File,
    ): GroupView? {
        val spec =
            File(planDir, DayStore.PROFILE_CHOSEN_FILE)
                .takeIf { it.isFile }
                ?.let { f -> runCatching { specOf(JSONObject(f.readText())) }.getOrNull() }
                ?: return null
        val all = store.groups()
        return all.firstOrNull { it.spec.name == spec.name }
            ?: all.firstOrNull { sameMakeUp(it.spec, spec) }
            ?: runCatching { groupNew(spec, appLang()) }.getOrNull()?.also(store::putGroup)
    }

    /** A `GroupProfile` of the core (`{name, adults, children, style, level}`) as the app edits it. */
    fun specOf(p: JSONObject): GroupSpec {
        val kids = p.optJSONArray("children") ?: JSONArray()
        return GroupSpec(
            p.getString("name"),
            p.getInt("adults").toUByte(),
            ByteArray(kids.length()) { kids.getInt(it).toByte() },
            p.getString("style") == "skating",
            p.getString("level"),
        )
    }

    private fun sameMakeUp(
        a: GroupSpec,
        b: GroupSpec,
    ) = a.adults == b.adults && a.children.contentEquals(b.children) && a.skating == b.skating && a.level == b.level
}
