package fr.skimap.app.fog

import android.hardware.SensorManager
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import fr.skimap.app.R
import fr.skimap.core.BearingView
import fr.skimap.core.fogBearing
import fr.skimap.tracking.BackGuide
import fr.skimap.tracking.Guidance
import java.util.Locale

/**
 * « Ramène-moi » over the map (K4), on its own when it is useful (off the way, or fog now:
 * [MapHint]), without any button: an arrow that points to the next useful point of the way
 * back computed on the phone (never straight across the terrain to the car), with the
 * distance and the time left. The fog bearing's own tools (K7) do the work: the phone's
 * compass while the arrow is shown ([Compass]) and the core's bearing ([fogBearing]); without
 * a compass, the arrow is drawn against north with the bearing in words.
 */
@Composable
fun BackArrowHint(
    back: BackGuide,
    guidance: Guidance,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val compass = remember { context.getSystemService(SensorManager::class.java)?.let(::Compass) }
    DisposableEffect(compass) {
        val on = compass?.start() == true
        onDispose { if (on) compass?.stop() }
    }
    val idle = remember { kotlinx.coroutines.flow.MutableStateFlow(CompassReading(null, Calibration.NONE)) }
    val reading by (compass?.reading ?: idle).collectAsStateWithLifecycle()
    val bearing =
        remember(back, reading.headingDeg) {
            fogBearing(back.hereLon, back.hereLat, back.guideLon, back.guideLat, reading.headingDeg, Outings.lang())
        }
    BackArrow(back, guidance, bearing, modifier)
}

/** The arrow of « ramène-moi », stateless (captured in the JVM tests). */
@Composable
fun BackArrow(
    back: BackGuide,
    guidance: Guidance,
    bearing: BearingView,
    modifier: Modifier = Modifier,
) {
    val to =
        when {
            back.target == "car" -> stringResource(R.string.back_to_car)
            back.target == "destination" && back.targetName != null -> stringResource(R.string.back_to_destination, back.targetName!!)
            back.target == "destination" -> stringResource(R.string.back_to_finish)
            else -> stringResource(R.string.back_to_start)
        }
    val locale = LocalConfiguration.current.locales.get(0) ?: Locale.ROOT
    val left = stringResource(R.string.back_left, distanceText(guidance.remainingM, locale), guidance.remainingText)
    val next = stringResource(R.string.back_next_point, bearing.distanceText)
    val description = stringResource(R.string.back_arrow) + (bearing.turnText?.let { ", $it" } ?: ", ${bearing.bearingText}")
    Surface(
        color = MaterialTheme.colorScheme.tertiaryContainer,
        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
        shape = MaterialTheme.shapes.medium,
        modifier = modifier.testTag(FogTags.BACK_HINT).semantics { liveRegion = LiveRegionMode.Polite },
    ) {
        Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            // With the compass: turned from where the phone points; without: from north.
            val angle = bearing.relativeDeg ?: bearing.bearingDeg
            BearingArrow(angle, Modifier.size(72.dp).testTag(FogTags.BACK_ARROW).semantics { contentDescription = description })
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(to, style = MaterialTheme.typography.titleMedium)
                Text(left, style = MaterialTheme.typography.bodyLarge)
                Text(next, style = MaterialTheme.typography.bodyMedium)
                Text(bearing.turnText ?: bearing.bearingText, style = MaterialTheme.typography.bodyMedium)
                if (bearing.relativeDeg == null) Text(stringResource(R.string.back_no_compass), style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}

/** « 2,3 km », « 850 m » in the language of the phone. */
fun distanceText(
    m: Double,
    locale: Locale = Locale.getDefault(),
): String =
    if (m >= KM) {
        String.format(locale, "%.1f km", m / KM)
    } else {
        String.format(locale, "%d m", (m / TEN_M).toLong() * TEN_M.toLong())
    }

private const val KM = 1000.0
private const val TEN_M = 10.0
