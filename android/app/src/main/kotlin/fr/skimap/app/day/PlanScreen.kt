package fr.skimap.app.day

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import fr.skimap.app.R
import fr.skimap.app.guidance.GuidanceBar
import fr.skimap.app.ui.theme.GloveTarget
import fr.skimap.core.DayPlanView
import fr.skimap.core.KeyPointView
import fr.skimap.sport.Queued
import fr.skimap.sport.SendOuting
import fr.skimap.tracking.TrackingState
import java.util.Locale

/** Test tags of the plan screen. */
object PlanTags {
    const val SEND = "plan_send"
    const val EMERGENCY = "plan_112"
    const val BACK = "plan_back"
    const val STOP = "plan_stop"
    const val RESUME = "plan_resume"
}

/** Callbacks of [PlanScreen]. */
data class PlanActions(
    val send: () -> Unit = {},
    val emergency: () -> Unit = {},
    val bringBack: () -> Unit = {},
    val stop: () -> Unit = {},
    val resume: () -> Unit = {},
    val newDay: () -> Unit = {},
    val answer: (Boolean) -> Unit = {},
    val refuseSwitch: () -> Unit = {},
    /** « Envoyer » of the recap (K11): Health Connect, Strava. */
    val sendOuting: () -> Unit = {},
)

/**
 * The plan of the day (K9, K10), during the outing: the guidance, the plan (departure,
 * expected return, limit, turnaround, group time), the actions — send the plan to the
 * contact (SMS pre-filled, sent by the user), 112 (the dialer opens), « ramène-moi », stop
 * — then the key points, the plans B and the equipment.
 */
@Composable
fun PlanScreen(
    plan: DayPlanView,
    tracking: TrackingState,
    actions: PlanActions,
    modifier: Modifier = Modifier,
    /** The outing ended and sent: where it is (K11). */
    sent: Queued? = null,
) {
    var confirmStop by rememberSaveable { mutableStateOf(false) }
    if (confirmStop) {
        StopDialog(
            onStop = {
                confirmStop = false
                actions.stop()
            },
            onClose = { confirmStop = false },
        )
    }
    val running = tracking as? TrackingState.Running
    LazyColumn(
        modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Text(
                stringResource(R.string.plan_title),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.semantics { heading() },
            )
            Text(plan.label, style = MaterialTheme.typography.titleMedium)
        }
        if (running != null) {
            item {
                GuidanceBar(running.guidance, onOpen = {}, onAnswer = actions.answer, onRefuseSwitch = actions.refuseSwitch)
                running.lastSaid?.let {
                    Text(
                        stringResource(R.string.plan_said, it),
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(top = 6.dp),
                    )
                }
            }
        }
        item { PlanSummary(plan) }
        item { PlanButtons(tracking, actions, sent, onStop = { confirmStop = true }) }
        item { PlanList(stringResource(R.string.plan_key_points), plan.keyPoints.map(::keyPointText), MaterialTheme.typography.bodyMedium) }
        if (plan.plansB.isNotEmpty()) {
            item { PlanList(stringResource(R.string.plan_plans_b), plan.plansB.map { "· $it" }, MaterialTheme.typography.bodySmall) }
        }
        if (plan.equipment.isNotEmpty()) {
            item { PlanList(stringResource(R.string.day_equipment), plan.equipment.map { "· $it" }, MaterialTheme.typography.bodySmall) }
        }
    }
}

private fun keyPointText(k: KeyPointView): String =
    "${k.at} · " + "%.1f km".format(Locale.FRANCE, k.km) + " · ${k.kind}" + (k.name?.let { " · $it" } ?: "")

@Composable
private fun StopDialog(
    onStop: () -> Unit,
    onClose: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onClose,
        text = { Text(stringResource(R.string.plan_stop_confirm), style = MaterialTheme.typography.bodyLarge) },
        confirmButton = { TextButton(onClick = onStop) { Text(stringResource(R.string.plan_stop_confirm_button)) } },
        dismissButton = { TextButton(onClick = onClose) { Text(stringResource(R.string.day_cancel)) } },
    )
}

@Composable
private fun PlanSummary(plan: DayPlanView) {
    Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(stringResource(R.string.plan_departure, plan.departure, plan.startName), style = MaterialTheme.typography.bodyLarge)
            Text(
                "%.1f km · ".format(Locale.FRANCE, plan.lengthKm) + stringResource(R.string.plan_back, plan.expectedBack),
                style = MaterialTheme.typography.bodyLarge,
            )
            plan.limit?.let { Text(stringResource(R.string.plan_limit, it), style = MaterialTheme.typography.bodyLarge) }
            plan.turnaround?.let { Text(stringResource(R.string.plan_turnaround, it), style = MaterialTheme.typography.bodyMedium) }
            plan.paceText?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
        }
    }
}

/** Send the plan, 112, « ramène-moi », stop (or resume, or a new day): one gesture each, glove size. */
@Composable
private fun PlanButtons(
    tracking: TrackingState,
    actions: PlanActions,
    sent: Queued?,
    onStop: () -> Unit,
) {
    val emergency = stringResource(R.string.plan_emergency_description)
    val big = Modifier.heightIn(min = GloveTarget)
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = actions.send, modifier = big.weight(1f).testTag(PlanTags.SEND)) { Text(stringResource(R.string.plan_send)) }
            Button(
                onClick = actions.emergency,
                colors =
                    ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.error,
                        contentColor = MaterialTheme.colorScheme.onError,
                    ),
                modifier = big.weight(1f).semantics { contentDescription = emergency }.testTag(PlanTags.EMERGENCY),
            ) { Text(stringResource(R.string.plan_emergency), style = MaterialTheme.typography.titleLarge) }
        }
        when (tracking) {
            is TrackingState.Running -> {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    val back = if (tracking.guidance.route == "back") R.string.plan_back_stop else R.string.plan_bring_back
                    OutlinedButton(onClick = actions.bringBack, modifier = big.weight(1f).testTag(PlanTags.BACK)) {
                        Text(stringResource(back))
                    }
                    OutlinedButton(onClick = onStop, modifier = big.weight(1f).testTag(PlanTags.STOP)) {
                        Text(stringResource(R.string.plan_stop))
                    }
                }
            }

            is TrackingState.Ended -> {
                // The recap, and « Envoyer » (Health Connect, Strava) when there is a trace.
                tracking.recap?.let { Text(it.text, style = MaterialTheme.typography.bodyLarge) }
                if (tracking.gpx != null) SendOuting(sent, onSend = actions.sendOuting)
                OutlinedButton(onClick = actions.newDay, modifier = big.fillMaxWidth()) { Text(stringResource(R.string.plan_new_day)) }
            }

            TrackingState.Idle -> {
                Button(onClick = actions.resume, modifier = big.fillMaxWidth().testTag(PlanTags.RESUME)) {
                    Text(stringResource(R.string.plan_resume))
                }
            }
        }
    }
}

@Composable
private fun PlanList(
    title: String,
    lines: List<String>,
    style: TextStyle,
) {
    Column {
        Text(title, style = MaterialTheme.typography.titleMedium)
        lines.forEach { Text(it, style = style) }
    }
}
