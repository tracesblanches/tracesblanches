package fr.skimap.app.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
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
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
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
import androidx.core.net.toUri
import fr.skimap.app.R
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.core.Card

/** Test tag of the notice under the links of a card. */
const val CARD_NOTICE_TAG = "card-notice"

/** Card of a touched feature, in a Material 3 bottom sheet. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CardSheet(
    card: Card,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    var notice by remember(card) { mutableStateOf<String?>(null) }
    val offline = stringResource(R.string.card_link_offline)
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        modifier = Modifier.testTag(Tags.CARD),
        dragHandle = { BottomSheetDefaults.DragHandle(width = 64.dp) },
    ) {
        CardContent(
            card,
            // The dialler opens with the number: the user calls, the app never does.
            onDial = { dial -> start(context, Intent(Intent.ACTION_DIAL, "tel:$dial".toUri())) },
            // A link (official piste plan, website) opens in the browser; offline, the
            // card says so instead of opening a page that cannot load.
            onOpen = { url ->
                if (online(context)) {
                    notice = null
                    start(context, Intent(Intent.ACTION_VIEW, url.toUri()))
                } else {
                    notice = offline
                }
            },
            notice = notice,
        )
    }
}

/** Whether the phone has a network that reaches the internet. */
private fun online(context: Context): Boolean {
    val cm = context.getSystemService(ConnectivityManager::class.java)
    val caps = cm?.getNetworkCapabilities(cm.activeNetwork)
    return caps?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true
}

private fun start(
    context: Context,
    intent: Intent,
) {
    try {
        context.startActivity(intent)
    } catch (_: ActivityNotFoundException) {
        // No dialler or browser on this device: nothing to open.
    }
}

/**
 * Content of [CardSheet], separate so that it can be captured and tested alone. [notice]:
 * a message under the links (a link that cannot open offline).
 */
@Composable
fun CardContent(
    card: Card,
    onDial: (String) -> Unit,
    onOpen: (String) -> Unit,
    modifier: Modifier = Modifier,
    notice: String? = null,
) {
    Column(
        modifier
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .navigationBarsPadding(),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(card.kind, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
        Text(card.title, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.semantics { heading() })
        card.phones.forEach { p ->
            OutlinedButton(onClick = { onDial(p.dial) }, modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget)) {
                Text(stringResource(R.string.card_call, p.label, p.display), style = MaterialTheme.typography.labelLarge)
            }
        }
        card.facts.forEach { f ->
            Column {
                Text(f.label, style = MaterialTheme.typography.titleMedium)
                Text(f.value, style = MaterialTheme.typography.bodyLarge)
            }
        }
        card.links.forEach { l ->
            TextButton(onClick = { onOpen(l.url) }, modifier = Modifier.heightIn(min = GloveTarget)) {
                Text(l.label, style = MaterialTheme.typography.labelLarge)
            }
        }
        if (notice != null) {
            Text(
                notice,
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.testTag(CARD_NOTICE_TAG),
            )
        }
        val updated = card.updated
        Text(
            if (updated != null) stringResource(R.string.card_source_dated, card.source, updated) else card.source,
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.padding(bottom = 16.dp),
        )
    }
}
