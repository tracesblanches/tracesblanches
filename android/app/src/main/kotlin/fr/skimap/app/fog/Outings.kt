package fr.skimap.app.fog

import android.content.Context
import fr.skimap.app.MainActivity
import fr.skimap.app.appLang
import fr.skimap.app.day.DayStore
import fr.skimap.app.day.group
import fr.skimap.core.Lang
import java.io.File
import java.time.LocalDate
import java.util.Locale

/**
 * What the outing tools share (K7, V1, A10, E2): the packs of the phone, the group whose
 * pace gives the times (C11, the one chosen in the morning), the outing directories.
 */
object Outings {
    /** Pack directories (one per area, with its `manifest.json`), by name. */
    fun packDirs(context: Context): List<File> =
        MainActivity
            .packsDir(context)
            .listFiles()
            .orEmpty()
            .filter { File(it, "manifest.json").isFile }
            .sortedBy { it.name }

    /** The pack of [site] (its directory name), else the first pack. */
    fun packDir(
        context: Context,
        site: String?,
    ): File? = packDirs(context).let { all -> all.firstOrNull { it.name == site } ?: all.firstOrNull() }

    /**
     * The group of the morning (C11, [DayStore.group]: the one chosen, else the one the
     * morning offers first), its model as JSON; `null` if none.
     */
    fun groupJson(context: Context): String? = DayStore(context).group()?.modelJson

    /** Where the outings are kept (the day plans and the guided outings). */
    fun plansRoot(context: Context): File = File(context.filesDir, "plans")

    /** A new outing directory. */
    fun newOutingDir(context: Context): File = DayStore(context).newPlanDir(LocalDate.now().toString())

    /** Private storage of the inflated rasters and the pack views. */
    fun workDir(context: Context): File = File(context.filesDir, "packview")

    /** Language of the texts said: French on a French phone, English otherwise. */
    fun lang(): Lang = appLang()
}
