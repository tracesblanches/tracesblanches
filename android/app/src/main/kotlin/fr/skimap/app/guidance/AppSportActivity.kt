package fr.skimap.app.guidance

import android.content.Context
import android.util.Log
import androidx.compose.runtime.Composable
import fr.skimap.app.data.Practice
import fr.skimap.app.data.PreferencesSettingsStore
import fr.skimap.app.ui.theme.SkimapTheme
import fr.skimap.sport.Outing
import fr.skimap.sport.SportActivity
import fr.skimap.sport.SportType
import fr.skimap.tracking.TrackingState

/** « Envoyer mes sorties » (module :sport) in the app's theme. */
class AppSportActivity : SportActivity() {
    @Composable
    override fun Themed(content: @Composable () -> Unit) = SkimapTheme(content = content)
}

/** The outing that ended, to send (K11): the core's GPX, the recap, the practice of the user. */
object OutingExport {
    fun of(
        context: Context,
        ended: TrackingState.Ended,
    ): Outing? {
        val gpx = ended.gpx ?: return null
        val practice = Practice.of(PreferencesSettingsStore { context.getSharedPreferences("skimap", Context.MODE_PRIVATE) }.practice())
        val sport = if (practice == Practice.SRN) SportType.BACKCOUNTRY_SKI else SportType.NORDIC_SKI
        // A trace of less than two points (stopped at once) is not an outing.
        return runCatching { Outing.of(ended.planDir, gpx, ended.recap?.text, ended.recap?.distanceKm ?: 0.0, sport) }
            .onFailure { Log.w("skimap", "outing $gpx", it) }
            .getOrNull()
    }
}
