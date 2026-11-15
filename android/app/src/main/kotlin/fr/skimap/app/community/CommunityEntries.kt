package fr.skimap.app.community

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.community.CommunityActivity
import fr.skimap.community.R

/**
 * Entries of the module :community in the map sheet (V8 to V15): report, « Mes sorties »,
 * fix the OSM map, labelled with the module's own texts.
 */
@Composable
fun CommunityEntries(
    siteId: String,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    HorizontalDivider(modifier, thickness = 2.dp, color = MaterialTheme.colorScheme.outline)
    Text(
        stringResource(R.string.report_title) + " · " + stringResource(R.string.outings_title),
        style = MaterialTheme.typography.titleMedium,
        modifier = Modifier.semantics { heading() },
    )
    listOf(
        CommunityActivity.MODE_REPORT to R.string.report_title,
        CommunityActivity.MODE_OUTINGS to R.string.outings_title,
        CommunityActivity.MODE_OSM to R.string.osm_title,
    ).forEach { (mode, label) ->
        OutlinedButton(
            onClick = { context.startActivity(CommunityActivity.intent(context, mode, siteId)) },
            modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag("community-$mode"),
        ) { Text(stringResource(label), style = MaterialTheme.typography.labelLarge) }
    }
}
