package fr.skimap.app.recap

import android.content.Context
import android.util.Log
import android.widget.Toast
import androidx.compose.runtime.Composable
import fr.skimap.app.data.Practice
import fr.skimap.app.data.PreferencesSettingsStore
import fr.skimap.app.fog.Outings
import fr.skimap.app.ui.theme.SkimapTheme
import fr.skimap.core.ExportSport
import fr.skimap.core.exportOuting
import fr.skimap.sport.HealthRationaleActivity
import fr.skimap.sport.HeartSample
import fr.skimap.sport.OutingPoint
import fr.skimap.sport.OutingShare
import fr.skimap.sport.ShareResult
import fr.skimap.sport.SharedOuting
import fr.skimap.sport.SportType
import java.io.File

/** The Health Connect rationale (module :sport) in the app's theme. */
class AppHealthRationaleActivity : HealthRationaleActivity() {
    @Composable
    override fun Themed(content: @Composable () -> Unit) = SkimapTheme(content = content)
}

/**
 * The files of a finished outing for the share menu (#63): exported by the core (GPX 1.1
 * and FIT, `export_outing`) into the share directory of module :sport, with the practice
 * of the user (randonnée nordique: backcountry) and the heart rate read for it.
 */
object OutingFiles {
    private fun sport(context: Context): ExportSport {
        val practice = Practice.of(PreferencesSettingsStore { context.getSharedPreferences("skimap", Context.MODE_PRIVATE) }.practice())
        return if (practice == Practice.SRN) ExportSport.BACKCOUNTRY_SKI else ExportSport.NORDIC_SKI
    }

    /** The outing of [dir] exported; `null` when it has no recorded trace. */
    fun export(
        context: Context,
        dir: String,
        heart: List<HeartSample>,
        notes: String? = null,
    ): SharedOuting? {
        val out = OutingShare.dir(context)
        // Only the files of this share are kept in the share directory.
        out.listFiles()?.forEach(File::delete)
        return runCatching {
            exportOuting(
                dir,
                Outings.workDir(context).path,
                out.path,
                sport(context),
                heart.map { fr.skimap.core.HeartSample(it.timeMs, it.bpm.coerceAtLeast(0).toUInt()) },
            )
        }.onFailure { Log.w("skimap", "export $dir", it) }
            .getOrNull()
            ?.let { e ->
                SharedOuting(
                    id = File(e.dir).name,
                    title = e.title,
                    sport = if (e.sport == ExportSport.BACKCOUNTRY_SKI) SportType.BACKCOUNTRY_SKI else SportType.NORDIC_SKI,
                    startMs = e.startMs,
                    endMs = e.endMs,
                    distanceM = e.distanceM,
                    ascentM = e.ascentM,
                    points = e.points.map { OutingPoint(it.tMs, it.lat, it.lon, it.eleM, it.distanceM, it.heartBpm?.toInt()) },
                    gpx = File(e.gpxPath),
                    fit = File(e.fitPath),
                    notes = notes,
                )
            }
    }

    /** What the user is told once the share menu is open (or why it is not). */
    fun tell(
        context: Context,
        r: ShareResult,
    ) {
        val text =
            when {
                !r.shared -> fr.skimap.sport.R.string.sport_share_failed
                r.healthRecords > 0 -> fr.skimap.sport.R.string.sport_share_health
                else -> return
            }
        Toast.makeText(context, text, Toast.LENGTH_LONG).show()
    }
}
