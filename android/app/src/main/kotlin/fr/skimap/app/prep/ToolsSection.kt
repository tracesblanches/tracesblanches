@file:Suppress("MatchingDeclarationName")

package fr.skimap.app.prep

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
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
import fr.skimap.app.R
import fr.skimap.app.offpiste.OffpisteActivity
import fr.skimap.app.recap.RecapActivity
import fr.skimap.app.ui.theme.GloveTarget

/** Test tags of the tools of the menu. */
object ToolsTags {
    const val OFFPISTE = "tools_offpiste"
    const val OUTINGS = "tools_outings"
    const val SETTINGS = "tools_settings"
}

/**
 * The tools of the menu (map sheet), three entries: the off-piste route (V1: A and B are
 * the skier's choice), my outings (A10: the recap also comes on its own at the end), and
 * the settings, folded (preparation, ski mode, server alert, less exposed terrain). The
 * bearing in the fog comes on its own ([fr.skimap.app.fog.MapHint]) or from a long touch
 * on the map.
 */
@Composable
fun ToolsSection(
    site: String?,
    onOpen: () -> Unit = {},
) {
    val context = LocalContext.current
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            stringResource(R.string.tools_title),
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.semantics { heading() },
        )
        listOf(
            Triple(R.string.tools_offpiste, ToolsTags.OFFPISTE) { OffpisteActivity.intent(context, site) },
            Triple(R.string.tools_outings, ToolsTags.OUTINGS) { RecapActivity.intent(context) },
            Triple(R.string.settings_title, ToolsTags.SETTINGS) { PrepActivity.intent(context) },
        ).forEach { (label, tag, intent) ->
            OutlinedButton(
                onClick = {
                    onOpen()
                    context.startActivity(intent())
                },
                modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(tag),
            ) { Text(stringResource(label), style = MaterialTheme.typography.labelLarge) }
        }
    }
}
