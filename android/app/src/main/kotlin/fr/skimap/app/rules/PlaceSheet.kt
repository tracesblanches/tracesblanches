package fr.skimap.app.rules

import android.widget.Toast
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.BottomSheetDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
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
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import fr.skimap.app.R
import fr.skimap.app.fog.FogActivity
import fr.skimap.app.fog.Outings
import fr.skimap.app.guidance.SkiTrackingService
import fr.skimap.app.offpiste.OffpisteActivity
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.core.PlaceRules
import fr.skimap.core.placeRules
import fr.skimap.tracking.Tracking
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Test tags of the place sheet. */
object PlaceTags {
    const val SHEET = "place_sheet"
    const val FOG = "place_fog"
    const val OFFPISTE = "place_offpiste"
    const val CAR = "place_car"
}

/** A place touched long on the map. */
data class Place(
    val lat: Double,
    val lon: Double,
)

/**
 * A long touch on the map (V6 #69): the rules of the place now (bivouac, access, leaving
 * the marked routes, fire), each with its period and source, and the way there — on a
 * bearing (K7) or off-piste (V1).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PlaceSheet(
    place: Place,
    site: String?,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    var rules by remember(place) { mutableStateOf<PlaceRules?>(null) }
    LaunchedEffect(place, site) {
        rules =
            withContext(Dispatchers.IO) {
                Outings.packDir(context, site)?.let { p ->
                    runCatching { placeRules(p.path, place.lon, place.lat, System.currentTimeMillis()) }.getOrNull()
                }
            }
    }
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        modifier = Modifier.testTag(PlaceTags.SHEET),
        dragHandle = { BottomSheetDefaults.DragHandle(width = 64.dp) },
    ) {
        PlaceContent(
            rules,
            onFog = {
                onDismiss()
                context.startActivity(FogActivity.intent(context, site, place.lat, place.lon))
            },
            onOffpiste = {
                onDismiss()
                context.startActivity(OffpisteActivity.intent(context, site, place.lat, place.lon))
            },
            onCar = {
                onDismiss()
                Tracking.setCar(context, SkiTrackingService::class.java, place.lat, place.lon)
                Toast.makeText(context, R.string.place_car_done, Toast.LENGTH_SHORT).show()
            },
        )
    }
}

/** Content of [PlaceSheet], separate so that it can be captured alone. */
@Composable
fun PlaceContent(
    rules: PlaceRules?,
    onFog: () -> Unit,
    onOffpiste: () -> Unit,
    onCar: () -> Unit = {},
) {
    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).navigationBarsPadding(),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            stringResource(R.string.place_title),
            style = MaterialTheme.typography.headlineSmall,
            modifier = Modifier.semantics { heading() },
        )
        OutlinedButton(onClick = onFog, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(PlaceTags.FOG)) {
            Text(stringResource(R.string.place_fog))
        }
        OutlinedButton(onClick = onOffpiste, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(PlaceTags.OFFPISTE)) {
            Text(stringResource(R.string.place_offpiste))
        }
        // « La voiture est ici » (K4): optional, the car is recorded on its own at the start.
        OutlinedButton(onClick = onCar, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(PlaceTags.CAR)) {
            Text(stringResource(R.string.place_car))
        }
        val r = rules ?: return@Column
        Text(
            stringResource(R.string.place_rules, r.atText),
            style = MaterialTheme.typography.titleMedium,
            modifier =
                Modifier.semantics {
                    heading()
                },
        )
        if (!r.covered) Text(stringResource(R.string.place_not_covered), style = MaterialTheme.typography.bodyLarge)
        r.topics.forEach { t ->
            Surface(
                color = if (t.restrictive) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.surfaceContainer,
                contentColor = if (t.restrictive) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSurface,
                shape = MaterialTheme.shapes.small,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("${t.title} : ${t.verdict}", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(8.dp))
            }
        }
        if (r.zones.isNotEmpty()) {
            Text(
                stringResource(R.string.place_zones, r.zones.joinToString()),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        r.rules.forEach { x ->
            Column(Modifier.padding(top = 4.dp)) {
                Text("${x.topic} — ${x.verdict} · ${x.period}", style = MaterialTheme.typography.titleSmall)
                Text(x.summary, style = MaterialTheme.typography.bodyMedium)
                x.conditions?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
                x.onlyWhere?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
                x.inactive?.let { Text(stringResource(R.string.place_inactive, it), style = MaterialTheme.typography.bodySmall) }
                Text(stringResource(R.string.place_source, x.sourceTitle, x.sourceKind), style = MaterialTheme.typography.bodySmall)
                Text(x.sourceUrl, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
            }
        }
        Text(r.note, style = MaterialTheme.typography.bodySmall)
    }
}
