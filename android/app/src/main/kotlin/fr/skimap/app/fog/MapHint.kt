package fr.skimap.app.fog

import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import fr.skimap.app.R
import fr.skimap.app.appLang
import fr.skimap.app.rules.LowExposure
import fr.skimap.app.rules.LowExposureWarning
import fr.skimap.core.fogExpected
import fr.skimap.core.isGuided
import fr.skimap.core.readDayPlan
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Test tag of the bearing hint. */
const val FOG_HINT = "fog_hint"

/** Far from the route beyond this, the bearing is offered, metres. */
const val LOST_M = 150.0

/** Why the bearing is offered on its own. */
enum class FogReason { FOG, LOST }

/** The start of the day (where the loop comes back) and whether fog is forecast now. */
private data class DayFacts(
    val name: String,
    val lat: Double,
    val lon: Double,
    val fog: Boolean,
)

/**
 * One hint at a time over the map, only when it matters (no permanent button): going back
 * (« ramène-moi », K4), the arrow to the next point of the way back when the skier is off
 * the way or in the fog ([BackArrowHint]); during a day plan, the bearing back to the start
 * (K7) is offered on its own when the plan's forecast gives fog now or the skier is far from
 * the route; otherwise the warning of the less exposed terrain layer when it is shown (V7).
 */
@Composable
fun MapHint(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val tracking by Tracking.state.collectAsStateWithLifecycle()
    val running = tracking as? TrackingState.Running
    val dir = running?.planDir?.takeIf { it.isNotEmpty() }
    var facts by remember(dir) { mutableStateOf<DayFacts?>(null) }
    LaunchedEffect(dir) {
        facts =
            dir?.let { d ->
                withContext(Dispatchers.IO) {
                    if (isGuided(d)) return@withContext null
                    runCatching {
                        val p = readDayPlan(d, appLang())
                        val fog = fogExpected(d, System.currentTimeMillis())
                        DayFacts(p.startName, p.line[1], p.line[0], fog)
                    }.getOrNull()
                }
            }
    }
    val f = facts
    // Going back: the arrow when off the way or in the fog.
    val back = running?.guidance?.back?.takeIf { running.guidance.route == "back" && (it.offWay || f?.fog == true) }
    if (running != null && back != null) {
        BackArrowHint(back, running.guidance, modifier)
        return
    }
    val reason =
        when {
            f == null || running == null -> null
            running.guidance.offRouteM >= LOST_M && running.guidance.offRouteM.isFinite() -> FogReason.LOST
            f.fog -> FogReason.FOG
            else -> null
        }
    if (f != null && reason != null) {
        FogHint(reason, f.name, modifier) { context.startActivity(FogActivity.intent(context, null, f.lat, f.lon)) }
        return
    }
    val low by LowExposure.shown.collectAsStateWithLifecycle()
    if (low) LowExposureWarning(modifier)
}

/** The bearing offered: one tap opens it. */
@Composable
fun FogHint(
    reason: FogReason,
    target: String,
    modifier: Modifier = Modifier,
    onOpen: () -> Unit,
) {
    Surface(
        onClick = onOpen,
        color = MaterialTheme.colorScheme.tertiaryContainer,
        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
        shape = MaterialTheme.shapes.medium,
        modifier = modifier.testTag(FOG_HINT),
    ) {
        Text(
            stringResource(if (reason == FogReason.FOG) R.string.fog_hint_fog else R.string.fog_hint_lost, target),
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(12.dp),
        )
    }
}
