package fr.skimap.app.day

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit
import fr.skimap.core.GroupSpec
import fr.skimap.core.GroupView
import fr.skimap.core.groupLearn
import fr.skimap.core.groupNew
import fr.skimap.core.groupRead
import org.json.JSONArray
import java.io.File

/** The contact who gets the plan (K10): the user's SMS app is opened on this number. */
data class Contact(
    val name: String,
    val number: String,
)

/**
 * What the morning keeps on the phone: the groups and what they learned (C11), the
 * contact (K10), the plans of the days. Groups are the core's models (JSON), stored as
 * they are.
 */
class DayStore(
    private val prefs: () -> SharedPreferences,
    private val filesDir: () -> File,
) {
    constructor(context: Context) : this(
        { context.getSharedPreferences(PREFS, Context.MODE_PRIVATE) },
        { context.filesDir },
    )

    fun contact(): Contact? {
        val p = prefs()
        val number = p.getString(KEY_NUMBER, null)?.takeIf { it.isNotBlank() } ?: return null
        return Contact(p.getString(KEY_NAME, null).orEmpty(), number)
    }

    fun setContact(c: Contact?) {
        prefs().edit {
            putString(KEY_NAME, c?.name)
            putString(KEY_NUMBER, c?.number)
        }
    }

    /** Saved groups; « seul » and « en famille » the first time. */
    fun groups(): List<GroupView> {
        val raw = prefs().getString(KEY_GROUPS, null)
        if (raw == null) {
            val defaults =
                listOf(
                    groupNew(GroupSpec("Seul", 1u, byteArrayOf(), false, "intermediate")),
                    groupNew(GroupSpec("En famille", 2u, FAMILY_AGES, false, "intermediate")),
                )
            saveGroups(defaults)
            return defaults
        }
        val a = JSONArray(raw)
        return (0 until a.length()).mapNotNull { runCatching { groupRead(a.getString(it)) }.getOrNull() }
    }

    fun saveGroups(groups: List<GroupView>) {
        val a = JSONArray()
        groups.forEach { a.put(it.modelJson) }
        prefs().edit { putString(KEY_GROUPS, a.toString()) }
    }

    /** Adds or replaces (same name) a group. */
    fun putGroup(g: GroupView) {
        saveGroups(groups().filterNot { it.spec.name == g.spec.name } + g)
    }

    fun selectedGroup(): String? = prefs().getString(KEY_SELECTED, null)

    fun selectGroup(name: String?) {
        prefs().edit { putString(KEY_SELECTED, name) }
    }

    /** A new directory for the plan of a day. */
    fun newPlanDir(date: String): File {
        val root = File(filesDir(), "plans")
        var n = 1
        while (File(root, "$date-$n").exists()) n++
        return File(root, "$date-$n").also { it.mkdirs() }
    }

    /** The group a plan was made for (its name), written at validation. */
    fun setPlanGroup(
        planDir: String,
        name: String?,
    ) {
        val f = File(planDir, GROUP_FILE)
        if (name == null) f.delete() else f.writeText(name)
    }

    /**
     * The end of an outing (C11): the group of the plan learns what the outing told
     * (`observation.json` written by the location service). Once per plan.
     */
    fun learnFrom(planDir: String): GroupView? {
        val dir = File(planDir)
        val name = File(dir, GROUP_FILE).takeIf { it.isFile }?.readText()
        val obs = File(dir, OBSERVATION_FILE).takeIf { it.isFile }?.readText()
        val done = File(dir, LEARNED_FILE)
        val g = groups().firstOrNull { it.spec.name == name }
        if (obs == null || g == null || done.exists()) return null
        val learned = groupLearn(g.modelJson, obs)
        putGroup(learned)
        done.writeText(learned.outings.toString())
        return learned
    }

    companion object {
        const val PREFS = "skimap_day"
        private const val KEY_NAME = "contact_name"
        private const val KEY_NUMBER = "contact_number"
        private const val KEY_GROUPS = "groups"
        private const val KEY_SELECTED = "group"
        const val GROUP_FILE = "group.txt"
        const val OBSERVATION_FILE = "observation.json"
        const val LEARNED_FILE = "learned.txt"

        /** « En famille » the first time: children of 6 and 9, the family of the scenarios (Q9). */
        private val FAMILY_AGES = byteArrayOf(6, 9)
    }
}
