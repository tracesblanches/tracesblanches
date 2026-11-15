package fr.skimap.community

import android.content.res.Resources
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/** Touch targets usable with gloves (the app's GloveTarget). */
val GloveTarget = 72.dp

/** Test tags. */
object CommunityTags {
    const val KIND = "community-kind-"
    const val PHOTO = "community-photo"
    const val CANCEL = "community-cancel"
    const val CONFIRM = "community-confirm-"
    const val DENY = "community-deny-"
    const val SEND_OSM = "community-send-osm"
    const val IMPORT = "community-import"
    const val CHALLENGE = "community-challenge-"
    const val PENDING = "community-pending"
}

private const val MINUTE_MS = 60_000L
private const val HOUR_MS = 60 * MINUTE_MS
private const val DAY_MS = 24 * HOUR_MS
private const val PERCENT = 100
private const val MAX_ERRORS_SHOWN = 3

/** « il y a 3 h » / « 3 h ago ». */
fun ageText(
    res: Resources,
    nowMs: Long,
    thenMs: Long,
): String {
    val d = (nowMs - thenMs).coerceAtLeast(0)
    return when {
        d < HOUR_MS -> res.getString(R.string.age_minutes, (d / MINUTE_MS).toInt())
        d < DAY_MS -> res.getString(R.string.age_hours, (d / HOUR_MS).toInt())
        else -> res.getString(R.string.age_days, (d / DAY_MS).toInt())
    }
}

/** « encore 21 h » / « 21 h left ». */
fun leftText(
    res: Resources,
    nowMs: Long,
    untilMs: Long,
): String {
    val d = (untilMs - nowMs).coerceAtLeast(0)
    return if (d < DAY_MS) {
        res.getString(R.string.left_hours, ((d + HOUR_MS - 1) / HOUR_MS).toInt())
    } else {
        res.getString(R.string.left_days, ((d + DAY_MS - 1) / DAY_MS).toInt())
    }
}

@Composable
private fun Title(text: String) {
    Text(text, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.semantics { heading() })
}

@Composable
private fun Section(text: String) {
    HorizontalDivider(thickness = 2.dp, color = MaterialTheme.colorScheme.outline)
    Text(text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
}

/**
 * The report screen (V9, V10): six big buttons, one tap reports at the current position
 * with the time (queued, sent when the network comes back); then an optional photo and
 * « Annuler ». Below, the map slot and the reports around with their age and votes.
 */
@Composable
fun ReportScreen(
    state: ReportState,
    nowMs: Long,
    onKind: (ReportKind) -> Unit,
    onPhoto: () -> Unit,
    onCancel: () -> Unit,
    onVote: (String, Boolean) -> Unit,
    modifier: Modifier = Modifier,
    map: @Composable () -> Unit = {},
) {
    val res = LocalResources.current
    Column(
        modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Title(stringResource(R.string.report_title))
        Text(
            when {
                state.position != null -> stringResource(R.string.report_where, ageText(res, nowMs, state.position.atMs))
                state.locating -> stringResource(R.string.report_locating)
                else -> stringResource(R.string.report_no_position)
            },
            style = MaterialTheme.typography.bodyLarge,
        )
        ReportKind.entries.chunked(2).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                row.forEach { k ->
                    Button(
                        onClick = { onKind(k) },
                        enabled = state.position != null,
                        colors = ButtonDefaults.buttonColors(containerColor = Color(k.color), contentColor = Color.White),
                        modifier = Modifier.weight(1f).heightIn(min = GloveTarget).testTag(CommunityTags.KIND + k.id),
                    ) { Text(stringResource(k.label), style = MaterialTheme.typography.titleMedium) }
                }
            }
        }
        state.last?.let { last ->
            Text(
                stringResource(R.string.report_done, stringResource(last.kind.label)),
                style = MaterialTheme.typography.bodyLarge,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(
                    onClick = onPhoto,
                    enabled = !state.photoAdded,
                    modifier = Modifier.weight(1f).heightIn(min = GloveTarget).testTag(CommunityTags.PHOTO),
                ) { Text(stringResource(if (state.photoAdded) R.string.photo_added else R.string.photo_add)) }
                OutlinedButton(
                    onClick = onCancel,
                    modifier = Modifier.weight(1f).heightIn(min = GloveTarget).testTag(CommunityTags.CANCEL),
                ) { Text(stringResource(R.string.report_cancel)) }
            }
        }
        Text(
            if (state.pending > 0) {
                res.getQuantityString(R.plurals.report_pending, state.pending, state.pending)
            } else {
                stringResource(R.string.report_all_sent)
            },
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.testTag(CommunityTags.PENDING),
        )
        Section(stringResource(R.string.around_title))
        map()
        when {
            state.aroundLoaded == false -> Text(stringResource(R.string.around_offline))
            state.aroundLoaded == null -> Text(stringResource(R.string.around_loading))
            state.around.isEmpty() -> Text(stringResource(R.string.around_none))
        }
        state.around.forEach { r -> AroundRow(r, nowMs, mine = r.id in state.mine, voted = r.id in state.voted, onVote = onVote) }
        Text(stringResource(R.string.around_rules), style = MaterialTheme.typography.bodySmall)
    }
}

@Composable
private fun AroundRow(
    r: ServerReport,
    nowMs: Long,
    mine: Boolean,
    voted: Boolean,
    onVote: (String, Boolean) -> Unit,
) {
    val res = LocalResources.current
    Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(16.dp).background(Color(r.kind.color), CircleShape))
            Text(
                "  " + stringResource(r.kind.label) + " · " + ageText(res, nowMs, r.seenAtMs),
                style = MaterialTheme.typography.titleMedium,
            )
        }
        Text(
            stringResource(R.string.around_votes, r.confirmations, r.denials, leftText(res, nowMs, r.expiresAtMs)),
            style = MaterialTheme.typography.bodyMedium,
        )
        if (mine) {
            Text(stringResource(R.string.around_mine), style = MaterialTheme.typography.bodySmall)
        } else if (!voted) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(
                    onClick = { onVote(r.id, true) },
                    modifier = Modifier.weight(1f).heightIn(min = GloveTarget).testTag(CommunityTags.CONFIRM + r.id),
                ) { Text(stringResource(R.string.vote_confirm)) }
                OutlinedButton(
                    onClick = { onVote(r.id, false) },
                    modifier = Modifier.weight(1f).heightIn(min = GloveTarget).testTag(CommunityTags.DENY + r.id),
                ) { Text(stringResource(R.string.vote_deny)) }
            }
        } else {
            Text(stringResource(R.string.vote_thanks), style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
private fun ChallengeRow(c: Challenge) {
    Column(Modifier.fillMaxWidth().testTag(CommunityTags.CHALLENGE + c.id)) {
        val title =
            when (c.kind) {
                ChallengeKind.ROUTE -> stringResource(R.string.challenge_route, c.title)
                ChallengeKind.DOMAIN -> stringResource(R.string.challenge_domain, c.title)
                ChallengeKind.DISTANCE -> stringResource(R.string.challenge_distance, c.totalKm)
            }
        Text(title, style = MaterialTheme.typography.titleMedium)
        LinearProgressIndicator(progress = { c.share.toFloat() }, modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp))
        Text(
            stringResource(R.string.challenge_progress, c.doneKm, c.totalKm, (c.share * PERCENT).toInt(), c.outings) +
                (c.doneOn?.let { " · " + stringResource(R.string.challenge_done, it) } ?: ""),
            style = MaterialTheme.typography.bodyMedium,
        )
        if (c.attribution.isNotBlank()) Text(c.attribution, style = MaterialTheme.typography.bodySmall)
    }
}

/**
 * « Défis et import Strava » (V13, V15): challenges and import of the Strava export. The outings
 * themselves are the app's history (« Bilans des sorties »), where the import puts them.
 */
@Composable
fun OutingsScreen(
    state: OutingsState,
    onImport: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Title(stringResource(R.string.outings_title))
        Section(stringResource(R.string.challenges_title))
        if (state.loading) Text(stringResource(R.string.outings_loading))
        state.challenges.forEach { ChallengeRow(it) }
        Section(stringResource(R.string.strava_title))
        Text(stringResource(R.string.strava_explain), style = MaterialTheme.typography.bodyMedium)
        Button(
            onClick = onImport,
            enabled = !state.importing,
            modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(CommunityTags.IMPORT),
        ) { Text(stringResource(if (state.importing) R.string.strava_importing else R.string.strava_import)) }
        state.imported?.let {
            Text(
                stringResource(R.string.strava_result, it.imported, it.already, it.otherSports, it.withoutTrack, it.learned),
                style = MaterialTheme.typography.bodyMedium,
            )
            it.errors.take(MAX_ERRORS_SHOWN).forEach { e -> Text(e, style = MaterialTheme.typography.bodySmall) }
        }
        state.importError?.let { Text(stringResource(R.string.strava_error, it), color = MaterialTheme.colorScheme.error) }
        Text(stringResource(R.string.outings_in_history), style = MaterialTheme.typography.bodyMedium)
    }
}

/**
 * A correction of OpenStreetMap (V12): the user chooses what is wrong, may add a comment,
 * reads the exact text that will be public, and sends it themself.
 */
@Composable
fun OsmNoteScreen(
    state: OsmState,
    onCorrection: (Correction) -> Unit,
    onComment: (String) -> Unit,
    onSend: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Title(stringResource(R.string.osm_title))
        Text(stringResource(R.string.osm_explain), style = MaterialTheme.typography.bodyMedium)
        Column(Modifier.selectableGroup()) {
            Correction.entries.forEach { c ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = GloveTarget)
                        .selectable(selected = state.correction == c, role = Role.RadioButton) { onCorrection(c) },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = state.correction == c, onClick = null)
                    Text(stringResource(c.label), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 8.dp))
                }
            }
        }
        OutlinedTextField(
            value = state.comment,
            onValueChange = onComment,
            label = { Text(stringResource(R.string.osm_comment)) },
            modifier = Modifier.fillMaxWidth(),
        )
        Section(stringResource(R.string.osm_preview))
        state.note?.let { Text(it.text, style = MaterialTheme.typography.bodyMedium) }
        state.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        Text(stringResource(R.string.osm_public, state.host), style = MaterialTheme.typography.bodySmall)
        Button(
            onClick = onSend,
            enabled = state.note != null && !state.sending && state.sent == null,
            modifier = Modifier.fillMaxWidth().heightIn(min = GloveTarget).testTag(CommunityTags.SEND_OSM),
        ) { Text(stringResource(if (state.sending) R.string.osm_sending else R.string.osm_send)) }
        state.sent?.let { Text(stringResource(R.string.osm_sent, it), style = MaterialTheme.typography.bodyLarge) }
    }
}
