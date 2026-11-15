package fr.skimap.sport

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/** Test tags of the sending screen. */
object SportTags {
    const val HEALTH_ALLOW = "sport_health_allow"
    const val STRAVA_CONNECT = "sport_strava_connect"
    const val STRAVA_DISCONNECT = "sport_strava_disconnect"
    const val AUTO = "sport_auto"
    const val SEND = "sport_send"
}

/** Callbacks of [SportScreen]. */
data class SportActions(
    val allowHealth: () -> Unit = {},
    val installHealth: () -> Unit = {},
    val connectStrava: () -> Unit = {},
    val disconnectStrava: () -> Unit = {},
    val setAutoSend: (Boolean) -> Unit = {},
    val close: () -> Unit = {},
)

/** Touch targets for gloves (as the app's). */
private val Target = 72.dp

/**
 * « Envoyer mes sorties » (K11): Health Connect (permission), Strava (connect, disconnect;
 * a sober line when the skimap service has no Strava application yet), the automatic
 * sending at the end of the outing, and the outings queued with where they are.
 */
@Composable
fun SportScreen(
    state: SportUi,
    actions: SportActions,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.safeDrawing)
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            stringResource(R.string.sport_title),
            style = MaterialTheme.typography.headlineSmall,
            modifier = Modifier.semantics { heading() },
        )
        Text(stringResource(R.string.sport_intro), style = MaterialTheme.typography.bodyLarge)
        state.error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyLarge) }
        Section(stringResource(R.string.sport_health))
        HealthPart(state.health, actions)
        Section(stringResource(R.string.sport_strava))
        StravaPart(state.strava, actions)
        HorizontalDivider()
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = Target)
                .toggleable(value = state.autoSend, role = Role.Switch, onValueChange = actions.setAutoSend)
                .testTag(SportTags.AUTO),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(stringResource(R.string.sport_auto), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Switch(checked = state.autoSend, onCheckedChange = null)
        }
        Section(stringResource(R.string.sport_queue))
        if (state.queue.isEmpty()) {
            Text(stringResource(R.string.sport_queue_empty), style = MaterialTheme.typography.bodyMedium)
        } else {
            state.queue.forEach { Text(queueLine(it), style = MaterialTheme.typography.bodyMedium) }
        }
        Button(onClick = actions.close, modifier = Modifier.fillMaxWidth().heightIn(min = Target)) {
            Text(stringResource(R.string.sport_close))
        }
    }
}

@Composable
private fun HealthPart(
    h: HealthUi,
    actions: SportActions,
) {
    when (h) {
        HealthUi.READY -> {
            Text(stringResource(R.string.sport_health_ready), style = MaterialTheme.typography.bodyLarge)
        }

        HealthUi.NOT_ALLOWED -> {
            Text(stringResource(R.string.sport_health_ask), style = MaterialTheme.typography.bodyLarge)
            Button(
                onClick = actions.allowHealth,
                modifier = Modifier.fillMaxWidth().heightIn(min = Target).testTag(SportTags.HEALTH_ALLOW),
            ) { Text(stringResource(R.string.sport_health_allow)) }
        }

        HealthUi.NEEDS_UPDATE -> {
            Text(stringResource(R.string.sport_health_update), style = MaterialTheme.typography.bodyLarge)
            OutlinedButton(onClick = actions.installHealth, modifier = Modifier.fillMaxWidth().heightIn(min = Target)) {
                Text(stringResource(R.string.sport_health_install))
            }
        }

        HealthUi.UNAVAILABLE -> {
            Text(stringResource(R.string.sport_health_unavailable), style = MaterialTheme.typography.bodyLarge)
        }
    }
}

@Composable
private fun StravaPart(
    s: StravaUi,
    actions: SportActions,
) {
    val big = Modifier.fillMaxWidth().heightIn(min = Target)
    when (s) {
        StravaUi.Checking -> {
            Text(stringResource(R.string.sport_strava_checking), style = MaterialTheme.typography.bodyLarge)
        }

        StravaUi.NotConfigured -> {
            Text(stringResource(R.string.sport_strava_not_configured), style = MaterialTheme.typography.bodyLarge)
        }

        StravaUi.Unreachable -> {
            Text(stringResource(R.string.sport_strava_unreachable), style = MaterialTheme.typography.bodyLarge)
        }

        StravaUi.Disconnected -> {
            Button(onClick = actions.connectStrava, modifier = big.testTag(SportTags.STRAVA_CONNECT)) {
                Text(stringResource(R.string.sport_strava_connect))
            }
            Text(stringResource(R.string.sport_strava_hint), style = MaterialTheme.typography.bodyMedium)
        }

        StravaUi.Connecting -> {
            Text(stringResource(R.string.sport_strava_connecting), style = MaterialTheme.typography.bodyLarge)
        }

        is StravaUi.Connected -> {
            Text(
                s.athlete?.let { stringResource(R.string.sport_strava_connected, it) }
                    ?: stringResource(R.string.sport_strava_connected_anonymous),
                style = MaterialTheme.typography.bodyLarge,
            )
            Text(stringResource(R.string.sport_strava_hint), style = MaterialTheme.typography.bodyMedium)
            OutlinedButton(onClick = actions.disconnectStrava, modifier = big.testTag(SportTags.STRAVA_DISCONNECT)) {
                Text(stringResource(R.string.sport_strava_disconnect))
            }
        }
    }
}

/** « Boucle du Chaumoz · Health Connect envoyée · Strava en attente ». */
@Composable
fun queueLine(q: Queued): String = stringResource(R.string.sport_line, q.outing.name, deliveryText(q.health), deliveryText(q.strava))

/** Where one destination is, in words. */
@Composable
fun deliveryText(d: Delivery): String =
    when (d.step) {
        Step.PENDING -> {
            stringResource(R.string.sport_step_pending)
        }

        Step.UPLOADING -> {
            stringResource(R.string.sport_step_uploading)
        }

        Step.DONE -> {
            stringResource(R.string.sport_step_done)
        }

        Step.FAILED -> {
            if (d.detail == Sport.NOT_CONNECTED) {
                stringResource(R.string.sport_step_not_connected)
            } else {
                stringResource(R.string.sport_step_failed, d.detail.orEmpty())
            }
        }

        Step.OFF -> {
            when (d.detail) {
                Sport.NOT_ALLOWED -> stringResource(R.string.sport_step_not_allowed)
                Sport.NOT_CONNECTED -> stringResource(R.string.sport_step_not_connected)
                else -> stringResource(R.string.sport_line_off)
            }
        }
    }

/** The « Envoyer » of the recap: the button, and where the outing is once sent. */
@Composable
fun SendOuting(
    sent: Queued?,
    onSend: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Button(onClick = onSend, modifier = Modifier.fillMaxWidth().heightIn(min = Target).testTag(SportTags.SEND)) {
            Text(stringResource(R.string.sport_send))
        }
        sent?.let { Text(queueLine(it), style = MaterialTheme.typography.bodyMedium) }
    }
}

@Composable
private fun Section(text: String) {
    HorizontalDivider()
    Text(text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
}

/** The Health Connect rationale (the privacy policy of what is written). */
@Composable
fun HealthRationale(
    onClose: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.safeDrawing)
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            stringResource(R.string.sport_rationale_title),
            style = MaterialTheme.typography.headlineSmall,
            modifier = Modifier.semantics { heading() },
        )
        Text(stringResource(R.string.sport_rationale_text), style = MaterialTheme.typography.bodyLarge)
        Button(onClick = onClose, modifier = Modifier.fillMaxWidth().heightIn(min = Target)) {
            Text(stringResource(R.string.sport_close))
        }
    }
}
