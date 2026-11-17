package fr.skimap.app.guidance

import android.content.Context
import android.content.Intent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import fr.skimap.app.R
import fr.skimap.app.day.DayActivity
import fr.skimap.tracking.Guidance
import fr.skimap.tracking.GuidanceLabels
import fr.skimap.tracking.GuidanceText
import fr.skimap.tracking.Phase
import fr.skimap.tracking.Proposal
import fr.skimap.tracking.Tracking
import fr.skimap.tracking.TrackingState
import fr.skimap.tracking.Turn

/** Rotation of the one arrow icon for each turn, degrees (straight on: 0). */
private val ARROW_DEGREES: Map<Turn, Float> =
    mapOf(
        Turn.LEFT to -90f,
        Turn.RIGHT to 90f,
        Turn.SLIGHT_LEFT to -45f,
        Turn.SLIGHT_RIGHT to 45f,
        Turn.KEEP_LEFT to -25f,
        Turn.KEEP_RIGHT to 25f,
        Turn.U_TURN to 180f,
    )

/** Rotation of the one arrow icon for a turn, degrees. */
fun arrowDegrees(t: Turn?): Float = t?.let { ARROW_DEGREES[it] } ?: 0f

/**
 * What the map shows of the day, in one place (the map screen includes it once, over the
 * map): during an outing a thin banner at the bottom, between the corner controls, with
 * the next direction and its distance, the time left and the arrival (and a proposal to
 * accept or refuse when there is one); otherwise a small « Ma journée » control at the top
 * right that opens the morning.
 */
@Composable
fun GuidanceOverlay(
    siteId: String?,
    modifier: Modifier = Modifier,
) {
    val state by Tracking.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    GuidanceOverlayContent(
        state = state,
        modifier = modifier,
        onOpen = { openDay(context, siteId) },
        onAnswer = { accept ->
            Tracking.command(context, SkiTrackingService::class.java, if (accept) Tracking.ACTION_ACCEPT else Tracking.ACTION_DECLINE)
        },
        onRefuseSwitch = { Tracking.command(context, SkiTrackingService::class.java, Tracking.ACTION_REFUSE_SWITCH) },
    )
}

private fun openDay(
    context: Context,
    siteId: String?,
) {
    context.startActivity(Intent(context, DayActivity::class.java).putExtra(DayActivity.EXTRA_SITE, siteId))
}

/** [GuidanceOverlay] for a given state (tests, captures). */
@Composable
fun GuidanceOverlayContent(
    state: TrackingState,
    onOpen: () -> Unit,
    onAnswer: (Boolean) -> Unit,
    onRefuseSwitch: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(modifier.fillMaxSize()) {
        when (state) {
            is TrackingState.Running -> {
                GuidanceBar(
                    state.guidance,
                    onOpen = onOpen,
                    onAnswer = onAnswer,
                    onRefuseSwitch = onRefuseSwitch,
                    modifier =
                        Modifier
                            .align(Alignment.BottomCenter)
                            // Clear of the 48 dp corner controls of the map.
                            .padding(horizontal = 52.dp)
                            .widthIn(max = 480.dp),
                )
            }

            else -> {
                // The main action of the map, at the bottom between the corner controls:
                // one clear button, the map stays visible around it.
                val label = stringResource(if (state is TrackingState.Ended) R.string.day_open else R.string.day_prepare)
                Button(
                    onClick = onOpen,
                    modifier =
                        Modifier
                            .align(Alignment.BottomCenter)
                            // Clear of the 48 dp corner controls of the map.
                            .padding(horizontal = 56.dp)
                            .heightIn(min = 56.dp)
                            .testTag(GuidanceTags.DAY),
                    contentPadding = PaddingValues(horizontal = 18.dp, vertical = 8.dp),
                    elevation = ButtonDefaults.buttonElevation(defaultElevation = 3.dp),
                ) {
                    Icon(painterResource(R.drawable.ic_day), contentDescription = null, modifier = Modifier.size(22.dp))
                    Text(
                        label,
                        style = MaterialTheme.typography.labelLarge,
                        // Large fonts wrap on two lines rather than cut the words.
                        maxLines = 2,
                        textAlign = TextAlign.Center,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
            }
        }
    }
}

/**
 * The thin banner: two short lines (« ← 250 m à gauche · Greffier », « 1 h 05 restantes ·
 * arrivée 15 h 20 »), about 60 dp high, translucent over the map. Touch: the plan.
 */
@Composable
fun GuidanceBar(
    g: Guidance,
    onOpen: () -> Unit,
    onAnswer: (Boolean) -> Unit,
    onRefuseSwitch: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val open = stringResource(R.string.guidance_open)
    val context = LocalContext.current
    val labels = remember(context) { GuidanceLabels.of(context) }
    Surface(
        modifier = modifier.fillMaxWidth().testTag(GuidanceTags.BAR),
        shape = RoundedCornerShape(14.dp),
        color = MaterialTheme.colorScheme.surface.copy(alpha = OVERLAY_ALPHA),
        contentColor = MaterialTheme.colorScheme.onSurface,
        shadowElevation = 3.dp,
    ) {
        Column {
            Row(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = 56.dp)
                    .clickable(onClickLabel = open, onClick = onOpen)
                    .semantics(mergeDescendants = true) {
                        contentDescription = GuidanceText.accessibility(g, labels)
                        liveRegion = LiveRegionMode.Polite
                    }.padding(horizontal = 10.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                if (g.phase == Phase.MOVING || g.phase == Phase.PAUSED || g.phase == Phase.REFUGE) {
                    Icon(
                        painterResource(R.drawable.ic_arrow),
                        contentDescription = null,
                        modifier = Modifier.size(30.dp).rotate(arrowDegrees(g.nextTurn)),
                        tint = MaterialTheme.colorScheme.primary,
                    )
                }
                // One line each; with a large font they wrap rather than lose the direction.
                val lines = if (LocalDensity.current.fontScale > LARGE_FONT) 2 else 1
                Column(Modifier.weight(1f)) {
                    Text(
                        GuidanceText.headline(g, withArrow = false, l = labels),
                        style = MaterialTheme.typography.titleMedium,
                        maxLines = lines,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        GuidanceText.short(g, labels),
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = lines,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            val p = g.proposal
            val s = g.switch
            if (p != null) {
                Column(Modifier.padding(start = 10.dp, end = 10.dp, bottom = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        if (p.kind == Proposal.BACK) stringResource(fr.skimap.tracking.R.string.back_proposal) else p.text,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        val pad = PaddingValues(horizontal = 6.dp)
                        Button(
                            onClick = { onAnswer(true) },
                            contentPadding = pad,
                            modifier = Modifier.weight(1f).heightIn(min = 56.dp).testTag(GuidanceTags.ACCEPT),
                        ) {
                            Text(stringResource(R.string.guidance_accept), style = MaterialTheme.typography.labelMedium, maxLines = 1)
                        }
                        OutlinedButton(
                            onClick = { onAnswer(false) },
                            contentPadding = pad,
                            modifier = Modifier.weight(1f).heightIn(min = 56.dp).testTag(GuidanceTags.DECLINE),
                        ) {
                            Text(stringResource(R.string.guidance_decline), style = MaterialTheme.typography.labelMedium, maxLines = 1)
                        }
                    }
                }
            } else if (s != null && s.refusable) {
                Row(
                    Modifier.padding(start = 10.dp, end = 10.dp, bottom = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(s.cause, style = MaterialTheme.typography.bodySmall, modifier = Modifier.weight(1f))
                    OutlinedButton(onClick = onRefuseSwitch, modifier = Modifier.heightIn(min = 48.dp)) {
                        Text(stringResource(R.string.guidance_keep))
                    }
                }
            }
        }
    }
}

private const val OVERLAY_ALPHA = 0.9f
private const val LARGE_FONT = 1.3f
